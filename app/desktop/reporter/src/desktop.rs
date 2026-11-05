use std::{collections::HashMap,path::{Path,PathBuf}};
use serde_json::{json,Value};
use crate::{Result,now,store::{Store,read_json,atomic_json},integration};

pub fn defaults()->Value{json!({"auto_start":false,"show_taskbar":false,"theme":"dark","notifications":true,"notification_seconds":3,"dock_hover_summary":true,"dock_summary_mode":"hover","startup_command":""})}
pub fn validate_options(value:&Value)->Result<Value>{
    if !value.is_object(){return Err("设置格式无效".into());}let mut out=defaults();for key in ["auto_start","show_taskbar","theme","notifications","notification_seconds","dock_hover_summary","dock_summary_mode"]{if let Some(value)=value.get(key){out[key]=value.clone();}}
    if ["auto_start","show_taskbar","notifications","dock_hover_summary"].iter().any(|k|!out[k].is_boolean()){return Err("开关设置无效".into());}
    if value.get("dock_summary_mode").is_none(){out["dock_summary_mode"]=json!(if out["dock_hover_summary"]==false{"never"}else{"hover"});}
    if ![Some("always"),Some("hover"),Some("never")].contains(&out["dock_summary_mode"].as_str()){return Err("任务摘要显示模式无效".into());}
    out["dock_hover_summary"]=json!(out["dock_summary_mode"]!="never");
    if ![Some("light"),Some("dark")].contains(&out["theme"].as_str()){return Err("主题设置无效".into());}
    if !out["notification_seconds"].as_u64().is_some_and(|n|(1..=60).contains(&n)){return Err("通知停留时间须为 1–60 秒".into());}Ok(out)
}
pub fn load_options(root:&Path)->Value{read_json(&root.join("preferences.json")).ok().and_then(|raw|validate_options(&raw).ok().map(|mut valid|{valid["startup_command"]=raw.get("startup_command").cloned().unwrap_or(json!(""));valid})).unwrap_or_else(defaults)}
pub fn merge_options(previous:&Value,patch:&Value)->Result<Value>{
    if !patch.is_object(){return Err("设置格式无效".into());}
    let mut merged=previous.clone();
    for key in ["auto_start","show_taskbar","theme","notifications","notification_seconds","dock_hover_summary","dock_summary_mode"]{if let Some(value)=patch.get(key){merged[key]=value.clone();}}
    if patch.get("dock_summary_mode").is_none()&&patch.get("dock_hover_summary").is_some(){merged["dock_summary_mode"]=json!(if merged["dock_hover_summary"]==false{"never"}else{"hover"});}
    let mut out=validate_options(&merged)?;out["startup_command"]=previous.get("startup_command").cloned().unwrap_or(json!(""));Ok(out)
}
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
    struct TempRoot(PathBuf);
    impl TempRoot{fn new()->Self{let path=std::env::temp_dir().join(format!("floatboard-options-{}",uuid::Uuid::new_v4().simple()));std::fs::create_dir_all(&path).unwrap();Self(path)}}
    impl Drop for TempRoot{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
    #[test]
    fn preference_patches_merge_latest_values_and_preserve_saved_light_theme(){
        let temp=TempRoot::new();let gui=temp.0.join("CodexFloatboard.exe");
        assert_eq!(load_options(&temp.0)["theme"],"dark");
        atomic_json(&temp.0.join("preferences.json"),&json!({"theme":"light","notifications":false,"notification_seconds":60,"dock_summary_mode":"always"})).unwrap();
        let mut engine=Engine::new(temp.0.clone(),gui);
        let saved=engine.call("preferences",json!({"show_taskbar":true})).unwrap();
        assert_eq!(saved["theme"],"light");assert_eq!(saved["notifications"],false);assert_eq!(saved["notification_seconds"],60);assert_eq!(saved["dock_summary_mode"],"always");
        let saved=engine.call("preferences",json!({"notification_seconds":45})).unwrap();
        assert_eq!(saved["show_taskbar"],true);assert_eq!(saved["theme"],"light");assert_eq!(saved["notifications"],false);
        let before=std::fs::read(temp.0.join("preferences.json")).unwrap();
        assert!(engine.call("preferences",json!({"notification_seconds":61})).is_err());
        assert_eq!(std::fs::read(temp.0.join("preferences.json")).unwrap(),before);
    }
    #[test]
    fn preference_patch_respects_legacy_summary_and_internal_startup_ownership(){
        let mut previous=defaults();previous["dock_summary_mode"]=json!("always");previous["startup_command"]=json!("owned.exe");
        let saved=merge_options(&previous,&json!({"theme":"light","startup_command":"unowned.exe"})).unwrap();
        assert_eq!(saved["dock_summary_mode"],"always");assert_eq!(saved["startup_command"],"owned.exe");
        assert_eq!(merge_options(&previous,&json!({"dock_hover_summary":false})).unwrap()["dock_summary_mode"],"never");
        assert_eq!(merge_options(&previous,&json!({"dock_summary_mode":"always","dock_hover_summary":false})).unwrap()["dock_hover_summary"],true);
        assert!(merge_options(&previous,&json!({"dock_hover_summary":"false"})).is_err());
    }
    #[test]
    fn startup_failure_does_not_commit_preferences(){
        let temp=TempRoot::new();let previous=defaults();atomic_json(&temp.0.join("preferences.json"),&previous).unwrap();
        let error=save_options_with_startup(&temp.0,&temp.0.join("gui.exe"),&json!({"auto_start":true}),false,|_,_,_|Err("fixture startup failure".into())).unwrap_err();
        assert!(error.contains("fixture startup failure"));assert_eq!(load_options(&temp.0),previous);
    }
    #[test]
    fn preference_write_failure_rolls_back_startup_and_reports_failed_rollback(){
        for fail_rollback in [false,true]{
            let temp=TempRoot::new();std::fs::create_dir(temp.0.join("preferences.json")).unwrap();let mut calls=Vec::new();
            let error=save_options_with_startup(&temp.0,&temp.0.join("gui.exe"),&json!({"auto_start":true}),false,|enabled,desired,owned|{
                calls.push((enabled,desired.to_owned(),owned.to_owned()));
                if fail_rollback&&calls.len()==2{Err("fixture rollback failure".into())}else{Ok(None)}
            }).unwrap_err();
            assert_eq!(calls.len(),2);assert!(calls[0].0);assert!(!calls[1].0);assert_eq!(calls[1].2,calls[0].1);
            assert_eq!(error.contains("开机启动恢复失败"),fail_rollback);assert!(temp.0.join("preferences.json").is_dir());
        }
    }
    #[test]
    fn isolated_preferences_never_attempt_startup(){
        let temp=TempRoot::new();let gui=temp.0.join("gui.exe");
        assert!(save_options_with_startup(&temp.0,&gui,&json!({"auto_start":true}),true,|_,_,_|panic!("must not write startup")).is_err());
        let saved=save_options_with_startup(&temp.0,&gui,&json!({"theme":"light","notification_seconds":60}),true,|_,_,_|panic!("must not write startup")).unwrap();
        assert_eq!(saved["theme"],"light");assert_eq!(saved["notification_seconds"],60);assert_eq!(saved["startup_command"],"");
    }
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
    fn archived_completions_do_not_create_notifications(){
        let mut detector=ChangeDetector::default();assert!(detector.update(&[]).is_empty());
        assert!(detector.update(&[json!({"id":"archive:1","archived":true,"status":"completed","title":"旧任务","plan":[],"blockers":[]})]).is_empty());
    }
    #[test]
    fn blocker_notices_are_sticky_until_resolved(){
        let mut detector=ChangeDetector::default();
        let mut task=json!({"id":"sticky","title":"卡点测试","status":"running","plan":[],"blockers":[]});
        assert!(detector.update(&[task.clone()]).is_empty());
        task["status"]=json!("blocked");task["blockers"]=json!([{"id":"b","title":"等待依赖","open":true}]);
        let notices=detector.update(&[task.clone()]);assert_eq!(notices[0]["sticky"],true);assert_eq!(notices[0]["tab"],1);
        task["status"]=json!("running");task["blockers"][0]["open"]=json!(false);
        let notices=detector.update(&[task]);assert_eq!(notices[0]["sticky"],false);
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
    let isolated=std::env::var("FLOATBOARD_DEVELOPMENT").as_deref()==Ok("1") || std::env::args().any(|arg|arg=="--isolated-instance");
    save_options_with_startup(root,gui,value,isolated,startup)
}
fn save_options_with_startup(root:&Path,gui:&Path,value:&Value,isolated:bool,mut update_startup:impl FnMut(bool,&str,&str)->Result<Option<String>>)->Result<Value>{
    let previous=load_options(root);let mut out=merge_options(&previous,value)?;
    if isolated{if out["auto_start"]==true{return Err("隔离开发/测试实例不能设置系统开机启动".into());}out["startup_command"]=json!("");atomic_json(&root.join("preferences.json"),&out)?;return Ok(out);}
    let desired=integration::command_line(&[crate::update_install::launch_entry(gui).to_string_lossy().into_owned(),"--data-dir".into(),root.to_string_lossy().into_owned()]);
    let changed=out["auto_start"]!=previous["auto_start"]||out["auto_start"]==true&&previous["startup_command"]!=desired;
    let old=if changed{update_startup(out["auto_start"]==true,&desired,previous["startup_command"].as_str().unwrap_or(""))?}else{None};
    out["startup_command"]=json!(if out["auto_start"]==true{desired.clone()}else{String::new()});
    if let Err(error)=atomic_json(&root.join("preferences.json"),&out){
        if changed{if let Err(rollback)=update_startup(old.is_some(),old.as_deref().unwrap_or(&desired),&desired){return Err(format!("{error}；开机启动恢复失败：{rollback}"));}}
        return Err(error);
    }Ok(out)
}
fn label(status:&str)->&str{match status{"running"=>"进行中","blocked"=>"有卡点","waiting"=>"待确认","completed"=>"已完成","cancelled"=>"已取消",_=>"未汇报"}}
fn step_label(status:&str)->&str{match status{"inProgress"=>"进行中","blocked"=>"受阻","completed"=>"已完成",_=>"待执行"}}
fn terminal(task:&Value)->bool{["completed","cancelled"].contains(&task["status"].as_str().unwrap_or(""))}
#[derive(Default)]
pub struct ChangeDetector{previous:Option<HashMap<String,Value>>}
impl ChangeDetector{
    pub fn update(&mut self,tasks:&[Value])->Vec<Value>{
        let mut current=HashMap::new();let mut notices=Vec::new();
        for task in tasks.iter().filter(|t|t["archived"]!=true&&(t["status"]!="unreported"||t["plan"].as_array().is_some_and(|p|!p.is_empty()))){
            let id=task["id"].as_str().unwrap_or("").to_owned();let old=self.previous.as_ref().and_then(|p|p.get(&id));let mut lines=Vec::new();let mut tab=0;
            if self.previous.is_some(){
                if old.is_none()||old.is_some_and(|o|terminal(o)&&!terminal(task)&&o["title"]!=task["title"]){lines.push(format!("新增任务 · {}",label(task["status"].as_str().unwrap_or(""))));if let Some(blockers)=task["blockers"].as_array(){for b in blockers.iter().filter(|b|b["open"]==true){lines.push(format!("新增卡点 · {}",b["title"].as_str().unwrap_or("")));tab=1;}}}
                else if let Some(old)=old{
                    if old["status"]!=task["status"]{lines.push(format!("任务 · {} → {}",label(old["status"].as_str().unwrap_or("")),label(task["status"].as_str().unwrap_or(""))));}
                    if let Some(plan)=task["plan"].as_array(){for item in plan{let previous=old["plan"].as_array().and_then(|p|p.iter().find(|s|s["id"]==item["id"]));let title=item["title"].as_str().unwrap_or("");if previous.is_none(){lines.push(format!("新增清单 · {title}"));}else if previous.unwrap()["status"]!=item["status"]{lines.push(format!("{title} · {}",step_label(item["status"].as_str().unwrap_or(""))));}}}
                    if let Some(blockers)=task["blockers"].as_array(){for item in blockers{let previous=old["blockers"].as_array().and_then(|p|p.iter().find(|s|s["id"]==item["id"]));if previous.is_none()&&item["open"]==true||previous.is_some_and(|p|p["open"]!=item["open"]){let prefix=if previous.is_none(){"新增卡点"}else if item["open"]==true{"卡点待处理"}else{"卡点已解除"};lines.insert(0,format!("{prefix} · {}",item["title"].as_str().unwrap_or("")));tab=1;}}}
                    if let Some(blockers)=old["blockers"].as_array(){for item in blockers.iter().filter(|b|b["open"]==true){if !task["blockers"].as_array().is_some_and(|p|p.iter().any(|b|b["id"]==item["id"])){lines.insert(0,format!("卡点已解除 · {}",item["title"].as_str().unwrap_or("")));tab=1;}}}
                }
                if !lines.is_empty(){let remaining=lines.len().saturating_sub(3);lines.truncate(3);if remaining>0{lines.push(format!("另有 {remaining} 项变化"));}notices.push(json!({"task_id":id,"title":task["title"].as_str().filter(|s|!s.is_empty()).unwrap_or("任务更新"),"lines":lines,"tab":tab,"sticky":task["blockers"].as_array().is_some_and(|items|items.iter().any(|item|item["open"]==true))}));}
            }
            current.insert(id,json!({"id":task["id"],"title":task["title"],"status":task["status"],"plan":task["plan"],"blockers":task["blockers"]}));
        }self.previous=Some(current);notices
    }
}
pub struct Engine{pub root:PathBuf,gui:PathBuf,collector_start_error:Option<String>,store:Option<Store>,notices:ChangeDetector,connection:Option<Value>,checked_at:f64,restart_required:bool,repair_pending:bool,notifications:Option<std::sync::Arc<crate::notifications::NotificationService>>}
impl Engine{
    pub fn new(root:PathBuf,gui:PathBuf)->Self{Self{root,gui,collector_start_error:None,store:None,notices:ChangeDetector::default(),connection:None,checked_at:0.,restart_required:false,repair_pending:false,notifications:None}}
    /// Quiesce all local readers/producers before the independent repair helper starts.
    pub fn begin_repair(&mut self)->Result<PathBuf>{
        if self.repair_pending || self.restart_required{return Err("看板正在准备重启，请稍后重试".into());}
        self.repair_pending=true;
        if let Some(service)=self.notifications.take(){service.shutdown();}
        self.store=None;
        Ok(self.root.clone())
    }
    pub fn cancel_repair(&mut self){self.repair_pending=false;self.start_notifications();}
    pub fn set_collector_start_result(&mut self,result:Result<()>){self.collector_start_error=result.err();}
    pub fn start_notifications(&mut self){if !self.repair_pending && !self.restart_required && self.notifications.is_none(){self.notifications=Some(std::sync::Arc::new(crate::notifications::NotificationService::new(self.root.clone())));}}
    /// Clone while locked; perform notification network/control work after releasing Engine.
    pub fn notification_service(&mut self)->Result<std::sync::Arc<crate::notifications::NotificationService>>{
        if self.repair_pending{return Err("正在修复任务数据，请等待看板重启".into());}
        if self.restart_required || crate::migration::resolve_root(&self.root)?!=self.root{return Err("数据目录已切换，请先重启看板".into());}
        self.start_notifications();self.notifications.clone().ok_or("通知服务不可用".into())
    }
    pub fn data_location(&self)->Value{crate::migration::location(&self.root,self.restart_required)}
    pub fn migrate_data(&mut self,path:&Path)->Result<Value>{
        if self.restart_required{return Err("数据目录已切换，请先重启看板".into());}
        let receipt=crate::integration::receipt_path(&self.root);
        if receipt.exists() {
            let receipt=read_json(&receipt)?;
            if receipt["reporter_version"]!=env!("CARGO_PKG_VERSION") {
                return Err("请先用新版重新连接 Codex，完整重启 Codex 后再迁移".into());
            }
        }
        if let Some(service)=self.notifications.take(){service.shutdown();}
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
        if self.repair_pending{return Err("正在修复任务数据，请等待看板重启".into());}
        let current=crate::migration::resolve_root(&self.root)?;
        if current!=self.root{if let Some(service)=self.notifications.take(){service.shutdown();}self.store=None;self.root=current;self.restart_required=true;self.connection=None;}
        if method=="data_location"{return Ok(self.data_location());}
        if method=="migrate_data"{if params["producers_stopped"]!=true{return Err("迁移前必须退出全部 Codex 会话及其他看板，确认已暂停上报".into());}return self.migrate_data(Path::new(params["path"].as_str().filter(|p|!p.is_empty()).ok_or("请选择新的数据目录")?));}
        if self.restart_required && !["snapshot","window_load"].contains(&method){return Err("数据目录已切换，请先重启看板".into());}
        if !["preferences","window_save"].contains(&method) && !method.starts_with("notification_"){ return self.call_inner(method,params);}
        let root=self.root.clone();
        crate::migration::with_root(&root,|resolved|{
            if resolved!=self.root{if let Some(service)=self.notifications.take(){service.shutdown();}self.store=None;self.root=resolved.to_path_buf();self.restart_required=true;self.connection=None;return Err("数据目录已切换，请重启看板".into());}
            self.call_inner(method,params)
        })
    }
    fn call_inner(&mut self,method:&str,params:Value)->Result<Value>{match method{
        "snapshot"=>{
            // Storage failure is not collector failure, nor an empty task list.
            // Keep connection diagnostics available even when SQLite cannot read.
            let loaded=(||{if self.store.is_none(){self.store=Some(Store::open(&self.root,true)?);}self.store.as_mut().unwrap().tasks()})();
            let (tasks,storage)=match loaded{Ok(tasks)=>(tasks,json!({"status":"ok","message":""})),Err(_)=>{self.store=None;(Vec::new(),json!({"status":"error","message":"任务数据库读取失败，数据不等于已清空。请保留数据库及 WAL 文件，勿删除、重建或反复重新连接；先备份并检查数据完整性。"}))}};
            if params["notify"]!=false {self.start_notifications();}
            let notices=if params["notify"]!=false && storage["status"]=="ok"{self.notices.update(&tasks)}else{Vec::new()};let options=load_options(&self.root);
            let heartbeat=read_json(&self.root.join("health.json")).ok();
            let age=heartbeat.as_ref().and_then(|v|v["at"].as_f64()).map(|at|now()-at);
            let healthy=heartbeat.as_ref().is_some_and(|v|v["status"]=="ok") && age.is_some_and(|age|(0.0..5.).contains(&age));
            let reason=if heartbeat.is_none(){"missing"}else if age.is_none_or(|age|age<0.){"clock_or_invalid"}else if age.is_some_and(|age|age>=5.){"stale"}else if heartbeat.as_ref().is_some_and(|v|v["status"]!="ok"){"storage_error"}else{"ok"};
            let collector=json!({"status":reason,"heartbeat_age":age,"pid":heartbeat.as_ref().map(|v|v["pid"].clone()),"version":heartbeat.as_ref().map(|v|v["version"].clone()),"start_error":self.collector_start_error,"data_root":self.root,"guidance":if healthy{"采集心跳正常；不代表任务库一定可读。"}else{"先查看数据目录与数据库读取状态；不要删除库或反复重连。旧采集器交接失败不会停止 Codex，请保留现场后检查。"}});
            if self.connection.is_none()||now()-self.checked_at>3.{let mut connection=crate::bridge::decorate_connection(&self.root,integration::connection_state(&self.root));connection["codex_home"]=json!(integration::codex_home());connection["isolated"]=json!(std::env::var("FLOATBOARD_DEVELOPMENT").as_deref()==Ok("1"));connection["codex_mode"]=json!(std::env::var("FLOATBOARD_CODEX_MODE").unwrap_or_else(|_|"isolated".into()));if !healthy && connection["state"]=="sync_verified"{connection["state"]=json!("error");connection["diagnostics"]=json!(["同步曾验证，但当前采集器离线；请检查数据目录权限，稍后再检查连接。"]);}self.connection=Some(connection);self.checked_at=now();}
            Ok(json!({"tasks":tasks,"notices":if options["notifications"]==true{notices}else{Vec::new()},"options":options,"healthy":healthy,"collector":collector,"storage":storage,"connection":self.connection,"data_location":self.data_location(),"restart_required":self.restart_required,"now":now()}))
        },
        "preferences"=>save_options(&self.root,&self.gui,&params),
        "preview"=>Ok(json!({"changes":integration::preview(&self.root,params["remove"]==true)?.changes})),
        "connect"=>{let result=integration::connect_with_recovery(&self.root,params["confirm_recovery"]==true)?;self.connection=None;Ok(result)},
        "connect_request"=>Ok(json!(format!("请帮我接入当前 Windows 用户的 Codex 任务看板，保留其他配置和正常 Hook 信任流程。请执行：{}",integration::command_line(&[crate::runtime::reporter_path()?.to_string_lossy().into_owned(),"--connect".into(),"--data-dir".into(),self.root.to_string_lossy().into_owned()])))),
        "disconnect"=>{let plan=integration::preview(&self.root,true)?;let changed=integration::apply(&self.root,&plan)?;self.connection=None;Ok(json!({"success":true,"changed":changed}))},
        "window_load"=>{let mut state=read_json(&self.root.join("window.json")).ok().filter(|v|v.is_object()).unwrap_or(json!({}));if state.get("tab").is_some_and(|v|!v.as_u64().is_some_and(|n|n<=2)){state["tab"]=json!(0);}Ok(state)},
        "window_save"=>{let mut old=self.call_inner("window_load",json!({}))?;for key in ["x","y","width","height","pinned","selected","tab","filter","details_collapsed"]{if let Some(value)=params.get(key){let valid=match key{"pinned"|"details_collapsed"=>value.is_boolean(),"selected"|"filter"=>value.as_str().is_some_and(|s|s.len()<=500),"tab"=>value.as_u64().is_some_and(|n|n<=2),"width"=>value.as_f64().is_some_and(|n|(392.0..=100000.).contains(&n)),"height"=>value.as_f64().is_some_and(|n|(570.0..=100000.).contains(&n)),_=>value.as_f64().is_some_and(|n|n.is_finite()&&n.abs()<=100000.)};if !valid{return Err("窗口设置无效".into());}old[key]=value.clone();}}atomic_json(&self.root.join("window.json"),&old)?;Ok(json!(true))},
        "notification_snapshot"|"notification_save"|"notification_channel_save"|"notification_channel_delete"|"notification_test"|"notification_retry"|"notification_clear"|"notification_weixin_action"=>{
            let service=self.notifications.get_or_insert_with(||std::sync::Arc::new(crate::notifications::NotificationService::new(self.root.clone())));
            service.call(method,params)
        },
        _=>Err("未知看板操作".into())
    }}
}
