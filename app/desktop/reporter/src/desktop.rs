use std::{collections::HashMap,path::{Path,PathBuf}};
use serde_json::{json,Value};
use crate::{Result,now,store::{Store,read_json,atomic_json},integration};

pub fn defaults()->Value{json!({"auto_start":false,"show_taskbar":false,"theme":"light","notifications":true,"notification_seconds":3,"startup_command":""})}
pub fn validate_options(value:&Value)->Result<Value>{
    if !value.is_object(){return Err("设置格式无效".into());}let mut out=defaults();for key in ["auto_start","show_taskbar","theme","notifications","notification_seconds"]{if let Some(value)=value.get(key){out[key]=value.clone();}}
    if ["auto_start","show_taskbar","notifications"].iter().any(|k|!out[k].is_boolean()){return Err("开关设置无效".into());}
    if ![Some("light"),Some("dark")].contains(&out["theme"].as_str()){return Err("主题设置无效".into());}
    if !out["notification_seconds"].as_u64().is_some_and(|n|(1..=30).contains(&n)){return Err("通知停留时间须为 1–30 秒".into());}Ok(out)
}
pub fn load_options(root:&Path)->Value{read_json(&root.join("preferences.json")).ok().and_then(|raw|validate_options(&raw).ok().map(|mut valid|{valid["startup_command"]=raw.get("startup_command").cloned().unwrap_or(json!(""));valid})).unwrap_or_else(defaults)}
const RUN_KEY:&str=r"Software\Microsoft\Windows\CurrentVersion\Run";
fn startup(enabled:bool,desired:&str,owned:&str)->Result<Option<String>>{
    startup_at(RUN_KEY,enabled,desired,owned)
}
fn startup_at(key_path:&str,enabled:bool,desired:&str,owned:&str)->Result<Option<String>>{
    let (key,_)=winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).create_subkey(key_path).map_err(|_|"自启设置不可写")?;
    let current:Option<String>=match key.get_value("CodexFloatboard"){Ok(v)=>Some(v),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>None,Err(_)=>return Err("自启项不可读".into())};
    if current.as_ref().is_some_and(|s|s!=desired&&s!=owned){return Err("自启项已由其他配置占用，未覆盖".into());}
    if enabled{key.set_value("CodexFloatboard",&desired).map_err(|_|"自启设置失败")?;}else if current.is_some(){key.delete_value("CodexFloatboard").map_err(|_|"自启撤销失败")?;}Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_ownership_roundtrip_in_isolated_registry_key(){
        let path=format!(r"Software\CodexFloatboardTests\{}",uuid::Uuid::new_v4().simple());
        let user=winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let result=std::panic::catch_unwind(||{
            assert_eq!(startup_at(&path,true,"old.exe --data-dir isolated","").unwrap(),None);
            assert!(startup_at(&path,true,"new.exe","unowned.exe").is_err());
            assert_eq!(startup_at(&path,true,"new.exe","old.exe --data-dir isolated").unwrap().as_deref(),Some("old.exe --data-dir isolated"));
            assert_eq!(startup_at(&path,false,"new.exe","new.exe").unwrap().as_deref(),Some("new.exe"));
            assert!(user.open_subkey(&path).unwrap().get_value::<String,_>("CodexFloatboard").is_err());
        });
        let _=user.delete_subkey_all(&path);assert!(result.is_ok());
    }
    #[test]
    fn secondary_snapshot_cannot_consume_main_notifications(){
        let root=std::env::temp_dir().join(format!("floatboard-notify-{}",uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let result=(||{
            let mut store=Store::open(&root,false).unwrap();
            let mut engine=Engine::new(root.clone(),root.join("CodexFloatboard.exe"));
            engine.connection=Some(json!({"state":"ready"}));engine.checked_at=now();
            assert!(engine.call("snapshot",json!({"notify":true})).unwrap()["notices"].as_array().unwrap().is_empty());
            let report=json!({"revision":0,"title":"通知隔离验证","status":"running","plan":[],"blockers":[],"evidence":[]});
            let event=json!({"event_id":"notice-1","kind":"progress","recorded_at":now(),"session_id":"notice","host_id":"synthetic","report":report});
            store.ingest(&event).unwrap();
            assert!(engine.call("snapshot",json!({"notify":false})).unwrap()["notices"].as_array().unwrap().is_empty());
            assert_eq!(engine.call("snapshot",json!({"notify":true})).unwrap()["notices"].as_array().unwrap().len(),1);
            drop(engine);drop(store);
        })();
        let _=std::fs::remove_dir_all(&root);result
    }
}
pub fn save_options(root:&Path,gui:&Path,value:&Value)->Result<Value>{
    let previous=load_options(root);let mut out=validate_options(value)?;let desired=integration::command_line(&[gui.to_string_lossy().into_owned(),"--data-dir".into(),root.to_string_lossy().into_owned()]);
    let changed=out["auto_start"]!=previous["auto_start"]||out["auto_start"]==true&&previous["startup_command"]!=desired;
    let old=if changed{startup(out["auto_start"]==true,&desired,previous["startup_command"].as_str().unwrap_or(""))?}else{None};
    out["startup_command"]=json!(if out["auto_start"]==true{desired.clone()}else{String::new()});
    if let Err(error)=atomic_json(&root.join("preferences.json"),&out){if changed{let _=startup(old.is_some(),old.as_deref().unwrap_or(&desired),&desired);}return Err(error);}Ok(out)
}
fn label(status:&str)->&str{match status{"running"=>"进行中","blocked"=>"有卡点","waiting"=>"待确认","completed"=>"已完成","cancelled"=>"已取消",_=>"未汇报"}}
fn step_label(status:&str)->&str{match status{"inProgress"=>"进行中","blocked"=>"受阻","completed"=>"已完成",_=>"待执行"}}
fn terminal(task:&Value)->bool{["completed","cancelled"].contains(&task["status"].as_str().unwrap_or(""))}
#[derive(Default)]
pub struct ChangeDetector{previous:Option<HashMap<String,Value>>}
impl ChangeDetector{
    pub fn update(&mut self,tasks:&[Value])->Vec<Value>{
        let mut current=HashMap::new();let mut notices=Vec::new();
        for task in tasks.iter().filter(|t|t["status"]!="unreported"||t["plan"].as_array().is_some_and(|p|!p.is_empty())){
            let id=task["id"].as_str().unwrap_or("").to_owned();let old=self.previous.as_ref().and_then(|p|p.get(&id));let mut lines=Vec::new();let mut tab=0;
            if self.previous.is_some(){
                if old.is_none()||old.is_some_and(|o|terminal(o)&&!terminal(task)&&o["title"]!=task["title"]){lines.push(format!("新增任务 · {}",label(task["status"].as_str().unwrap_or(""))));if let Some(blockers)=task["blockers"].as_array(){for b in blockers.iter().filter(|b|b["open"]==true){lines.push(format!("新增卡点 · {}",b["title"].as_str().unwrap_or("")));tab=1;}}}
                else if let Some(old)=old{
                    if old["status"]!=task["status"]{lines.push(format!("任务 · {} → {}",label(old["status"].as_str().unwrap_or("")),label(task["status"].as_str().unwrap_or(""))));}
                    if let Some(plan)=task["plan"].as_array(){for item in plan{let previous=old["plan"].as_array().and_then(|p|p.iter().find(|s|s["id"]==item["id"]));let title=item["title"].as_str().unwrap_or("");if previous.is_none(){lines.push(format!("新增清单 · {title}"));}else if previous.unwrap()["status"]!=item["status"]{lines.push(format!("{title} · {}",step_label(item["status"].as_str().unwrap_or(""))));}}}
                    if let Some(blockers)=task["blockers"].as_array(){for item in blockers{let previous=old["blockers"].as_array().and_then(|p|p.iter().find(|s|s["id"]==item["id"]));if previous.is_none()&&item["open"]==true||previous.is_some_and(|p|p["open"]!=item["open"]){let prefix=if previous.is_none(){"新增卡点"}else if item["open"]==true{"卡点待处理"}else{"卡点已解除"};lines.insert(0,format!("{prefix} · {}",item["title"].as_str().unwrap_or("")));tab=1;}}}
                    if let Some(blockers)=old["blockers"].as_array(){for item in blockers.iter().filter(|b|b["open"]==true){if !task["blockers"].as_array().is_some_and(|p|p.iter().any(|b|b["id"]==item["id"])){lines.insert(0,format!("卡点已解除 · {}",item["title"].as_str().unwrap_or("")));tab=1;}}}
                }
                if !lines.is_empty(){let remaining=lines.len().saturating_sub(3);lines.truncate(3);if remaining>0{lines.push(format!("另有 {remaining} 项变化"));}notices.push(json!({"task_id":id,"title":task["title"].as_str().filter(|s|!s.is_empty()).unwrap_or("任务更新"),"lines":lines,"tab":tab}));}
            }
            current.insert(id,json!({"id":task["id"],"title":task["title"],"status":task["status"],"plan":task["plan"],"blockers":task["blockers"]}));
        }self.previous=Some(current);notices
    }
}
pub struct Engine{pub root:PathBuf,gui:PathBuf,store:Option<Store>,notices:ChangeDetector,connection:Option<Value>,checked_at:f64,restart_required:bool}
impl Engine{
    pub fn new(root:PathBuf,gui:PathBuf)->Self{Self{root,gui,store:None,notices:ChangeDetector::default(),connection:None,checked_at:0.,restart_required:false}}
    pub fn data_location(&self)->Value{crate::migration::location(&self.root,self.restart_required)}
    pub fn migrate_data(&mut self,path:&Path)->Result<Value>{
        if self.restart_required{return Err("数据目录已切换，请先重启看板".into());}
        let receipt=self.root.join("integration.json");
        if receipt.exists() {
            let receipt=read_json(&receipt)?;
            if receipt["reporter_version"]!=env!("CARGO_PKG_VERSION") {
                return Err("请先用新版重新连接 Codex，完整重启 Codex 后再迁移".into());
            }
        }
        self.store=None;
        let previous=self.root.clone();
        let destination=match crate::migration::migrate(&self.root,path){
            Ok(destination)=>destination,
            Err(error)=>{let _=crate::runtime::spawn_worker(&self.root);return Err(error);}
        };
        self.root=destination;self.restart_required=true;self.connection=None;self.notices=ChangeDetector::default();
        let started=crate::runtime::spawn_worker(&self.root);
        Ok(json!({"success":true,"current":self.root,"path":self.root,"previous":previous,"restart_required":true,"worker_started":started.is_ok(),"warning":started.err(),"old_data_retained":true}))
    }
    pub fn call(&mut self,method:&str,params:Value)->Result<Value>{
        let current=crate::migration::resolve_root(&self.root)?;
        if current!=self.root{self.store=None;self.root=current;self.restart_required=true;self.connection=None;}
        if method=="data_location"{return Ok(self.data_location());}
        if method=="migrate_data"{if params["producers_stopped"]!=true{return Err("迁移前必须退出全部 Codex 会话及其他看板，确认已暂停上报".into());}return self.migrate_data(Path::new(params["path"].as_str().filter(|p|!p.is_empty()).ok_or("请选择新的数据目录")?));}
        if self.restart_required && !["snapshot","window_load"].contains(&method){return Err("数据目录已切换，请先重启看板".into());}
        if !["preferences","window_save"].contains(&method){return self.call_inner(method,params);}
        let root=self.root.clone();
        crate::migration::with_root(&root,|resolved|{
            if resolved!=self.root{self.store=None;self.root=resolved.to_path_buf();self.restart_required=true;self.connection=None;return Err("数据目录已切换，请重启看板".into());}
            self.call_inner(method,params)
        })
    }
    fn call_inner(&mut self,method:&str,params:Value)->Result<Value>{match method{
        "snapshot"=>{
            if self.store.is_none(){self.store=Some(Store::open(&self.root,true)?);}let tasks=self.store.as_mut().unwrap().tasks()?;
            let notices=if params["notify"]!=false{self.notices.update(&tasks)}else{Vec::new()};let options=load_options(&self.root);
            let healthy=read_json(&self.root.join("health.json")).is_ok_and(|v|v["status"]=="ok"&&v["at"].as_f64().is_some_and(|at|(0.0..5.).contains(&(now()-at))));
            if self.connection.is_none()||now()-self.checked_at>3.{self.connection=Some(integration::connection_state(&self.root));self.checked_at=now();}
            Ok(json!({"tasks":tasks,"notices":if options["notifications"]==true{notices}else{Vec::new()},"options":options,"healthy":healthy,"connection":self.connection,"data_location":self.data_location(),"restart_required":self.restart_required,"now":now()}))
        },
        "preferences"=>save_options(&self.root,&self.gui,&params),
        "preview"=>Ok(json!({"changes":integration::preview(&self.root,params["remove"]==true)?.changes})),
        "connect"=>{let result=integration::connect(&self.root)?;self.connection=None;Ok(result)},
        "connect_request"=>Ok(json!(format!("请帮我接入当前 Windows 用户的 Codex 任务看板，保留其他配置和正常 Hook 信任流程。请执行：{}",integration::command_line(&[crate::runtime::reporter_path()?.to_string_lossy().into_owned(),"--connect".into(),"--data-dir".into(),self.root.to_string_lossy().into_owned()])))),
        "disconnect"=>{let plan=integration::preview(&self.root,true)?;let changed=integration::apply(&self.root,&plan)?;self.connection=None;Ok(json!({"success":true,"changed":changed}))},
        "window_load"=>Ok(read_json(&self.root.join("window.json")).ok().filter(|v|v.is_object()).unwrap_or(json!({}))),
        "window_save"=>{let mut old=self.call_inner("window_load",json!({}))?;for key in ["x","y","width","height","pinned","selected","tab","filter"]{if let Some(value)=params.get(key){let valid=match key{"pinned"=>value.is_boolean(),"selected"|"filter"=>value.as_str().is_some_and(|s|s.len()<=500),"tab"=>value.as_u64().is_some_and(|n|n<=3),"width"=>value.as_f64().is_some_and(|n|(392.0..=100000.).contains(&n)),"height"=>value.as_f64().is_some_and(|n|(570.0..=100000.).contains(&n)),_=>value.as_f64().is_some_and(|n|n.is_finite()&&n.abs()<=100000.)};if !valid{return Err("窗口设置无效".into());}old[key]=value.clone();}}atomic_json(&self.root.join("window.json"),&old)?;Ok(json!(true))},
        _=>Err("未知看板操作".into())
    }}
}
