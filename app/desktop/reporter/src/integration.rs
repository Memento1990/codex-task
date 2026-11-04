use std::{fs,path::{Path,PathBuf},process::{Command,Stdio},io::Write,time::{Duration,Instant}};
use std::os::windows::process::CommandExt;
use serde_json::{json,Value};
use toml_edit::DocumentMut;
use crate::{Result,store::{atomic_bytes,atomic_json,read_json,hash,text},runtime::{ProcessLock,reporter_path}};
pub const HOOKS:[&str;9]=["SessionStart","UserPromptSubmit","PreToolUse","PostToolUse","PermissionRequest","Stop","SessionEnd","SubagentStart","SubagentStop"];
/// A path component, never process-wide environment, selects the DEV owner.
/// Substrings (including CodexFloatboard-Dev-old) remain formal namespaces.
pub fn server_name(root:&Path)->&'static str{if root.components().any(|part|part.as_os_str().to_string_lossy().eq_ignore_ascii_case("CodexFloatboard-Dev")){"codex_floatboard_dev"}else{"codex_floatboard"}}
pub fn codex_home()->PathBuf{std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(||PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".codex"))}
pub fn command_line(args:&[String])->String{args.iter().map(|arg|{
    if !arg.is_empty()&&!arg.contains([' ', '\t','"']){return arg.clone();}
    let mut out=String::from("\"");let mut backslashes=0;
    for ch in arg.chars(){if ch=='\\'{backslashes+=1;}else{out.push_str(&"\\".repeat(if ch=='"'{backslashes*2+1}else{backslashes}));backslashes=0;out.push(ch);}}
    out.push_str(&"\\".repeat(backslashes*2));out.push('"');out
}).collect::<Vec<_>>().join(" ")}
fn legacy_contribution(root:&Path,reporter:&Path)->Value{
    let stable=reporter.file_name().is_some_and(|name|name.to_string_lossy().eq_ignore_ascii_case("FloatboardBridge.exe"));
    let mut args=vec![reporter.to_string_lossy().into_owned(),"--hook".into()];
    if !stable{args.extend(["--data-dir".into(),root.to_string_lossy().into_owned()]);}
    let mcp_args=if stable{json!(["--mcp"])}else{json!(["--mcp","--data-dir",root])};
    json!({"mcp":{"command":reporter,"args":mcp_args,"tools":{"report_progress":{"approval_mode":"approve"}}},"hook":{"hooks":[{"type":"command","command":command_line(&args),"timeout":5}]}})
}
/// Event-specific hooks coexist with the original command representation so
/// receipts from the nine-command route remain exact ownership evidence.
pub fn desired_hook(contribution:&Value,event:&str)->Value{contribution.get("hooks_by_event").and_then(|hooks|hooks.get(event)).cloned().unwrap_or_else(||contribution["hook"].clone())}
pub fn contribution(root:&Path,reporter:&Path)->Value{
    let mut out=legacy_contribution(root,reporter);
    if reporter.file_name().is_some_and(|name|name.to_string_lossy().eq_ignore_ascii_case("FloatboardBridge.exe")){
        out["mcp"]["tools"]["hook_context"]=json!({"approval_mode":"approve"});
        let mut hooks=serde_json::Map::new();
        for event in HOOKS.into_iter().filter(|event|*event!="SessionEnd"){
            hooks.insert(event.into(),json!({"hooks":[{"type":"mcp_tool","server":server_name(root),"tool":"hook_context","input":{"hook_event_name":event,"session_id":"${session_id}","cwd":"${cwd}"},"timeout":5}]}));
        }
        out["hooks_by_event"]=Value::Object(hooks);
    }
    // Codex caps SessionEnd at 3 seconds; preserve the historical generic
    // command hook (5s) as exact receipt evidence for older installations.
    let mut session_end=out["hook"].clone();session_end["hooks"][0]["timeout"]=json!(3);
    out["hooks_by_event"]["SessionEnd"]=session_end;
    out
}
pub struct Preview{pub path:PathBuf,pub home:PathBuf,pub before_hash:String,pub updated:Vec<u8>,pub remove:bool,pub contribution:Value,pub changes:Vec<Value>,pub present:usize,pub changed:bool,pub recovery_required:bool,pub recovery_confirmed:bool,pub receipt_before:Option<Value>,pub expected_runtime:Option<(PathBuf,Option<Value>,Value)>}
const JOURNAL:&str="integration-journal.json";
pub fn receipt_path(root:&Path)->PathBuf{crate::bridge::control_root(root).join("integration.json")}
fn load_receipt(root:&Path)->Result<Option<Value>>{
    let target=receipt_path(root);
    if let Some(receipt)=optional_json(&target)?{return Ok(Some(receipt));}
    let legacy=root.join("integration.json");
    if target!=legacy {return optional_json(&legacy);}
    Ok(None)
}
fn optional_json(path:&Path)->Result<Option<Value>>{match fs::read(path){Ok(bytes)=>serde_json::from_slice(&bytes).map(Some).map_err(|_|"接入状态格式无效".into()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(_)=>Err("接入状态不可读".into())}}
fn config_bytes(path:&Path)->Result<Vec<u8>>{match fs::read(path){Ok(bytes)=>Ok(bytes),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(Vec::new()),Err(_)=>Err("Codex 配置不可读".into())}}
fn set_receipt(root:&Path,receipt:Option<&Value>)->Result<()>{let path=receipt_path(root);if let Some(value)=receipt{atomic_json(&path,value)}else{match fs::remove_file(path){Ok(())=>Ok(()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(_)=>Err("旧接入状态恢复失败".into())}}}
fn recover_locked(root:&Path,home:&Path)->Result<bool>{
    crate::migration::check_path(&crate::bridge::control_root(root))?;
    let path=crate::bridge::control_root(root).join(JOURNAL);let Some(journal)=optional_json(&path)?else{return Ok(false);};
    if journal["version"]!=1||journal["home"]!=json!(home)||journal["config_path"]!=json!(home.join("config.toml")){return Err("接入事务目标不匹配，未修改配置".into());}
    let disk=hash(&config_bytes(&home.join("config.toml"))?);
    let before=journal["before_hash"].as_str().ok_or("接入事务缺少原配置校验")?;
    let after=journal["after_hash"].as_str().ok_or("接入事务缺少新配置校验")?;
    let receipt=load_receipt(root)?;
    let old=journal.get("old_receipt").filter(|v|!v.is_null());let new=journal.get("new_receipt").filter(|v|!v.is_null()).ok_or("接入事务缺少新归属")?;
    if receipt.as_ref()!=old&&receipt.as_ref()!=Some(new){return Err("接入归属在事务后发生变化，未覆盖；请重新检查".into());}
    let runtime=if let Some(path)=journal["runtime_path"].as_str(){
        let path=PathBuf::from(path);
        if path!=crate::bridge::control_root(root).join("runtime").join("active.json"){return Err("接入事务运行时目标无效".into());}
        crate::migration::check_path(&path)?;
        let current=optional_json(&path)?;let before=journal.get("runtime_before").filter(|v|!v.is_null());let after=journal.get("runtime_after").filter(|v|!v.is_null()).ok_or("接入事务运行时缺失")?;
        if current.as_ref()!=before&&current.as_ref()!=Some(after){return Err("连接运行时在事务后发生变化，未覆盖".into());}
        Some((path,before,after))
    }else{None};
    if disk==after {
        if let Some((path,_,after))=&runtime{atomic_json(path,after)?;}
        set_receipt(root,Some(new))?;
    }
    else if disk==before {
        if let Some((path,before,_))=&runtime{if let Some(before)=before{atomic_json(path,before)?;}else{match fs::remove_file(path){Ok(())=>{},Err(error)if error.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err("旧运行时恢复失败".into())}}}
        set_receipt(root,old)?;
    }
    else{return Err("Codex 配置在未完成接入事务后发生变化，已停止恢复；未覆盖用户修改".into());}
    fs::remove_file(path).map_err(|_|"接入事务已恢复但日志清理失败，请重试")?;Ok(true)
}
/// Recover only our two-file transaction, never restore an entire config backup.
pub fn recover_at(root:&Path,home:&Path)->Result<bool>{let _lock=ProcessLock::acquire(&crate::bridge::control_root(root).join("integration.lock")).map_err(|_|"另一个接入操作正在进行")?;let _home_lock=ProcessLock::acquire(&home.join(".floatboard-config.lock")).map_err(|_|"同一 Codex 目录的正式版或 DEV 接入操作正在进行")?;recover_locked(root,home)}
fn suspected_hook(value:&Value)->bool{let raw=value.to_string().to_lowercase();raw.contains("floatboardreporter")||raw.contains("codex_floatboard")||raw.contains("floatboardbridge")||value["hooks"].as_array().is_some_and(|hooks|hooks.iter().any(|hook|hook["type"]=="mcp_tool"&&hook["tool"]=="hook_context"))}
/// Only an exact, complete contribution under the other namespace is foreign.
/// In particular, a same-owner command cannot hide behind a foreign MCP key.
fn foreign_contribution(parsed:&Value,root:&Path)->Option<Value>{
    let foreign=if server_name(root)=="codex_floatboard"{"codex_floatboard_dev"}else{"codex_floatboard"};
    let mcp=&parsed["mcp_servers"][foreign];
    let reporter=Path::new(mcp["command"].as_str()?);
    if !reporter.is_absolute(){return None;}
    let filename=reporter.file_name()?.to_string_lossy();
    let foreign_root=if filename.eq_ignore_ascii_case("FloatboardBridge.exe"){
        // Stable bridges own their data selection; the bridge's path carries
        // the same namespace marker as its private control/data directory.
        reporter.parent()?
    }else if filename.eq_ignore_ascii_case("FloatboardReporter.exe"){
        let args=mcp["args"].as_array()?;
        if args.len()!=3||args[0]!="--mcp"||args[1]!="--data-dir"{return None;}
        let selected=Path::new(args[2].as_str()?);
        if !selected.is_absolute(){return None;}
        selected
    }else{return None;};
    if server_name(foreign_root)!=foreign{return None;}
    let current=contribution(foreign_root,reporter);
    let mut prior=current.clone();prior["hooks_by_event"].as_object_mut()?.remove("SessionEnd");
    for expected in [current,prior,legacy_contribution(foreign_root,reporter)]{
        if mcp==&expected["mcp"]&&HOOKS.iter().all(|event|parsed["hooks"][event].as_array().is_some_and(|entries|entries.iter().filter(|entry|*entry==&desired_hook(&expected,event)).count()==1)){return Some(expected);}
    }
    None
}
fn suspected_owned_hook(value:&Value,event:&str,foreign:Option<&Value>)->bool{
    suspected_hook(value)&&!foreign.is_some_and(|contribution|HOOKS.contains(&event)&&value==&desired_hook(contribution,event))
}
fn recognized_legacy(parsed:&Value,root:&Path)->Option<Value>{
    let foreign=foreign_contribution(parsed,root);
    let mcp=&parsed["mcp_servers"][server_name(root)];
    let reporter=Path::new(mcp["command"].as_str()?);
    if !reporter.is_absolute(){return None;}
    let filename=reporter.file_name()?.to_string_lossy();
    if !filename.eq_ignore_ascii_case("FloatboardReporter.exe")&&!(filename.eq_ignore_ascii_case("FloatboardBridge.exe")&&crate::bridge::integration_reporter(root).ok().as_deref()==Some(reporter)){return None;}
    let current=contribution(root,reporter);
    let mut prior=current.clone();prior["hooks_by_event"].as_object_mut()?.remove("SessionEnd");
    for expected in [current,prior,legacy_contribution(root,reporter)]{
        if mcp!=&expected["mcp"]{continue;}
        let matches=HOOKS.iter().all(|name|{
            let Some(hooks)=parsed["hooks"][name].as_array()else{return false;};
            let expected_hook=desired_hook(&expected,name);
            hooks.iter().filter(|entry|*entry==&expected_hook).count()==1&&!hooks.iter().any(|entry|entry!=&expected_hook&&suspected_owned_hook(entry,name,foreign.as_ref()))
        });
        if matches{return Some(expected);}
    }
    None
}
fn snippet(key:&str,value:&Value)->String{let mut object=value.clone();for key in key.split('.').rev(){object=json!({key:object});}toml_edit::ser::to_string_pretty(&object).unwrap_or_default()}
fn set_from(document:&mut DocumentMut,section:&str,key:&str,value:&Value)->Result<()>{
    let generated=toml_edit::ser::to_document(&json!({section:{key:value}})).map_err(|_|"接入配置编码失败")?;
    if !document.contains_key(section){document[section]=toml_edit::Item::Table(toml_edit::Table::new());}
    document[section][key]=generated[section][key].clone();Ok(())
}
pub fn preview_at(root:&Path,home:&Path,reporter:&Path,remove:bool)->Result<Preview>{preview_at_with_recovery(root,home,reporter,remove,false)}
pub fn preview_at_with_recovery(root:&Path,home:&Path,reporter:&Path,remove:bool,confirm_recovery:bool)->Result<Preview>{
    let _lock=ProcessLock::acquire(&crate::bridge::control_root(root).join("integration.lock")).map_err(|_|"另一个接入操作正在进行")?;
    let _home_lock=ProcessLock::acquire(&home.join(".floatboard-config.lock")).map_err(|_|"同一 Codex 目录的正式版或 DEV 接入操作正在进行")?;
    recover_locked(root,home)?;
    let path=home.join("config.toml");let original=config_bytes(&path)?;
    let source=std::str::from_utf8(&original).map_err(|_|"Codex 配置无法解析；未修改文件")?;let bom=source.starts_with('\u{feff}');
    let mut document=source.trim_start_matches('\u{feff}').parse::<DocumentMut>().map_err(|_|"Codex 配置无法解析；未修改文件")?;
    let parsed:Value=toml_edit::de::from_document(document.clone()).map_err(|_|"Codex 配置无法解析；未修改文件")?;
    let desired=contribution(root,reporter);
    let key=server_name(root);let foreign=foreign_contribution(&parsed,root);
    let receipt_before=load_receipt(root)?;
    let mut recovery_required=false;
    let owned=if let Some(receipt)=&receipt_before {
        if Path::new(receipt["home"].as_str().unwrap_or(""))!=home{return Err("此数据目录已绑定另一处 Codex 配置".into());}
        if receipt["status"]=="prepared"{
            recovery_required=true;
            let disk=hash(&original);
            if receipt["after_hash"]==disk {
                receipt.get("contribution").filter(|v|v["mcp"].is_object()&&v["hook"].is_object()).cloned().ok_or("旧事务新归属无效")?
            }else if receipt["before_hash"]==disk {
                // Recognize the current before image directly. Never restore a
                // whole backup or guess ownership from a command's basename.
                if let Some(old)=recognized_legacy(&parsed,root){old}
                else if parsed["mcp_servers"][key].is_null()&&!parsed["hooks"].as_object().is_some_and(|events|events.iter().any(|(event,entries)|entries.as_array().is_some_and(|entries|entries.iter().any(|entry|suspected_owned_hook(entry,event,foreign.as_ref()))))){desired.clone()}
                else{return Err("旧事务停在原配置，但无法严格识别旧归属；未修改配置".into());}
            }else{return Err("旧版未完成事务的配置与前后校验均不匹配；未恢复或覆盖".into());}
        }else{receipt.get("contribution").filter(|v|v["mcp"].is_object()&&v["hook"].is_object()).cloned().ok_or("接入归属记录无效")?}
    }else if let Some(legacy)=recognized_legacy(&parsed,root){recovery_required=true;legacy}else{desired.clone()};
    // Even an exact current contribution requires consent when ownership was lost.
    if receipt_before.is_none() && !parsed["mcp_servers"][key].is_null(){recovery_required=true;}
    let installed=&parsed["mcp_servers"][key];
    if !installed.is_null()&&installed!=&owned["mcp"]{return Err("同名 MCP 配置已被修改；工具不会覆盖".into());}
    if !parsed["hooks"].is_null()&&!parsed["hooks"].is_object(){return Err("现有 hooks 格式不兼容".into());}
    if let Some(events)=parsed["hooks"].as_object(){for (name,entries) in events {
        if !HOOKS.contains(&name.as_str()) && entries.as_array().is_some_and(|entries|entries.iter().any(|entry|suspected_owned_hook(entry,name,foreign.as_ref()))){return Err("额外事件中存在疑似看板 Hook，未自动接管".into());}
    }}
    let mut present=usize::from(!installed.is_null());let mut changes=Vec::new();
    let target=if remove{Value::Null}else{desired["mcp"].clone()};
    let mut records=vec![("本地进度连接".to_owned(),"mcp_servers".to_owned(),key.to_owned(),installed.clone(),target)];
    let names=["会话开始","收到请求","工具调用前","工具调用后","等待审批","本轮结束","会话结束","子任务开始","子任务结束"];
    for (name,title) in HOOKS.iter().zip(names){
        let owned_hook=desired_hook(&owned,name);let desired_hook=desired_hook(&desired,name);
        let raw=&parsed["hooks"][name];let current=if raw.is_null(){Vec::new()}else{raw.as_array().ok_or("现有 Hook 配置格式不兼容")?.clone()};
        if current.iter().any(|entry|entry!=&owned_hook&&suspected_owned_hook(entry,name,foreign.as_ref())){return Err(format!("{name} 中有疑似被修改的旧看板 Hook；未覆盖或追加，请先检查冲突"));}
        if current.iter().filter(|entry|*entry==&owned_hook).count()>1{return Err(format!("{name} 中存在重复看板 Hook，未自动修改"));}
        if receipt_before.is_none()&&current.contains(&owned_hook){recovery_required=true;}
        let exists=current.contains(&owned_hook);present+=usize::from(exists);
        let mut target:Vec<Value>=current.iter().filter(|v|*v!=&owned_hook).cloned().collect();
        if !remove&&!target.contains(&desired_hook){target.push(desired_hook.clone());}
        // 高级差异只展示本工具条目，绝不渲染其他 Hook 或凭据。
        let before=if exists{json!([owned_hook])}else{Value::Null};let after=if remove{Value::Null}else{json!([desired_hook])};
        if before!=after{let key=format!("hooks.{name}");changes.push(change(title,&key,&before,&after));}
        if current!=target{if target.is_empty(){if let Some(table)=document.get_mut("hooks").and_then(|i|i.as_table_mut()){table.remove(name);}}else{set_from(&mut document,"hooks",name,&json!(target))?;}}
    }
    for (title,section,key,before,after) in records.drain(..){if before!=after{changes.insert(0,change(&title,&format!("{section}.{key}"),&before,&after));if after.is_null(){if let Some(table)=document.get_mut(&section).and_then(|i|i.as_table_mut()){table.remove(&key);}}else{set_from(&mut document,&section,&key,&after)?;}}}
    for key in ["hooks","mcp_servers"]{if document.get(key).and_then(|i|i.as_table()).is_some_and(|t|t.is_empty()){document.remove(key);}}
    if receipt_before.is_none()&&recovery_required&&recognized_legacy(&parsed,root).is_none(){return Err("发现无归属记录且不完整或被修改的看板配置，未自动认领或追加 Hook".into());}
    let updated=if changes.is_empty(){original.clone()}else{format!("{}{}",if bom{"\u{feff}"}else{""},document).into_bytes()};
    if recovery_required&&changes.is_empty(){changes.push(json!({"title":"恢复旧版连接归属","key":"integration.ownership","before":"归属记录缺失","after":"用户确认后恢复本工具归属；不重写 Codex 配置","diff":"仅恢复本工具连接记录，不改变 Hook 信任。"}));}
    Ok(Preview{path,home:home.to_path_buf(),before_hash:hash(&original),changed:updated!=original,updated,remove,contribution:if remove{owned}else{desired},changes,present,recovery_required,recovery_confirmed:confirm_recovery,receipt_before,expected_runtime:None})
}
fn change(title:&str,key:&str,before:&Value,after:&Value)->Value{
    let old=if before.is_null(){String::new()}else{snippet(key,before)};let new=if after.is_null(){String::new()}else{snippet(key,after)};
    let diff=old.lines().map(|s|format!("- {s}\n")).chain(new.lines().map(|s|format!("+ {s}\n"))).collect::<String>();
    json!({"title":title,"key":key,"before":if before.is_null(){"未配置"}else{"已连接"},"after":if after.is_null(){"未配置"}else if before.is_null(){"连接本地 Rust 上报服务"}else{"更新为 Rust 上报服务"},"diff":text(&json!(diff),30000).unwrap_or_default()})
}
pub fn preview(root:&Path,remove:bool)->Result<Preview>{preview_with_recovery(root,remove,false)}
pub fn preview_with_recovery(root:&Path,remove:bool,confirm_recovery:bool)->Result<Preview>{preview_at_with_recovery(root,&codex_home(),&crate::bridge::integration_reporter(root)?,remove,confirm_recovery)}
pub fn connection_state(root:&Path)->Value{
    let observed=read_json(&root.join("hook-status.json")).ok();
    let registered=fs::read_dir(root.join("sessions")).ok().is_some_and(|entries|entries.flatten().any(|entry|entry.path().extension().is_some_and(|v|v=="json")));
    let hook_seen=observed.as_ref().is_some_and(|v|v["status"]=="ok")||registered;
    let hook_error=observed.as_ref().is_some_and(|v|v["status"]=="error");
    match preview(root,false){
        Ok(p)=>{
            let mut diagnostics=Vec::new();
            if p.present==10&&!hook_seen { diagnostics.push("接入配置已写入，但尚未收到生命周期 Hook。请在 Codex 确认启用与信任，并按客户端能力正常重载连接；仅新建标签页可能仍复用旧 app-server。若当前任务正在执行，可稍后处理，不要强制退出。连接载入后可在原会话发送同步指令。".to_string()); }
            if hook_error { diagnostics.push("Reporter 收到过 Hook，但登记或落盘失败。请检查数据目录可写性，或等待迁移完成后重试；原始输入未保存。".to_string()); }
            if hook_seen { diagnostics.push("已收到 Hook；若任务仍显示未汇报，请确认当前会话已加载 report_progress 工具并执行结构化进度汇报。简短问答可以不创建清单。".to_string()); }
            if p.changed&&p.present>0 {diagnostics.push("当前配置仍引用先前的 Reporter；请重新检查连接，完成正常 Hook 信任并重新加载 Codex。".to_string());}
            json!({"state":if p.recovery_required{"recovery_required"}else if p.present==10{"configured"}else if p.present>0{"partial"}else{"missing"},"configured":p.present==10,"recovery_required":p.recovery_required,"error":"","hook_seen":hook_seen,"hook_error":hook_error,"upgrade_available":p.changed&&p.present>0,"diagnostics":diagnostics})
        },
        Err(error)=>json!({"state":"error","error":error,"hook_seen":hook_seen})
    }
}
pub fn apply(root:&Path,plan:&Preview)->Result<bool>{
    let _lock=ProcessLock::acquire(&crate::bridge::control_root(root).join("integration.lock")).map_err(|_|"另一个接入操作正在进行")?;
    let _home_lock=ProcessLock::acquire(&plan.home.join(".floatboard-config.lock")).map_err(|_|"同一 Codex 目录的正式版或 DEV 接入操作正在进行")?;
    recover_locked(root,&plan.home)?;
    if plan.path!=plan.home.join("config.toml"){return Err("接入计划目标无效".into());}
    if plan.recovery_required&&!plan.recovery_confirmed{return Err("发现旧版连接但归属记录缺失；请确认恢复旧版连接后重试".into());}
    let original=config_bytes(&plan.path)?;
    if hash(&original)!=plan.before_hash{return Err("配置在预览后发生变化，请重新检查".into());}
    if load_receipt(root)?!=plan.receipt_before{return Err("归属记录在预览后发生变化，请重新检查".into());}
    let backup=crate::bridge::control_root(root).join("backups").join(format!("{}-{}.toml",crate::now() as u64,uuid::Uuid::new_v4().simple()));
    if plan.changed {
        fs::create_dir_all(backup.parent().unwrap()).map_err(|_|"配置备份目录不可写")?;
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&backup).map_err(|_|"配置备份不可写，未修改配置")?;
        file.write_all(&original).and_then(|_|file.sync_all()).map_err(|_|"配置备份失败，未修改配置")?;
    }
    let activation=if plan.remove{None}else{crate::bridge::activation(root)?};
    if let Some(expected)=&plan.expected_runtime{if activation.as_ref()!=Some(expected){return Err("连接运行时在预检后发生变化，未激活；请重新连接".into());}}
    let runtime_changed=activation.as_ref().is_some_and(|(_,before,after)|before.as_ref()!=Some(after));
    let refreshed=plan.changed||runtime_changed;
    let connected_at=if !refreshed{plan.receipt_before.as_ref().and_then(|receipt|receipt["connected_at"].as_f64()).unwrap_or_else(crate::now)}else{crate::now()};
    let generation=if !refreshed{plan.receipt_before.as_ref().and_then(|receipt|receipt["generation"].as_str()).map(str::to_owned).unwrap_or_else(||uuid::Uuid::new_v4().to_string())}else{uuid::Uuid::new_v4().to_string()};
    let runtime_sha256=activation.as_ref().map(|(_,_,after)|after["sha256"].clone()).or_else(||plan.receipt_before.as_ref().map(|receipt|receipt["runtime_sha256"].clone())).unwrap_or(Value::Null);
    let receipt=json!({"runtime_sha256":runtime_sha256,"generation":generation,"connected_at":connected_at,"home":plan.home,"contribution":plan.contribution,"backup":if plan.changed{json!(backup)}else{Value::Null},"before_hash":plan.before_hash,"after_hash":hash(&plan.updated),"status":if plan.remove{"removed"}else{"applied"},"implementation":"rust","reporter_version":env!("CARGO_PKG_VERSION")});
    let journal=json!({"version":1,"home":plan.home,"config_path":plan.path,"before_hash":plan.before_hash,"after_hash":hash(&plan.updated),"old_receipt":plan.receipt_before,"new_receipt":receipt,"runtime_path":activation.as_ref().map(|(path,_,_)|path),"runtime_before":activation.as_ref().and_then(|(_,before,_)|before.as_ref()),"runtime_after":activation.as_ref().map(|(_,_,after)|after)});
    let journal_path=crate::bridge::control_root(root).join(JOURNAL);
    atomic_json(&journal_path,&journal)?;
    // Never publish the new owner before the config. Every subsequent failure
    // leaves this journal in place for hash-based recovery on the next request.
    if hash(&config_bytes(&plan.path)?)!=plan.before_hash{return Err("配置出现并发修改，请重新检查；事务日志已保留".into());}
    if plan.changed{atomic_bytes(&plan.path,&plan.updated)?;}
    if hash(&config_bytes(&plan.path)?)!=hash(&plan.updated){return Err("配置提交后发生并发修改，未发布归属；请检查事务".into());}
    if let Some((path,before,after))=&activation{
        if optional_json(path)?!=*before{return Err("连接运行时在预览后发生变化，事务日志已保留".into());}
        atomic_json(path,after)?;
    }
    set_receipt(root,Some(&receipt))?;
    fs::remove_file(journal_path).map_err(|_|"接入已提交但事务日志清理失败，请重试")?;
    Ok(plan.changed||runtime_changed)
}
pub fn preflight(reporter:&Path,root:&Path)->Result<()>{
    let mut child=Command::new(reporter).args(["--mcp","--data-dir"]).arg(root).env("FLOATBOARD_PREFLIGHT","1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"本地 MCP 预检失败，未改动 Codex 配置")?;
    if let Some(mut input)=child.stdin.take(){let requests=concat!("{\"id\":1,\"method\":\"initialize\"}\n","{\"id\":2,\"method\":\"tools/list\"}\n");if input.write_all(requests.as_bytes()).is_err(){let _=child.kill();let _=child.wait();return Err("本地 MCP 预检中断".into());}}
    let deadline=Instant::now()+Duration::from_secs(12);
    loop{match child.try_wait(){Ok(Some(_))=>break,Ok(None)if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(20)),_=>{let _=child.kill();let _=child.wait();return Err("本地 MCP 预检超时".into());}}}
    let output=child.wait_with_output().map_err(|_|"本地 MCP 预检失败")?;
    let valid=String::from_utf8_lossy(&output.stdout).lines().filter_map(|line|serde_json::from_str::<Value>(line).ok()).any(|r|r["id"]==2&&r["result"]["tools"].as_array().is_some_and(|tools|["report_progress","hook_context"].iter().all(|name|tools.iter().any(|tool|tool["name"]==*name))));
    if !output.status.success()||!valid{return Err("本地 MCP 预检失败，未改动 Codex 配置".into());}Ok(())
}
pub fn connect(root:&Path)->Result<Value>{connect_with_recovery(root,false)}
pub fn connect_with_recovery(root:&Path,confirm_recovery:bool)->Result<Value>{
    let mut plan=preview_with_recovery(root,false,confirm_recovery)?;
    if plan.recovery_required&&!confirm_recovery{return Ok(json!({"success":false,"state":"recovery_required","recovery_required":true,"changes":plan.changes,"message":"发现完整匹配的旧版连接但归属记录缺失；请确认恢复后继续。未修改 Codex 配置。"}));}
    crate::bridge::install(root)?;
    let reporter=reporter_path()?;
    let source_digest=hash(&fs::read(&reporter).map_err(|_|"预检 Reporter 不可读")?);
    plan.expected_runtime=crate::bridge::activation(root)?;
    if plan.expected_runtime.as_ref().is_some_and(|(_,_,after)|after["sha256"]!=source_digest){return Err("待激活运行时与本次预检不匹配，未激活；请重新连接".into());}
    preflight(&reporter,root)?;
    if hash(&fs::read(&reporter).map_err(|_|"预检 Reporter 不可读")?)!=source_digest{return Err("Reporter 在预检期间发生变化，未激活".into());}
    let changed=apply(root,&plan)?;
    Ok(json!({"success":true,"state":"configured","changed":changed,"recovery_required":false,"message":"接入配置已就绪；请在 Codex 中完成正常信任并重新加载配置。尚未验证正式会话同步。"}))
}
