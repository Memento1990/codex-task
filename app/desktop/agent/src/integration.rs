//! Receipt-owned Codex configuration changes. Never adopt unrecognized entries.
use std::{fs,path::Path};
use serde_json::{json,Value};
use floatboard_client::{windows::Instance,Profile,files::{atomic_bytes,atomic_json,read_json}};
use floatboard_protocol::{Result,hash};
use toml_edit::DocumentMut;
const HOOKS:[&str;9]=["SessionStart","UserPromptSubmit","PreToolUse","PostToolUse","PermissionRequest","Stop","SessionEnd","SubagentStart","SubagentStop"];
fn command(path:&Path)->String{format!("\"{}\"",path.display())}
pub fn contribution(profile:&Profile)->Value{
    let hooks=HOOKS.into_iter().map(|event|{
        let hook=if event=="SessionEnd"{json!({"hooks":[{"type":"command","command":format!("{} --profile {}",command(&profile.agent_exe.with_file_name("FloatboardHook.exe")),command(&profile.root.join("profile.json"))),"timeout":3}]})}
        else{json!({"hooks":[{"type":"mcp_tool","server":profile.server_name(),"tool":"hook_context","input":{"hook_event_name":event,"session_id":"${session_id}","cwd":"${cwd}"},"timeout":5}]})};(event.to_string(),hook)
    }).collect::<serde_json::Map<_,_>>();
    json!({"mcp":{"url":format!("{}/mcp",profile.origin()),"tools":{"hook_context":{"approval_mode":"approve"},"report_progress":{"approval_mode":"approve"}}},"hooks":hooks})
}
fn config(profile:&Profile)->Result<(Vec<u8>,DocumentMut,Value)>{
    let path=profile.codex_home.join("config.toml");let bytes=match fs::read(path){Ok(v)=>v,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Vec::new(),Err(_)=>return Err("Codex配置不可读".into())};
    let text=std::str::from_utf8(&bytes).map_err(|_|"Codex配置编码无效")?;let doc=text.trim_start_matches('\u{feff}').parse::<DocumentMut>().map_err(|_|"Codex配置无效")?;
    let parsed=toml_edit::de::from_document(doc.clone()).map_err(|_|"Codex配置格式无效")?;Ok((bytes,doc,parsed))
}
fn lock(profile:&Profile)->Result<Instance>{
    let root=std::fs::canonicalize(&profile.codex_home).map_err(|_|"Codex目录不可解析")?;
    let digest=hash(root.to_string_lossy().to_lowercase().as_bytes());Instance::acquire_named(&format!("Config.{}",digest))
}
fn recover(profile:&Profile)->Result<()>{
    let path=profile.root.join("integration-transaction.json");if !path.exists(){return Ok(());}
    let journal=read_json(&path)?;if journal["config_path"]!=json!(profile.codex_home.join("config.toml")){return Err("接入事务目标不符".into());}
    let (before,_,_)=config(profile)?;let digest=hash(&before);
    if journal["after_hash"]==digest{atomic_json(&profile.root.join("integration.json"),&journal["receipt"])?;}
    else if journal["before_hash"]!=digest{return Err("接入事务中断后配置被其他程序修改，请离线检查；未覆盖".into());}
    fs::remove_file(path).map_err(|_|"接入事务清理失败")?;Ok(())
}
pub fn connection(profile:&Profile)->Result<Value>{
    let _lock=lock(profile)?;recover(profile)?;
    let (_,_,parsed)=config(profile)?;let desired=contribution(profile);let present=parsed["mcp_servers"][profile.server_name()]==desired["mcp"]&&HOOKS.iter().all(|event|parsed["hooks"][event].as_array().is_some_and(|items|items.contains(&desired["hooks"][event])));
    Ok(json!({"state":if present{"configured"}else{"missing"},"config_state":if present{"ready"}else{"missing"},"server_name":profile.server_name(),"codex_home":profile.codex_home,"isolated":profile.environment!="production","codex_mode":if profile.codex_home.starts_with(&profile.root){"isolated"}else{"local"},"diagnostics":[],"connected":present,"profile_id":profile.profile_id,"endpoint":format!("{}/mcp",profile.origin())}))
}
pub fn call(profile:&Profile,method:&str,params:&Value)->Result<Value>{
    change(profile,method,params,None)
}
fn change(profile:&Profile,method:&str,params:&Value,legacy:Option<Value>)->Result<Value>{
    if method=="connect_request"{return Ok(json!(format!("请在任务侧窗的Codex接入窗口预览并确认接入，然后对 {} 完成本地OAuth配对。",profile.server_name())));}
    let _lock=lock(profile)?;recover(profile)?;
    let remove=method=="disconnect"||params["remove"]==true;let (before,mut doc,parsed)=config(profile)?;let desired=contribution(profile);
    let receipt_path=profile.root.join("integration.json");let receipt=if receipt_path.exists(){Some(read_json(&receipt_path)?)}else{legacy};
    if receipt.as_ref().is_some_and(|r|r["profile_id"]!=profile.profile_id){return Err("接入回执属于其他profile，未操作".into());}
    for event in HOOKS{if !parsed["hooks"][event].is_null()&&!parsed["hooks"][event].is_array(){return Err("现有Hook配置类型无效，未覆盖".into());}}
    let old=receipt.as_ref().and_then(|r|r.get("contribution"));let current=&parsed["mcp_servers"][profile.server_name()];
    if !current.is_null()&&old.map(|v|&v["mcp"])!=Some(current)&&*current!=desired["mcp"]{return Err("Codex接入项不属于当前profile或尚未迁移，未覆盖".into());}
    if method=="preview"{let before_text=json!({"mcp":current,"owned_hooks":old.map(|v|&v["hooks"])}).to_string();return Ok(json!({"before_hash":hash(&before),"changes":[{"title":"MCP 与生命周期 Hook","key":profile.server_name(),"before":current.to_string(),"after":if remove{"移除".to_owned()}else{desired.to_string()},"diff":format!("当前配置：\n{}\n本工具{}：\n{}",before_text,if remove{"移除"}else{"贡献"},desired)}]}));}
    if params["confirmed"]!=true{return Err("接入变更需要确认预览".into());}
    if params["before_hash"].as_str()!=Some(hash(&before).as_str()){return Err("Codex配置已变化，请重新预览".into());}
    if !doc.contains_key("mcp_servers"){doc["mcp_servers"]=toml_edit::Item::Table(toml_edit::Table::new());}
    doc["mcp_servers"].as_table_mut().ok_or("Codex MCP配置不是表")?.remove(profile.server_name());
    if !remove {let generated=toml_edit::ser::to_document(&json!({"mcp_servers":{profile.server_name():desired["mcp"]}})).map_err(|_|"接入编码失败")?;doc["mcp_servers"][profile.server_name()]=generated["mcp_servers"][profile.server_name()].clone();}
    for event in HOOKS{
        let mut values=parsed["hooks"][event].as_array().cloned().unwrap_or_default();
        values.retain(|entry|old.is_none_or(|old|entry!=&old["hooks"][event])&&entry!=&desired["hooks"][event]);
        if !remove{values.push(desired["hooks"][event].clone());}
        let generated=toml_edit::ser::to_document(&json!({"hooks":{event:values}})).map_err(|_|"Hook编码失败")?;
        if !doc.contains_key("hooks"){doc["hooks"]=toml_edit::Item::Table(toml_edit::Table::new());}doc["hooks"][event]=generated["hooks"][event].clone();
    }
    // Receipt and config journal permit completion only for the exact before/after bytes.
    let after=doc.to_string().into_bytes();let journal=profile.root.join("integration-transaction.json");
    atomic_json(&journal,&json!({"before_hash":hash(&before),"after_hash":hash(&after),"after":String::from_utf8(after.clone()).map_err(|_|"接入编码无效")?,"receipt":{"profile_id":profile.profile_id,"contribution":if remove{Value::Null}else{desired.clone()}},"config_path":profile.codex_home.join("config.toml")}))?;
    atomic_bytes(&profile.codex_home.join("config.toml"),&after)?;
    atomic_json(&receipt_path,&json!({"profile_id":profile.profile_id,"contribution":if remove{Value::Null}else{desired}}))?;
    fs::remove_file(journal).map_err(|_|"接入已保存，事务记录清理失败")?;Ok(json!({"success":true,"changed":before!=after,"state":if remove{"not_connected"}else{"configured"}}))
}

/// One-time offline migration, using the exact original product receipt as ownership evidence.
pub fn migrate_legacy(profile:&Profile,source:&Path,params:&Value)->Result<Value>{
    let bytes=fs::read(source).map_err(|_|"旧接入回执不可读")?;let original:Value=serde_json::from_slice(&bytes).map_err(|_|"旧接入回执无效")?;
    if original["home"]!=json!(profile.codex_home)||original["status"]!="applied"||!original["contribution"]["mcp"].is_object()||!original["contribution"]["hook"].is_object(){return Err("旧接入归属证据不完整，禁止猜测或覆盖".into());}
    let hooks=HOOKS.into_iter().map(|event|(event.to_owned(),original["contribution"]["hooks_by_event"].get(event).cloned().unwrap_or_else(||original["contribution"]["hook"].clone()))).collect::<serde_json::Map<_,_>>();
    let receipt=json!({"profile_id":profile.profile_id,"contribution":{"mcp":original["contribution"]["mcp"],"hooks":hooks}});
    if params["confirmed"]==true&&params["legacy_receipt_sha256"]!=hash(&bytes){return Err("旧接入回执已变化，请重新预览".into());}
    let mut result=change(profile,if params["confirmed"]==true{"connect"}else{"preview"},params,Some(receipt))?;result["legacy_receipt_sha256"]=json!(hash(&bytes));Ok(result)
}

#[cfg(test)]mod tests{
use super::*;
struct Fixture{profile:Profile}
impl Fixture{fn new()->Self{let root=std::env::temp_dir().join(format!("floatboard-config-{}",uuid::Uuid::new_v4()));fs::create_dir_all(root.join("codex")).unwrap();Self{profile:Profile{profile_id:uuid::Uuid::new_v4().to_string(),owner_sid:floatboard_client::windows::sid().unwrap(),environment:"development".into(),root:root.clone(),port:59001,agent_exe:root.join("FloatboardAgent.exe"),gui_exe:root.join("CodexFloatboard.exe"),codex_home:root.join("codex")}}}}
impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.profile.root);}}
#[test]fn owned_changes_preserve_unrelated_mcp_hooks_and_comments(){
 let f=Fixture::new();let p=&f.profile;let path=p.codex_home.join("config.toml");fs::write(&path,"# keep comment\nmodel = 'synthetic'\n[mcp_servers.other]\nurl='http://127.0.0.1:3333/mcp'\n[[hooks.SessionStart]]\n[[hooks.SessionStart.hooks]]\ntype='command'\ncommand='other-hook.exe'\n").unwrap();
 let before=config(p).unwrap().2;let preview=call(p,"preview",&json!({})).unwrap();call(p,"connect",&json!({"confirmed":true,"before_hash":preview["before_hash"]})).unwrap();assert_eq!(connection(p).unwrap()["connected"],true);
 let preview=call(p,"preview",&json!({"remove":true})).unwrap();call(p,"disconnect",&json!({"confirmed":true,"before_hash":preview["before_hash"]})).unwrap();let after=config(p).unwrap().2;assert_eq!(after["mcp_servers"]["other"],before["mcp_servers"]["other"]);assert_eq!(after["hooks"]["SessionStart"],before["hooks"]["SessionStart"]);assert!(fs::read_to_string(path).unwrap().contains("# keep comment"));
}
#[test]fn changed_config_and_unrecognized_owned_name_are_never_overwritten(){let f=Fixture::new();let p=&f.profile;let path=p.codex_home.join("config.toml");let preview=call(p,"preview",&json!({})).unwrap();fs::write(&path,"model='changed'").unwrap();assert!(call(p,"connect",&json!({"confirmed":true,"before_hash":preview["before_hash"]})).unwrap_err().contains("已变化"));fs::write(&path,"[mcp_servers.codex_floatboard_dev]\ncommand='unknown.exe'").unwrap();let before=fs::read(&path).unwrap();assert!(call(p,"preview",&json!({})).is_err());assert_eq!(fs::read(path).unwrap(),before);}
#[test]fn interrupted_config_commit_recovers_only_the_exact_committed_bytes(){let f=Fixture::new();let p=&f.profile;let after=b"model='committed'";let receipt=json!({"profile_id":p.profile_id,"contribution":contribution(p)});let journal=p.root.join("integration-transaction.json");atomic_json(&journal,&json!({"config_path":p.codex_home.join("config.toml"),"before_hash":hash(b""),"after_hash":hash(after),"receipt":receipt})).unwrap();fs::write(p.codex_home.join("config.toml"),after).unwrap();recover(p).unwrap();assert_eq!(read_json(&p.root.join("integration.json")).unwrap(),receipt);assert!(!journal.exists());atomic_json(&journal,&json!({"config_path":p.codex_home.join("config.toml"),"before_hash":"unrelated","after_hash":"other","receipt":receipt})).unwrap();assert!(recover(p).is_err());assert_eq!(fs::read(p.codex_home.join("config.toml")).unwrap(),after);}
}
