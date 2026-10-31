use std::{fs,path::{Path,PathBuf}};
use serde_json::{json,Value};
use floatboard_core::{store::{self,Store},ingest,runtime,integration,desktop};
struct Temp(PathBuf);
impl Temp{fn new()->Self{let path=std::env::temp_dir().join(format!("floatboard-rust-{}",uuid::Uuid::new_v4().simple()));fs::create_dir_all(&path).unwrap();Self(path)}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
fn report()->Value{json!({"revision":0,"title":"专用验收任务","status":"running","current_step":"检查","plan":[{"id":"check","title":"检查","status":"inProgress"}],"blockers":[],"evidence":[]})}
fn event(kind:&str)->Value{json!({"event_id":uuid::Uuid::new_v4().simple().to_string(),"recorded_at":floatboard_core::now(),"kind":kind,"session_id":"test","host_id":"test-host","cwd":"C:/isolated"})}
fn progress_event(revision:i64)->Value{let mut value=event("progress");value["report"]=report();value["report"]["revision"]=json!(revision);value}
#[test]
fn legacy_sqlite_tasks_and_timing_match_exactly(){
    let temp=Temp::new();fs::write(temp.0.join("events.sqlite3"),include_bytes!("fixtures/legacy-events.sqlite3")).unwrap();
    let mut expected:Value=serde_json::from_str(include_str!("fixtures/legacy-tasks.json")).unwrap();
    let mut store=Store::open(&temp.0,true).unwrap();let mut actual=json!(store.tasks().unwrap());
    // JSON 的 0 与 0.0 均为数字；比较数值，其他字段逐项严格比对。
    fn numbers(value:&mut Value){match value{Value::Number(n)=>*value=json!(n.as_f64().unwrap()),Value::Array(items)=>items.iter_mut().for_each(numbers),Value::Object(items)=>items.values_mut().for_each(numbers),_=>{}}}
    numbers(&mut expected);numbers(&mut actual);assert_eq!(actual,expected);
}
#[test]
fn invalid_reports_are_rejected(){
    let cases=[json!({"revision":true}),json!({"revision":-1}),json!({"status":[]}),json!({"plan":[null]}),json!({"plan":[{"id":"x","title":"","status":"pending"}]}),json!({"blockers":[{"id":"x","title":"x","open":"true"}]}),json!({"evidence":[null]}),json!({"status":"blocked"}),json!({"status":"completed"}),json!({"plan":[{"id":"same","title":"x","status":"pending"},{"id":"same","title":"x","status":"pending"}]})];
    for case in cases{let mut value=report();for (k,v) in case.as_object().unwrap(){value[k]=v.clone();}assert!(store::validate_report(&value).is_err(),"accepted {case}");}
}
#[test]
fn completion_requires_evidence_and_no_open_blockers(){
    let mut value=report();value["status"]=json!("completed");value["plan"][0]["status"]=json!("completed");value["evidence"]=json!([{"label":"已核验"}]);assert!(store::validate_report(&value).is_ok());value["blockers"]=json!([{"id":"wait","title":"待确认","open":true}]);assert!(store::validate_report(&value).is_err());
}
#[test]
fn concurrent_registration_keeps_one_binding(){
    let temp=Temp::new();let handles:Vec<_>=(0..20).map(|_|{let path=temp.0.clone();std::thread::spawn(move||store::register(&path,"same","C:/isolated").unwrap())}).collect();let records:Vec<_>=handles.into_iter().map(|h|h.join().unwrap()).collect();assert!(records.iter().all(|v|v["session_token"]==records[0]["session_token"]));
}
#[test]
fn authentication_rejects_cross_session(){
    let temp=Temp::new();let a=store::register(&temp.0,"a","C:/a").unwrap();store::register(&temp.0,"b","C:/b").unwrap();assert!(store::authenticate(&temp.0,&json!({"session_id":"b","session_token":a["session_token"]})).is_err());assert!(store::authenticate(&temp.0,&a).is_ok());
}
#[test]
fn duplicate_and_older_revision_do_not_overwrite(){
    let temp=Temp::new();let mut store=Store::open(&temp.0,false).unwrap();let value=progress_event(5);assert!(store.ingest(&value).unwrap());assert!(!store.ingest(&value).unwrap());store.ingest(&progress_event(1)).unwrap();assert_eq!(store.tasks().unwrap()[0]["revision"],5);
}
#[test]
fn runtime_stop_does_not_complete_business_task(){
    let temp=Temp::new();let mut store=Store::open(&temp.0,false).unwrap();let mut value=progress_event(0);value["report"]["status"]=json!("blocked");value["report"]["blockers"]=json!([{"id":"dep","title":"等待依赖","open":true}]);store.ingest(&value).unwrap();store.ingest(&event("Stop")).unwrap();let task=store.tasks().unwrap().remove(0);assert_eq!(task["runtime"],"idle");assert_eq!(task["status"],"blocked");assert_eq!(task["blockers"][0]["open"],true);
}
#[test]
fn old_runtime_and_child_report_cannot_overwrite_parent(){
    let temp=Temp::new();let mut store=Store::open(&temp.0,false).unwrap();store.ingest(&progress_event(5)).unwrap();store.ingest(&event("Stop")).unwrap();let mut old=event("PreToolUse");old["recorded_at"]=json!(floatboard_core::now()-10.);store.ingest(&old).unwrap();let mut child=progress_event(99);child["agent_id"]=json!("child");store.ingest(&child).unwrap();let task=store.tasks().unwrap().remove(0);assert_eq!(task["runtime"],"idle");assert_eq!(task["revision"],5);assert_eq!(task["agents"]["child"]["runtime"],"active");
}
#[test]
fn sensitive_unlisted_fields_never_persist(){
    let temp=Temp::new();let mut store=Store::open(&temp.0,false).unwrap();let mut value=event("SessionStart");value["raw_prompt"]=json!("PRIVATE_CONTENT");value["session_token"]=json!("PRIVATE_TOKEN");value["tool_input"]=json!("PRIVATE_COMMAND");store.ingest(&value).unwrap();let saved:String=store.db.query_row("select payload from events",[],|r|r.get(0)).unwrap();assert!(!saved.contains("PRIVATE_"));assert_eq!(store::text(&json!("token=synthetic-secret-value"),500).unwrap(),"[敏感值已过滤]");
}
#[test]
fn queue_and_quarantine_survive_restart(){
    let temp=Temp::new();for _ in 0..5{store::enqueue(&temp.0,event("Stop")).unwrap();}fs::write(temp.0.join("inbox/broken.json"),"[1]").unwrap();let mut store=Store::open(&temp.0,false).unwrap();assert_eq!(runtime::drain(&temp.0,&mut store).unwrap(),(5,1));drop(store);let mut store=Store::open(&temp.0,false).unwrap();assert_eq!(store.tasks().unwrap().len(),1);assert_eq!(runtime::drain(&temp.0,&mut store).unwrap(),(0,0));assert!(temp.0.join("rejected/broken.json").exists());
}
#[test]
fn hook_resume_includes_pending_revision_without_token_in_tasks(){
    let temp=Temp::new();let payload=json!({"hook_event_name":"SessionStart","session_id":"real","cwd":"C:/test"});let context=ingest::hook(&temp.0,&payload).unwrap();assert!(context["hookSpecificOutput"]["additionalContext"].is_string());let binding=store::register(&temp.0,"real","C:/test").unwrap();let mut value=report();value["session_id"]=json!("real");value["session_token"]=binding["session_token"].clone();value["revision"]=json!(8);ingest::progress(&temp.0,&value).unwrap();let context=ingest::hook(&temp.0,&payload).unwrap();assert!(context["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("last_revision=8"));let mut store=Store::open(&temp.0,false).unwrap();runtime::drain(&temp.0,&mut store).unwrap();assert!(!serde_json::to_string(&store.tasks().unwrap()).unwrap().contains(binding["session_token"].as_str().unwrap()));
}
#[test]
fn native_plan_mapping_works(){
    let temp=Temp::new();ingest::hook(&temp.0,&json!({"hook_event_name":"PostToolUse","session_id":"plan","tool_name":"update_plan","tool_input":{"plan":[{"step":"检查","status":"in_progress"}]}})).unwrap();let mut store=Store::open(&temp.0,false).unwrap();runtime::drain(&temp.0,&mut store).unwrap();assert_eq!(store.tasks().unwrap()[0]["plan"][0]["status"],"inProgress");
}
#[test]
fn malformed_mcp_continues_and_rejects_unknown_tool(){
    let temp=Temp::new();let input=b"[]\nnull\n{\"id\":1,\"method\":\"initialize\",\"params\":null}\n{\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"other\"}}\n{\"id\":3,\"method\":\"ping\"}\n";let mut output=Vec::new();ingest::serve_mcp(&temp.0,std::io::Cursor::new(input),&mut output).unwrap();let values:Vec<Value>=String::from_utf8(output).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();assert_eq!(values.len(),3);assert_eq!(values[1]["error"]["code"],-32602);assert_eq!(values[2]["result"],json!({}));
}
#[test]
fn native_lock_is_exclusive_and_released(){let temp=Temp::new();let path=temp.0.join("lock");let first=runtime::ProcessLock::acquire(&path).unwrap();assert!(runtime::ProcessLock::acquire(&path).is_err());drop(first);assert!(runtime::ProcessLock::acquire(&path).is_ok());}
#[test]
fn window_state_preserves_legacy_fields_and_validates_types(){
    let temp=Temp::new();store::atomic_json(&temp.0.join("window.json"),&json!({"legacy":"keep","width":392})).unwrap();let mut engine=desktop::Engine::new(temp.0.clone(),temp.0.join("CodexFloatboard.exe"));engine.call("window_save",json!({"width":772,"height":570,"unknown":"drop"})).unwrap();let value=engine.call("window_load",json!({})).unwrap();assert_eq!(value["legacy"],"keep");assert_eq!(value["width"],772);assert!(value.get("unknown").is_none());assert!(engine.call("window_save",json!({"pinned":"false"})).is_err());assert!(engine.call("window_save",json!({"width":0,"height":0})).is_err());assert_eq!(engine.call("window_load",json!({})).unwrap()["width"],772);assert!(engine.call("shell",json!({})).is_err());
}
#[test]
fn preferences_validate_boundaries_without_touching_registry(){
    for value in [json!({"notifications":"yes"}),json!({"theme":"custom"}),json!({"notification_seconds":0}),json!({"notification_seconds":31}),json!({"notification_seconds":true})]{assert!(desktop::validate_options(&value).is_err());}let temp=Temp::new();let value=desktop::save_options(&temp.0,&temp.0.join("CodexFloatboard.exe"),&desktop::defaults()).unwrap();assert_eq!(value["auto_start"],false);assert_eq!(desktop::load_options(&temp.0)["theme"],"light");
}
fn setup_config(root:&Path,home:&Path){fs::create_dir_all(home).unwrap();fs::write(home.join("config.toml"),"# original comment\nmodel = 'preserve-model'\n[mcp_servers.other]\ncommand = 'keep.exe'\n[hooks]\nStop = [{hooks = [{type = 'command', command = 'other-hook', timeout = 3}]}]\n").unwrap();let _=root;}
#[test]
fn config_connect_disconnect_preserve_other_entries_and_comments(){
    let temp=Temp::new();let root=temp.0.join("data");let home=temp.0.join("codex");setup_config(&root,&home);let exe=temp.0.join("FloatboardReporter.exe");let plan=integration::preview_at(&root,&home,&exe,false).unwrap();assert_eq!(plan.changes.len(),10);integration::apply(&root,&plan).unwrap();let applied=fs::read_to_string(home.join("config.toml")).unwrap();assert!(applied.contains("# original comment")&&applied.contains("preserve-model")&&applied.contains("other-hook"));let again=integration::preview_at(&root,&home,&exe,false).unwrap();assert!(!again.changed);assert_eq!(again.present,10);let remove=integration::preview_at(&root,&home,&exe,true).unwrap();integration::apply(&root,&remove).unwrap();let removed=fs::read_to_string(home.join("config.toml")).unwrap();assert!(!removed.contains("codex_floatboard"));assert!(removed.contains("other-hook")&&removed.contains("keep.exe"));
}
#[test]
fn config_migrates_owned_python_paths_and_rejects_concurrent_edit(){
    let temp=Temp::new();let root=temp.0.join("data");let home=temp.0.join("codex");setup_config(&root,&home);let legacy=temp.0.join("legacy/FloatboardReporter.exe");integration::apply(&root,&integration::preview_at(&root,&home,&legacy,false).unwrap()).unwrap();let native=temp.0.join("native/FloatboardReporter.exe");let upgrade=integration::preview_at(&root,&home,&native,false).unwrap();assert_eq!(upgrade.changes.len(),10);integration::apply(&root,&upgrade).unwrap();let source=fs::read_to_string(home.join("config.toml")).unwrap();assert!(!source.contains("legacy"));assert!(source.contains("native"));let remove=integration::preview_at(&root,&home,&native,true).unwrap();fs::write(home.join("config.toml"),format!("{source}\n# concurrent\n")).unwrap();assert!(integration::apply(&root,&remove).is_err());assert!(fs::read_to_string(home.join("config.toml")).unwrap().contains("# concurrent"));
}
#[test]
fn reconnect_refreshes_version_receipt_without_rewriting_config(){
    let temp=Temp::new();let root=temp.0.join("data");let home=temp.0.join("codex");setup_config(&root,&home);
    let exe=temp.0.join("FloatboardReporter.exe");integration::apply(&root,&integration::preview_at(&root,&home,&exe,false).unwrap()).unwrap();
    let path=root.join("integration.json");let mut receipt=store::read_json(&path).unwrap();receipt["reporter_version"]=json!("0.7.0");store::atomic_json(&path,&receipt).unwrap();
    let before=fs::read(home.join("config.toml")).unwrap();let plan=integration::preview_at(&root,&home,&exe,false).unwrap();assert!(!plan.changed);assert!(!integration::apply(&root,&plan).unwrap());
    assert_eq!(store::read_json(&path).unwrap()["reporter_version"],env!("CARGO_PKG_VERSION"));assert_eq!(before,fs::read(home.join("config.toml")).unwrap());
}
#[test]
fn advanced_diff_excludes_other_configuration(){
    let temp=Temp::new();let home=temp.0.join("codex");setup_config(&temp.0,&home);let plan=integration::preview_at(&temp.0,&home,&temp.0.join("FloatboardReporter.exe"),false).unwrap();let display=serde_json::to_string(&plan.changes).unwrap();assert!(!display.contains("preserve-model")&&!display.contains("other-hook")&&!display.contains("keep.exe"));assert!(display.contains("--mcp"));
}
#[test]
fn notification_first_load_silent_and_changes_only(){
    let mut detector=desktop::ChangeDetector::default();let mut task=json!({"id":"t","title":"验收","status":"running","plan":[],"blockers":[],"history":[]});assert!(detector.update(&[task.clone()]).is_empty());task["history"]=json!([{"kind":"PreToolUse"}]);assert!(detector.update(&[task.clone()]).is_empty());task["blockers"]=json!([{"id":"dep","title":"依赖","open":true}]);let notices=detector.update(&[task.clone()]);assert_eq!(notices.len(),1);assert_eq!(notices[0]["tab"],1);task["blockers"]=json!([]);assert!(detector.update(&[task])[0]["lines"][0].as_str().unwrap().contains("已解除"));
}
