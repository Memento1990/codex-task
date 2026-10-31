use std::{fs,path::{Path,PathBuf},process::{Command,Stdio},io::Write,time::{Duration,Instant}};
use std::os::windows::process::CommandExt;
use serde_json::{json,Value};
use toml_edit::DocumentMut;
use crate::{Result,store::{atomic_bytes,atomic_json,read_json,hash,text},runtime::{ProcessLock,reporter_path}};
pub const HOOKS:[&str;9]=["SessionStart","UserPromptSubmit","PreToolUse","PostToolUse","PermissionRequest","Stop","SessionEnd","SubagentStart","SubagentStop"];
pub fn codex_home()->PathBuf{std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(||PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".codex"))}
pub fn command_line(args:&[String])->String{args.iter().map(|arg|{
    if !arg.is_empty()&&!arg.contains([' ', '\t','"']){return arg.clone();}
    let mut out=String::from("\"");let mut backslashes=0;
    for ch in arg.chars(){if ch=='\\'{backslashes+=1;}else{out.push_str(&"\\".repeat(if ch=='"'{backslashes*2+1}else{backslashes}));backslashes=0;out.push(ch);}}
    out.push_str(&"\\".repeat(backslashes*2));out.push('"');out
}).collect::<Vec<_>>().join(" ")}
pub fn contribution(root:&Path,reporter:&Path)->Value{
    let args=vec![reporter.to_string_lossy().into_owned(),"--hook".into(),"--data-dir".into(),root.to_string_lossy().into_owned()];
    json!({"mcp":{"command":reporter,"args":["--mcp","--data-dir",root],"tools":{"report_progress":{"approval_mode":"approve"}}},"hook":{"hooks":[{"type":"command","command":command_line(&args),"timeout":5}]}})
}
pub struct Preview{pub path:PathBuf,pub home:PathBuf,pub before_hash:String,pub updated:Vec<u8>,pub remove:bool,pub contribution:Value,pub changes:Vec<Value>,pub present:usize,pub changed:bool}
fn snippet(key:&str,value:&Value)->String{let mut object=value.clone();for key in key.split('.').rev(){object=json!({key:object});}toml_edit::ser::to_string_pretty(&object).unwrap_or_default()}
fn set_from(document:&mut DocumentMut,section:&str,key:&str,value:&Value)->Result<()>{
    let generated=toml_edit::ser::to_document(&json!({section:{key:value}})).map_err(|_|"接入配置编码失败")?;
    if !document.contains_key(section){document[section]=toml_edit::Item::Table(toml_edit::Table::new());}
    document[section][key]=generated[section][key].clone();Ok(())
}
pub fn preview_at(root:&Path,home:&Path,reporter:&Path,remove:bool)->Result<Preview>{
    let path=home.join("config.toml");let original=match fs::read(&path){Ok(b)=>b,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Vec::new(),Err(_)=>return Err("Codex 配置不可读".into())};
    let source=std::str::from_utf8(&original).map_err(|_|"Codex 配置无法解析；未修改文件")?;let bom=source.starts_with('\u{feff}');
    let mut document=source.trim_start_matches('\u{feff}').parse::<DocumentMut>().map_err(|_|"Codex 配置无法解析；未修改文件")?;
    let parsed:Value=toml_edit::de::from_document(document.clone()).map_err(|_|"Codex 配置无法解析；未修改文件")?;
    let desired=contribution(root,reporter);
    let owned=if root.join("integration.json").exists(){let receipt=read_json(&root.join("integration.json"))?;if Path::new(receipt["home"].as_str().unwrap_or(""))!=home{return Err("此数据目录已绑定另一处 Codex 配置".into());}receipt["contribution"].clone()}else{desired.clone()};
    let installed=&parsed["mcp_servers"]["codex_floatboard"];
    if !installed.is_null()&&installed!=&owned["mcp"]{return Err("同名 MCP 配置已被修改；工具不会覆盖".into());}
    if !parsed["hooks"].is_null()&&!parsed["hooks"].is_object(){return Err("现有 hooks 格式不兼容".into());}
    let mut present=usize::from(!installed.is_null());let mut changes=Vec::new();
    let target=if remove{Value::Null}else{desired["mcp"].clone()};
    let mut records=vec![("本地进度连接".to_owned(),"mcp_servers".to_owned(),"codex_floatboard".to_owned(),installed.clone(),target)];
    let names=["会话开始","收到请求","工具调用前","工具调用后","等待审批","本轮结束","会话结束","子任务开始","子任务结束"];
    for (name,title) in HOOKS.iter().zip(names){
        let raw=&parsed["hooks"][name];let current=if raw.is_null(){Vec::new()}else{raw.as_array().ok_or("现有 Hook 配置格式不兼容")?.clone()};
        let exists=current.contains(&owned["hook"]);present+=usize::from(exists);
        let mut target:Vec<Value>=current.iter().filter(|v|*v!=&owned["hook"]).cloned().collect();
        if !remove&&!target.contains(&desired["hook"]){target.push(desired["hook"].clone());}
        // 高级差异只展示本工具条目，绝不渲染其他 Hook 或凭据。
        let before=if exists{json!([owned["hook"]])}else{Value::Null};let after=if remove{Value::Null}else{json!([desired["hook"]])};
        if before!=after{let key=format!("hooks.{name}");changes.push(change(title,&key,&before,&after));}
        if current!=target{if target.is_empty(){if let Some(table)=document.get_mut("hooks").and_then(|i|i.as_table_mut()){table.remove(name);}}else{set_from(&mut document,"hooks",name,&json!(target))?;}}
    }
    for (title,section,key,before,after) in records.drain(..){if before!=after{changes.insert(0,change(&title,&format!("{section}.{key}"),&before,&after));if after.is_null(){if let Some(table)=document.get_mut(&section).and_then(|i|i.as_table_mut()){table.remove(&key);}}else{set_from(&mut document,&section,&key,&after)?;}}}
    for key in ["hooks","mcp_servers"]{if document.get(key).and_then(|i|i.as_table()).is_some_and(|t|t.is_empty()){document.remove(key);}}
    let updated=if changes.is_empty(){original.clone()}else{format!("{}{}",if bom{"\u{feff}"}else{""},document).into_bytes()};
    Ok(Preview{path,home:home.to_path_buf(),before_hash:hash(&original),changed:updated!=original,updated,remove,contribution:if remove{owned}else{desired},changes,present})
}
fn change(title:&str,key:&str,before:&Value,after:&Value)->Value{
    let old=if before.is_null(){String::new()}else{snippet(key,before)};let new=if after.is_null(){String::new()}else{snippet(key,after)};
    let diff=old.lines().map(|s|format!("- {s}\n")).chain(new.lines().map(|s|format!("+ {s}\n"))).collect::<String>();
    json!({"title":title,"key":key,"before":if before.is_null(){"未配置"}else{"已连接"},"after":if after.is_null(){"未配置"}else if before.is_null(){"连接本地 Rust 上报服务"}else{"更新为 Rust 上报服务"},"diff":text(&json!(diff),30000).unwrap_or_default()})
}
pub fn preview(root:&Path,remove:bool)->Result<Preview>{preview_at(root,&codex_home(),&reporter_path()?,remove)}
pub fn connection_state(root:&Path)->Value{
    let observed=read_json(&root.join("hook-status.json")).ok();
    let registered=fs::read_dir(root.join("sessions")).ok().is_some_and(|entries|entries.flatten().any(|entry|entry.path().extension().is_some_and(|v|v=="json")));
    let hook_seen=observed.as_ref().is_some_and(|v|v["status"]=="ok")||registered;
    let hook_error=observed.as_ref().is_some_and(|v|v["status"]=="error");
    match preview(root,false){
        Ok(p)=>{
            let mut diagnostics=Vec::new();
            if p.present==10&&!hook_seen { diagnostics.push("接入配置已写入，但尚未收到生命周期 Hook。请完整退出并重启 Codex Desktop（仅新建标签页可能仍复用旧 app-server），再按 Codex 的 Hook 管理界面确认启用与信任。".to_string()); }
            if hook_error { diagnostics.push("Reporter 收到过 Hook，但登记或落盘失败。请检查数据目录可写性，或等待迁移完成后重试；原始输入未保存。".to_string()); }
            if hook_seen { diagnostics.push("已收到 Hook；若任务仍显示未汇报，请确认当前会话已加载 report_progress 工具并执行结构化进度汇报。简短问答可以不创建清单。".to_string()); }
            if p.changed&&p.present>0 {diagnostics.push("当前配置仍引用先前的 Reporter；请重新检查连接，完成正常 Hook 信任并重新加载 Codex。".to_string());}
            json!({"state":if p.present==10{"ready"}else if p.present>0{"partial"}else{"missing"},"error":"","hook_seen":hook_seen,"hook_error":hook_error,"upgrade_available":p.changed&&p.present>0,"diagnostics":diagnostics})
        },
        Err(error)=>json!({"state":"error","error":error,"hook_seen":hook_seen})
    }
}
pub fn apply(root:&Path,plan:&Preview)->Result<bool>{
    let _lock=ProcessLock::acquire(&root.join("integration.lock")).map_err(|_|"另一个接入操作正在进行")?;
    if hash(&fs::read(&plan.path).unwrap_or_default())!=plan.before_hash{return Err("配置在预览后发生变化，请重新检查".into());}
    if !plan.changed{
        // A portable in-place upgrade can keep identical command paths. Refresh
        // only our receipt after preflight; never rewrite Hook trust metadata.
        if !plan.remove && plan.present==10 {
            let path=root.join("integration.json");
            let mut receipt=read_json(&path).unwrap_or(json!({"home":plan.home,"contribution":plan.contribution,"implementation":"rust","status":"applied"}));
            receipt["reporter_version"]=json!(env!("CARGO_PKG_VERSION"));
            atomic_json(&path,&receipt)?;
        }
        return Ok(false);
    }
    let backup=root.join("backups").join(format!("{}-{}.toml",crate::now() as u64,uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(backup.parent().unwrap()).map_err(|_|"配置备份目录不可写")?;fs::copy(&plan.path,&backup).or_else(|e|if e.kind()==std::io::ErrorKind::NotFound{fs::write(&backup,[]).map(|_|0)}else{Err(e)}).map_err(|_|"配置备份失败，未修改配置")?;
    if hash(&fs::read(&plan.path).unwrap_or_default())!=plan.before_hash{return Err("配置出现并发修改，请重新检查".into());}
    let mut receipt=json!({"home":plan.home,"contribution":plan.contribution,"backup":backup,"before_hash":plan.before_hash,"after_hash":hash(&plan.updated),"status":"prepared","implementation":"rust","reporter_version":env!("CARGO_PKG_VERSION")});
    atomic_json(&root.join("integration.json"),&receipt)?;atomic_bytes(&plan.path,&plan.updated)?;
    receipt["status"]=json!(if plan.remove{"removed"}else{"applied"});atomic_json(&root.join("integration.json"),&receipt)?;Ok(true)
}
pub fn preflight(reporter:&Path,root:&Path)->Result<()>{
    let mut child=Command::new(reporter).args(["--mcp","--data-dir"]).arg(root).env("FLOATBOARD_PREFLIGHT","1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"本地 MCP 预检失败，未改动 Codex 配置")?;
    if let Some(mut input)=child.stdin.take(){let requests=concat!("{\"id\":1,\"method\":\"initialize\"}\n","{\"id\":2,\"method\":\"tools/list\"}\n");if input.write_all(requests.as_bytes()).is_err(){let _=child.kill();let _=child.wait();return Err("本地 MCP 预检中断".into());}}
    let deadline=Instant::now()+Duration::from_secs(12);
    loop{match child.try_wait(){Ok(Some(_))=>break,Ok(None)if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(20)),_=>{let _=child.kill();let _=child.wait();return Err("本地 MCP 预检超时".into());}}}
    let output=child.wait_with_output().map_err(|_|"本地 MCP 预检失败")?;
    let valid=String::from_utf8_lossy(&output.stdout).lines().filter_map(|line|serde_json::from_str::<Value>(line).ok()).any(|r|r["id"]==2&&r["result"]["tools"].as_array().is_some_and(|tools|tools.iter().any(|t|t["name"]=="report_progress")));
    if !output.status.success()||!valid{return Err("本地 MCP 预检失败，未改动 Codex 配置".into());}Ok(())
}
pub fn connect(root:&Path)->Result<Value>{let plan=preview(root,false)?;preflight(&reporter_path()?,root)?;let changed=apply(root,&plan)?;Ok(json!({"success":true,"state":"ready","changed":changed,"message":"接入配置已就绪；请完整重启 Codex Desktop 或重新加载 CLI 配置，完成正常 Hook 信任后开始任务。收到 Hook 后才能确认同步链路。"}))}
