use std::{fs,path::PathBuf};
use serde_json::json;
use floatboard_core::{ingest,integration,store};
struct Temp(PathBuf);
impl Temp {fn new()->Self {let path=std::env::temp_dir().join(format!("floatboard-diagnostics-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&path).unwrap();Self(path)}}
impl Drop for Temp {fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
#[test]
fn hook_health_records_only_metadata_not_original_payload(){
 let temp=Temp::new();let payload=json!({"hook_event_name":"SessionStart","session_id":"synthetic","cwd":"C:/isolated","prompt":"private-prompt-never-record","tool_input":{"command":"private-command-never-record"}});
 let result=ingest::hook(&temp.0,&payload).unwrap();assert!(result["hookSpecificOutput"]["additionalContext"].is_string());
 let health=store::read_json(&temp.0.join("hook-status.json")).unwrap();assert_eq!(health["status"],"ok");assert_eq!(health["event"],"SessionStart");let raw=health.to_string();assert!(!raw.contains("session_token")&&!raw.contains("private-")&&!raw.contains("synthetic"));
}
#[test]
fn no_hook_observation_is_not_invented_by_connection_check(){
 let temp=Temp::new();let result=integration::connection_state(&temp.0);assert_eq!(result["hook_seen"],false);assert!(!temp.0.join("hook-status.json").exists());assert!(!temp.0.join("events.sqlite3").exists());
 store::atomic_json(&temp.0.join("hook-status.json"),&json!({"status":"ok","at":floatboard_core::now(),"event":"SessionStart"})).unwrap();assert_eq!(integration::connection_state(&temp.0)["hook_seen"],true);
}
