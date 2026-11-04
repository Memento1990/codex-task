use std::{fs,path::PathBuf};
use serde_json::json;
use floatboard_core::{desktop::Engine,store::{Store,atomic_json},now};
struct Temp(PathBuf);
impl Temp{fn new()->Self{let p=std::env::temp_dir().join(format!("floatboard-storage-diagnostics-{}",uuid::Uuid::new_v4().simple()));fs::create_dir_all(&p).unwrap();Self(p)}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
#[test]
fn corrupted_storage_does_not_masquerade_as_offline_or_recreate_database(){
 let t=Temp::new();let path=t.0.join("events.sqlite3");fs::write(&path,b"not a sqlite database - preserve forensic bytes").unwrap();
 atomic_json(&t.0.join("health.json"),&json!({"status":"ok","at":now(),"pid":123})).unwrap();
 let original=fs::read(&path).unwrap();let mut e=Engine::new(t.0.clone(),t.0.join("fake.exe"));
 let s=e.call("snapshot",json!({"notify":false})).unwrap();assert_eq!(s["healthy"],true);assert_eq!(s["storage"]["status"],"error");assert_eq!(s["collector"]["status"],"ok");assert!(s["notices"].as_array().unwrap().is_empty());assert_eq!(fs::read(path).unwrap(),original);
}
#[test]
fn heartbeat_status_and_start_failure_are_independent_of_storage(){
 let t=Temp::new();drop(Store::open(&t.0,false).unwrap());let mut e=Engine::new(t.0.clone(),t.0.join("fake.exe"));e.set_collector_start_result(Err("旧采集器未确认安全交接，请稍后重试；会话未停止".into()));
 let s=e.call("snapshot",json!({"notify":false})).unwrap();assert_eq!(s["storage"]["status"],"ok");assert_eq!(s["collector"]["status"],"missing");assert!(s["collector"]["start_error"].as_str().unwrap().contains("安全交接"));
 for (status,at,expected) in [("ok",now()-10.,"stale"),("storage_error",now(),"storage_error"),("ok",now()+100.,"clock_or_invalid")] {
  atomic_json(&t.0.join("health.json"),&json!({"status":status,"at":at})).unwrap();let s=e.call("snapshot",json!({"notify":false})).unwrap();assert_eq!(s["healthy"],false);assert_eq!(s["collector"]["status"],expected);assert_eq!(s["storage"]["status"],"ok");
 }
}
