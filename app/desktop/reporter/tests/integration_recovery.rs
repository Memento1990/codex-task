use std::{fs,path::{Path,PathBuf}};
use floatboard_core::{integration as api,store::{self,hash}};
use serde_json::{json,Value};
struct Temp(PathBuf);
impl Temp{fn new()->Self{let root=std::env::temp_dir().join(format!("floatboard-integration-recovery-{}",uuid::Uuid::new_v4().simple()));fs::create_dir_all(root.join("home")).unwrap();Self(root)}fn data(&self)->PathBuf{self.0.join("data")}fn home(&self)->PathBuf{self.0.join("home")}fn exe(&self,name:&str)->PathBuf{self.0.join(name).join("FloatboardReporter.exe")}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
fn config(home:&Path)->Vec<u8>{fs::read(home.join("config.toml")).unwrap()}
fn journal(temp:&Temp,before:&[u8],after:&[u8],old:Option<Value>,new:&Value){store::atomic_json(&temp.data().join("integration-journal.json"),&json!({"version":1,"home":temp.home(),"config_path":temp.home().join("config.toml"),"before_hash":hash(before),"after_hash":hash(after),"old_receipt":old,"new_receipt":new})).unwrap();}
fn installed(temp:&Temp){fs::write(temp.home().join("config.toml"),"# keep\nmodel='preserved'\n").unwrap();let plan=api::preview_at(&temp.data(),&temp.home(),&temp.exe("old"),false).unwrap();api::apply(&temp.data(),&plan).unwrap();}
#[test]fn missing_receipt_requires_explicit_confirmation_then_upgrades(){
 let t=Temp::new();installed(&t);let before=config(&t.home());fs::remove_file(api::receipt_path(&t.data())).unwrap();
 let p=api::preview_at(&t.data(),&t.home(),&t.exe("new"),false).unwrap();assert!(p.recovery_required);assert_eq!(p.changes.len(),10);assert!(api::apply(&t.data(),&p).unwrap_err().contains("确认"));assert_eq!(before,config(&t.home()));
 let p=api::preview_at_with_recovery(&t.data(),&t.home(),&t.exe("new"),false,true).unwrap();api::apply(&t.data(),&p).unwrap();assert!(!api::preview_at(&t.data(),&t.home(),&t.exe("new"),false).unwrap().recovery_required);assert!(String::from_utf8(config(&t.home())).unwrap().contains("# keep"));
}
#[test]fn no_receipt_exact_current_config_still_requires_consent(){
 let t=Temp::new();installed(&t);fs::remove_file(api::receipt_path(&t.data())).unwrap();let p=api::preview_at(&t.data(),&t.home(),&t.exe("old"),false).unwrap();assert!(p.recovery_required);assert!(!p.changed);assert!(api::apply(&t.data(),&p).is_err());
}
#[test]fn altered_or_partial_old_configuration_is_never_adopted_or_duplicated(){
 for mutation in ["timeout", "extra-env", "missing-hook", "duplicate"] {
  let t=Temp::new();installed(&t);let source=String::from_utf8(config(&t.home())).unwrap();let mut doc=source.parse::<toml_edit::DocumentMut>().unwrap();
  match mutation{
   "timeout"=>{let text=source.replace("timeout = 5","timeout = 7").replace("timeout=5","timeout=7");assert_ne!(text,source);fs::write(t.home().join("config.toml"),text).unwrap();},
   "extra-env"=>{doc["mcp_servers"]["codex_floatboard"]["env"]["TOKEN"]=toml_edit::value("private");fs::write(t.home().join("config.toml"),doc.to_string()).unwrap();},
   "missing-hook"=>{doc["hooks"].as_table_mut().unwrap().remove("Stop");fs::write(t.home().join("config.toml"),doc.to_string()).unwrap();},
   _=>{let arr=doc["hooks"]["Stop"].as_array_mut().unwrap();arr.push(arr.get(0).unwrap().clone());fs::write(t.home().join("config.toml"),doc.to_string()).unwrap();}
  }
  fs::remove_file(api::receipt_path(&t.data())).unwrap();let before=config(&t.home());assert!(api::preview_at_with_recovery(&t.data(),&t.home(),&t.exe("new"),false,true).is_err(),"{mutation}");assert_eq!(config(&t.home()),before);
 }
}
#[test]fn journal_before_restores_previous_receipt_and_keeps_config(){
 let t=Temp::new();installed(&t);let before=config(&t.home());let old=store::read_json(&api::receipt_path(&t.data())).unwrap();let new=json!({"home":t.home(),"status":"applied","contribution":{}});journal(&t,&before,b"after",Some(old.clone()),&new);
 // Simulate an old implementation publishing receipt before config.
 store::atomic_json(&api::receipt_path(&t.data()),&new).unwrap();assert!(api::recover_at(&t.data(),&t.home()).unwrap());assert_eq!(store::read_json(&api::receipt_path(&t.data())).unwrap(),old);assert_eq!(config(&t.home()),before);assert!(!t.data().join("integration-journal.json").exists());
}
#[test]fn journal_after_completes_receipt_commit(){
 let t=Temp::new();installed(&t);let old=store::read_json(&api::receipt_path(&t.data())).unwrap();let new=json!({"home":t.home(),"status":"applied","contribution":{}});let before=config(&t.home());let after=b"# replacement\n";journal(&t,&before,after,Some(old),&new);fs::write(t.home().join("config.toml"),after).unwrap();api::recover_at(&t.data(),&t.home()).unwrap();assert_eq!(store::read_json(&api::receipt_path(&t.data())).unwrap(),new);assert_eq!(config(&t.home()),after);
}
#[test]fn journal_external_config_or_receipt_change_stops_recovery(){
 let t=Temp::new();installed(&t);let before=config(&t.home());let old=store::read_json(&api::receipt_path(&t.data())).unwrap();let new=json!({"home":t.home(),"status":"applied"});journal(&t,&before,b"after",Some(old.clone()),&new);fs::write(t.home().join("config.toml"),"# user change").unwrap();assert!(api::recover_at(&t.data(),&t.home()).is_err());assert_eq!(config(&t.home()),b"# user change");assert_eq!(store::read_json(&api::receipt_path(&t.data())).unwrap(),old);
 fs::write(t.home().join("config.toml"),&before).unwrap();store::atomic_json(&api::receipt_path(&t.data()),&json!({"external":true})).unwrap();assert!(api::recover_at(&t.data(),&t.home()).is_err());assert!(t.data().join("integration-journal.json").exists());
}
#[test]fn stale_plan_checks_receipt_and_config_without_overwriting(){
 let t=Temp::new();installed(&t);let p=api::preview_at(&t.data(),&t.home(),&t.exe("new"),false).unwrap();let before=config(&t.home());let mut receipt=store::read_json(&api::receipt_path(&t.data())).unwrap();receipt["reporter_version"]=json!("external");store::atomic_json(&api::receipt_path(&t.data()),&receipt).unwrap();assert!(api::apply(&t.data(),&p).is_err());assert_eq!(before,config(&t.home()));
}
#[test]fn reconnect_preserves_generation_and_connected_at_and_other_config(){
 let t=Temp::new();installed(&t);let before=config(&t.home());let old=store::read_json(&api::receipt_path(&t.data())).unwrap();let p=api::preview_at(&t.data(),&t.home(),&t.exe("old"),false).unwrap();assert!(!api::apply(&t.data(),&p).unwrap());let new=store::read_json(&api::receipt_path(&t.data())).unwrap();assert_eq!(old["connected_at"],new["connected_at"]);assert_eq!(old["after_hash"],new["after_hash"]);assert_eq!(before,config(&t.home()));
}
#[test]fn legacy_prepared_before_and_after_require_confirmation_and_preserve_config(){
 for after_image in [false,true] {
  let t=Temp::new();installed(&t);let old=config(&t.home());let mut receipt=store::read_json(&api::receipt_path(&t.data())).unwrap();
  let upgrade=api::preview_at(&t.data(),&t.home(),&t.exe("new"),false).unwrap();
  receipt["status"]=json!("prepared");receipt["before_hash"]=json!(hash(&old));receipt["after_hash"]=json!(hash(&upgrade.updated));receipt["contribution"]=upgrade.contribution.clone();store::atomic_json(&api::receipt_path(&t.data()),&receipt).unwrap();
  if after_image{fs::write(t.home().join("config.toml"),&upgrade.updated).unwrap();}
  let current=config(&t.home());let p=api::preview_at(&t.data(),&t.home(),&t.exe("new"),false).unwrap();assert!(p.recovery_required);assert!(api::apply(&t.data(),&p).is_err());assert_eq!(current,config(&t.home()));
  let p=api::preview_at_with_recovery(&t.data(),&t.home(),&t.exe("new"),false,true).unwrap();api::apply(&t.data(),&p).unwrap();assert_eq!(store::read_json(&api::receipt_path(&t.data())).unwrap()["status"],"applied");
 }
}
#[test]fn mcp_tool_hooks_use_event_templates_and_session_end_keeps_command(){
 let t=Temp::new();let bridge=t.0.join("FloatboardBridge.exe");let c=api::contribution(&t.data(),&bridge);
 assert_eq!(c["mcp"]["tools"]["hook_context"]["approval_mode"],"approve");
 for event in api::HOOKS {let hook=api::desired_hook(&c,event);let item=&hook["hooks"][0];if event=="SessionEnd"{assert_eq!(item["type"],"command");assert_eq!(item["timeout"],3);}else{assert_eq!(item["timeout"],5);assert_eq!(item["type"],"mcp_tool");assert_eq!(item["input"]["hook_event_name"],event);assert_eq!(item["input"]["session_id"],"${session_id}");}}
 let p=api::preview_at(&t.data(),&t.home(),&bridge,false).unwrap();api::apply(&t.data(),&p).unwrap();let p=api::preview_at(&t.data(),&t.home(),&bridge,false).unwrap();assert!(!p.changed);assert_eq!(p.present,10);
 let p=api::preview_at(&t.data(),&t.home(),&bridge,true).unwrap();api::apply(&t.data(),&p).unwrap();assert!(!String::from_utf8(config(&t.home())).unwrap().contains("codex_floatboard"));
}
#[test]fn command_receipt_upgrades_to_mixed_hooks_without_duplicates(){
 let t=Temp::new();installed(&t);let p=api::preview_at(&t.data(),&t.home(),&t.0.join("FloatboardBridge.exe"),false).unwrap();assert_eq!(p.changes.len(),10);api::apply(&t.data(),&p).unwrap();let source=String::from_utf8(config(&t.home())).unwrap();let parsed:Value=toml_edit::de::from_str(&source).unwrap();for event in api::HOOKS{assert_eq!(parsed["hooks"][event].as_array().unwrap().len(),1);}
}
#[test]fn runtime_activation_journal_recovers_before_after_and_rejects_third_state(){
 for state in ["before","after","same","external"]{
 let t=Temp::new();installed(&t);let before=config(&t.home());let old=store::read_json(&api::receipt_path(&t.data())).unwrap();let new=json!({"home":t.home(),"status":"applied"});let active=t.data().join("runtime/active.json");let old_runtime=json!({"sha256":"old"});let new_runtime=json!({"sha256":"new"});store::atomic_json(&active,&old_runtime).unwrap();
 let after=if state=="same"{before.clone()}else{b"# after\n".to_vec()};journal(&t,&before,&after,Some(old.clone()),&new);let jp=t.data().join("integration-journal.json");let mut j=store::read_json(&jp).unwrap();j["runtime_path"]=json!(active);j["runtime_before"]=old_runtime.clone();j["runtime_after"]=new_runtime.clone();store::atomic_json(&jp,&j).unwrap();
 if state=="after"{fs::write(t.home().join("config.toml"),after).unwrap();}if state=="external"{store::atomic_json(&active,&json!({"sha256":"external"})).unwrap();assert!(api::recover_at(&t.data(),&t.home()).is_err());continue;}
 api::recover_at(&t.data(),&t.home()).unwrap();assert_eq!(store::read_json(&active).unwrap(),if state=="before"{old_runtime}else{new_runtime});assert_eq!(store::read_json(&api::receipt_path(&t.data())).unwrap(),if state=="before"{old}else{new});
 }
}
#[test]fn missing_receipt_old_commands_recover_directly_to_mixed_bridge(){
 let t=Temp::new();installed(&t);fs::remove_file(api::receipt_path(&t.data())).unwrap();let bridge=t.0.join("FloatboardBridge.exe");let p=api::preview_at_with_recovery(&t.data(),&t.home(),&bridge,false,true).unwrap();assert!(p.recovery_required);api::apply(&t.data(),&p).unwrap();let p=api::preview_at(&t.data(),&t.home(),&bridge,false).unwrap();assert!(!p.changed);assert_eq!(p.present,10);
}
#[test]fn session_end_5s_receipt_upgrades_only_owned_hook_and_can_disconnect(){
 for receipt_present in [true,false]{
  let t=Temp::new();let reporter=t.exe("old");installed(&t);
  let mut receipt=store::read_json(&api::receipt_path(&t.data())).unwrap();
  let mut old=receipt["contribution"].clone();old["hooks_by_event"].as_object_mut().unwrap().remove("SessionEnd");
  // Exact pre-fix command-route contribution and configuration, not a loose
  // timeout substitution: unknown fields and third-party hooks stay protected.
  let mut parsed:Value=toml_edit::de::from_str(&String::from_utf8(config(&t.home())).unwrap()).unwrap();parsed["hooks"]["SessionEnd"]=json!([old["hook"].clone(),{"hooks":[{"type":"command","command":"third-party","timeout":2}]}]);
  fs::write(t.home().join("config.toml"),toml_edit::ser::to_string_pretty(&parsed).unwrap()).unwrap();
  receipt["contribution"]=old;receipt["after_hash"]=json!(hash(&config(&t.home())));store::atomic_json(&api::receipt_path(&t.data()),&receipt).unwrap();
  if !receipt_present{fs::remove_file(api::receipt_path(&t.data())).unwrap();}
  let p=api::preview_at_with_recovery(&t.data(),&t.home(),&reporter,false,true).unwrap();assert_eq!(p.recovery_required,!receipt_present);assert_eq!(p.changes.len(),1);assert_eq!(p.changes[0]["key"],"hooks.SessionEnd");
  api::apply(&t.data(),&p).unwrap();let parsed:Value=toml_edit::de::from_str(&String::from_utf8(config(&t.home())).unwrap()).unwrap();assert_eq!(parsed["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"],2);assert_eq!(parsed["hooks"]["SessionEnd"][1]["hooks"][0]["timeout"],3);
  assert!(!api::preview_at(&t.data(),&t.home(),&reporter,false).unwrap().changed);
  let p=api::preview_at(&t.data(),&t.home(),&reporter,true).unwrap();api::apply(&t.data(),&p).unwrap();let parsed:Value=toml_edit::de::from_str(&String::from_utf8(config(&t.home())).unwrap()).unwrap();assert_eq!(parsed["hooks"]["SessionEnd"].as_array().unwrap().len(),1);assert_eq!(parsed["hooks"]["SessionEnd"][0]["hooks"][0]["command"],"third-party");
 }
}
#[test]fn mixed_bridge_5s_receipt_updates_only_session_end(){
 let t=Temp::new();let bridge=t.0.join("FloatboardBridge.exe");let mut old=api::contribution(&t.data(),&bridge);old["hooks_by_event"].as_object_mut().unwrap().remove("SessionEnd");
 let mut hooks=serde_json::Map::new();for event in api::HOOKS{hooks.insert(event.into(),json!([api::desired_hook(&old,event)]));}
 let parsed=json!({"model":"keep-me","mcp_servers":{"codex_floatboard":old["mcp"]},"hooks":hooks});fs::write(t.home().join("config.toml"),toml_edit::ser::to_string_pretty(&parsed).unwrap()).unwrap();
 store::atomic_json(&api::receipt_path(&t.data()),&json!({"home":t.home(),"status":"applied","contribution":old})).unwrap();
 let p=api::preview_at(&t.data(),&t.home(),&bridge,false).unwrap();assert_eq!(p.changes.len(),1);assert_eq!(p.changes[0]["key"],"hooks.SessionEnd");api::apply(&t.data(),&p).unwrap();
 let after:Value=toml_edit::de::from_str(&String::from_utf8(config(&t.home())).unwrap()).unwrap();assert_eq!(after["model"],"keep-me");for event in api::HOOKS{assert_eq!(after["hooks"][event][0]["hooks"][0]["timeout"],if event=="SessionEnd"{3}else{5});}assert!(!api::preview_at(&t.data(),&t.home(),&bridge,false).unwrap().changed);
}
#[test]fn stable_bridge_contribution_does_not_embed_data_location(){let t=Temp::new();let c=api::contribution(&t.data(),&t.0.join("FloatboardBridge.exe"));assert_eq!(c["mcp"]["args"],json!(["--mcp"]));assert!(!c["hook"].to_string().contains("--data-dir"));}

fn parsed_config(home:&Path)->Value{toml_edit::de::from_str(&String::from_utf8(config(home)).unwrap()).unwrap()}
#[test]fn namespace_is_an_exact_case_insensitive_root_component(){
 assert_eq!(api::server_name(Path::new("C:/fixtures/cOdExFlOaTbOaRd-DeV/shared-local/data")),"codex_floatboard_dev");
 for root in ["C:/fixtures/CodexFloatboard/data","C:/fixtures/CodexFloatboard-Dev-old/data","C:/fixtures/prefix-CodexFloatboard-Dev/data"]{assert_eq!(api::server_name(Path::new(root)),"codex_floatboard");}
}
#[test]fn dev_and_formal_share_home_without_sharing_ownership(){
 for bridge in [false,true]{
  let t=Temp::new();fs::write(t.home().join("config.toml"),"# keep\nmodel='preserved'\n").unwrap();
  let dev=t.0.join("CodexFloatboard-Dev/shared-local/data");
  let formal_exe=if bridge{t.0.join("formal/runtime/FloatboardBridge.exe")}else{t.exe("formal")};
  let dev_exe=if bridge{dev.join("runtime/FloatboardBridge.exe")}else{dev.join("FloatboardReporter.exe")};
  let p=api::preview_at(&t.data(),&t.home(),&formal_exe,false).unwrap();api::apply(&t.data(),&p).unwrap();
  let formal_bytes=config(&t.home());let formal=parsed_config(&t.home());let receipt=fs::read(api::receipt_path(&t.data())).unwrap();
  let p=api::preview_at(&dev,&t.home(),&dev_exe,false).unwrap();assert!(!p.recovery_required);assert_eq!(p.present,0);assert_eq!(p.changes[0]["key"],"mcp_servers.codex_floatboard_dev");api::apply(&dev,&p).unwrap();
  let both=parsed_config(&t.home());assert_eq!(both["mcp_servers"]["codex_floatboard"],formal["mcp_servers"]["codex_floatboard"]);
  for event in api::HOOKS{assert_eq!(both["hooks"][event][0],formal["hooks"][event][0]);assert_eq!(both["hooks"][event].as_array().unwrap().len(),2);if bridge&&event!="SessionEnd"{assert_eq!(both["hooks"][event][1]["hooks"][0]["server"],"codex_floatboard_dev");}}
  assert_eq!(fs::read(api::receipt_path(&t.data())).unwrap(),receipt);
  let bytes=config(&t.home());let p=api::preview_at(&dev,&t.home(),&dev_exe,false).unwrap();assert!(!p.changed);assert_eq!(p.present,10);assert!(!api::apply(&dev,&p).unwrap());assert_eq!(bytes,config(&t.home()));
  let p=api::preview_at(&t.data(),&t.home(),&formal_exe,false).unwrap();assert!(!p.changed);assert_eq!(p.present,10);
  // Missing own receipt can be recovered without adopting the foreign entries.
  if !bridge{fs::remove_file(api::receipt_path(&dev)).unwrap();let p=api::preview_at_with_recovery(&dev,&t.home(),&dev_exe,false,true).unwrap();assert!(p.recovery_required);api::apply(&dev,&p).unwrap();}
  let p=api::preview_at(&dev,&t.home(),&dev_exe,true).unwrap();api::apply(&dev,&p).unwrap();assert_eq!(config(&t.home()),formal_bytes);assert_eq!(fs::read(api::receipt_path(&t.data())).unwrap(),receipt);
 }
}
#[test]fn corrupt_dev_entries_are_not_exempted_as_foreign(){
 for mutation in ["own-server","own-command","own-timeout","foreign-key-spoof","foreign-extra-field"]{
  let t=Temp::new();installed(&t);let dev=t.0.join("CodexFloatboard-Dev/shared-local/data");let bridge=dev.join("runtime/FloatboardBridge.exe");let p=api::preview_at(&dev,&t.home(),&bridge,false).unwrap();api::apply(&dev,&p).unwrap();
  let mut parsed=parsed_config(&t.home());
  match mutation{
   "own-server"=>parsed["hooks"]["Stop"][1]["hooks"][0]["server"]=json!("codex_floatboard_broken"),
   "own-command"=>parsed["hooks"]["SessionEnd"][1]["hooks"][0]["command"]=json!(format!("{} --modified",api::desired_hook(&api::contribution(&dev,&bridge),"SessionEnd")["hooks"][0]["command"].as_str().unwrap())),
   "own-timeout"=>parsed["hooks"]["Stop"][1]["hooks"][0]["timeout"]=json!(7),
   "foreign-key-spoof"=>{parsed["mcp_servers"]["codex_floatboard"]=parsed["mcp_servers"]["codex_floatboard_dev"].clone();},
   _=>parsed["hooks"]["Stop"][0]["unexpected"]=json!(true),
  }
  fs::write(t.home().join("config.toml"),toml_edit::ser::to_string_pretty(&parsed).unwrap()).unwrap();let before=config(&t.home());
  assert!(api::preview_at(&dev,&t.home(),&bridge,false).is_err(),"{mutation}");assert!(api::preview_at(&dev,&t.home(),&bridge,true).is_err(),"{mutation}");assert_eq!(config(&t.home()),before);
 }
}
