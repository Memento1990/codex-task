use std::{fs,path::{Path,PathBuf},time::{Duration,Instant}};
use floatboard_core::{runtime::{self,ProcessLock},migration,store::{self,Store}};
use serde_json::json;
struct Fixture(PathBuf);
impl Fixture{
 fn new()->Self{let root=std::env::temp_dir().join(format!("floatboard-repair-runtime-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();Self(root)}
 fn root(&self,name:&str)->PathBuf{let p=self.0.join(name);fs::create_dir_all(&p).unwrap();p}
}
impl Drop for Fixture{fn drop(&mut self){
 let roots:Vec<_>=fs::read_dir(&self.0).unwrap().flatten().filter(|e|e.path().is_dir()).map(|e|e.path()).collect();
 for root in &roots{let _=store::atomic_json(&root.join("worker-shutdown.json"),&json!({}));}
 let deadline=Instant::now()+Duration::from_secs(5);
 while roots.iter().any(|p|ProcessLock::acquire(&p.join("collector.lock")).is_err())&&Instant::now()<deadline{std::thread::sleep(Duration::from_millis(10));}
 let _=fs::remove_dir_all(&self.0);
}}
fn wait(root:&Path,status:&str)->serde_json::Value{let deadline=Instant::now()+Duration::from_secs(5);loop{
 if let Ok(v)=store::read_json(&root.join("health.json")){if v["status"]==status{return v;}}
 assert!(Instant::now()<deadline,"worker not {status}");std::thread::sleep(Duration::from_millis(10));
}}
#[test]
fn corrupt_database_worker_stays_alive_and_acknowledges_pause(){
 let fixture=Fixture::new();let root=fixture.root("corrupt");fs::write(root.join("events.sqlite3"),b"not a sqlite database").unwrap();
 let before=fs::read(root.join("events.sqlite3")).unwrap();
 runtime::with_repair_pause(&root,|pause|{
  assert_eq!(pause.root(),root);pause.verify()?;
  assert!(ProcessLock::acquire(&root.join("collector.lock")).is_err());
  assert!(root.join(migration::STOP).exists());Ok(())
 }).unwrap();
 wait(&root,"storage_error");assert_eq!(fs::read(root.join("events.sqlite3")).unwrap(),before);
 assert!(!root.join(migration::STOP).exists());
}
#[test]
fn redirect_preserves_worker_identity_and_drains_late_legacy_inbox(){
 let fixture=Fixture::new();let old=fixture.root("old");let new=fixture.root("new");
 fs::write(old.join("events.sqlite3"),b"broken").unwrap();drop(Store::open(&new,false).unwrap());
 let mut pid=json!(null);
 runtime::with_repair_pause(&old,|pause|{
  pause.verify()?;pid=store::read_json(&old.join(migration::ACK))?["pid"].clone();
  store::atomic_json(&new.join("migration-origins.json"),&json!({"roots":[old]}))?;
  store::atomic_json(&old.join(migration::REDIRECT),&json!({"version":1,"path":new}))?;Ok(())
 }).unwrap();
 assert_eq!(wait(&new,"ok")["pid"],pid);
 assert!(ProcessLock::acquire(&old.join("collector.lock")).is_err());
 store::enqueue(&old,json!({"kind":"SessionStart","session_id":"late","host_id":"test"})).unwrap();
 let deadline=Instant::now()+Duration::from_secs(5);loop{
  let db=Store::open(&new,true).unwrap();let count:i64=db.db.query_row("select count(*) from events",[],|r|r.get(0)).unwrap();
  if count==1{break;}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));
 }
 runtime::with_repair_pause(&new,|pause|{pause.verify()?;assert!(ProcessLock::acquire(&old.join("collector.lock")).is_err());Ok(())}).unwrap();
}
#[test]
fn missing_capability_never_stops_legacy_collector(){
 let fixture=Fixture::new();let root=fixture.root("legacy");let _owner=ProcessLock::acquire(&root.join("collector.lock")).unwrap();
 store::atomic_json(&root.join("health.json"),&json!({"pid":std::process::id(),"at":floatboard_core::now(),"version":"0.13.0","migration_protocol":1})).unwrap();
 assert!(runtime::with_repair_pause(&root,|_|Ok(())).is_err());
 assert!(!root.join("worker-shutdown.json").exists());assert!(!root.join(migration::STOP).exists());
}
#[test]
fn omitted_inherited_origin_is_rejected_before_callback(){
 let fixture=Fixture::new();let root=fixture.root("current");let origin=fixture.root("origin");
 runtime::with_repair_pause(&root,|pause|{
  store::atomic_json(&origin.join(migration::REDIRECT),&json!({"version":1,"path":root}))?;
  store::atomic_json(&root.join("migration-origins.json"),&json!({"roots":[origin]}))?;
  assert!(pause.verify().is_err());
  fs::remove_file(root.join("migration-origins.json")).unwrap();Ok(())
 }).unwrap();
}
#[test]
fn stale_or_mismatched_ack_is_not_a_pause_proof_and_failure_releases_stop(){
 let fixture=Fixture::new();let root=fixture.root("ack");
 let result:Result<(),String>=runtime::with_repair_pause(&root,|pause|{
  // Mutate the request rather than the ACK: the worker refreshes its ACK every
  // 20ms, so racing a direct ACK overwrite would make this assertion flaky.
  let mut stop=store::read_json(&root.join(migration::STOP))?;let original=stop.clone();stop["nonce"]=json!("wrong");store::atomic_json(&root.join(migration::STOP),&stop)?;
  assert!(pause.verify().is_err());store::atomic_json(&root.join(migration::STOP),&original)?;Err("fixture callback failure".into())
 });
 assert_eq!(result.unwrap_err(),"fixture callback failure");assert!(!root.join(migration::STOP).exists());wait(&root,"ok");
}
