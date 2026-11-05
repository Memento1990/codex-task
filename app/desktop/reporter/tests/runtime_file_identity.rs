use std::{fs,io::Write,os::windows::{fs::OpenOptionsExt,io::AsRawHandle},path::PathBuf};
use floatboard_core::store::Store;
use windows_sys::Win32::Storage::FileSystem::{LockFile,UnlockFile};

struct Fixture(PathBuf);
impl Fixture{fn new()->Self{let p=std::env::temp_dir().join(format!("floatboard-file-identity-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&p).unwrap();Self(p)}}
impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}

#[test]
fn replaced_main_and_collector_with_shared_wal_refuses_a_second_store(){
 for shape in ["unlink","renamed","missing"] {
 let f=Fixture::new();let root=f.0.join("data");fs::create_dir(&root).unwrap();drop(Store::open(&root,false).unwrap());
 let lock_path=root.join("collector.lock");let mut old_lock=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(7).open(&lock_path).unwrap();old_lock.write_all(b"0").unwrap();
 assert_ne!(unsafe{LockFile(old_lock.as_raw_handle(),0,0,1,0)},0);
 let old_main=fs::OpenOptions::new().read(true).write(true).share_mode(7).open(root.join("events.sqlite3")).unwrap();
 let before=fs::read(root.join("events.sqlite3")).unwrap();
 if shape=="renamed"{fs::rename(&lock_path,root.join("collector.lock.old")).unwrap();fs::rename(root.join("events.sqlite3"),root.join("events.sqlite3.old")).unwrap();}
 else{fs::remove_file(&lock_path).unwrap();fs::remove_file(root.join("events.sqlite3")).unwrap();}
 fs::write(&lock_path,b"0").unwrap();if shape!="missing"{fs::write(root.join("events.sqlite3"),&before).unwrap();}
 // Existing writers may still retain the same WAL/SHM while main/lock names changed.
 fs::write(root.join("events.sqlite3-wal"),[]).unwrap();fs::write(root.join("events.sqlite3-shm"),[]).unwrap();
 let wal=fs::OpenOptions::new().read(true).write(true).share_mode(7).open(root.join("events.sqlite3-wal")).unwrap();
 let shm=fs::OpenOptions::new().read(true).write(true).share_mode(7).open(root.join("events.sqlite3-shm")).unwrap();
 let result=Store::open(&root,true);
 assert!(result.is_err(),"{shape}: replaced identities must be rejected before SQLite opens the shared WAL");
 let mut engine=floatboard_core::desktop::Engine::new(root.clone(),root.join("fixture.exe"));let snapshot=engine.call("snapshot",serde_json::json!({"notify":false})).unwrap();
 assert_eq!(snapshot["storage"]["diagnostic"]["stage"],"file_identity.split");assert!(snapshot["storage"]["message"].as_str().unwrap().contains("正常重启 Windows"));assert!(snapshot["collector"]["guidance"].as_str().unwrap().contains("正常重启 Windows"));
 assert!(floatboard_core::runtime::spawn_worker(&root).unwrap_err().contains("file_identity.split"));
 let mut called=false;assert!(floatboard_core::runtime::with_repair_pause(&root,|_|{called=true;Ok(())}).unwrap_err().contains("file_identity.split"));assert!(!called);
 if shape=="missing"{assert!(!root.join("events.sqlite3").exists(),"identity checks must precede creating a missing main database");}
 unsafe{UnlockFile(old_lock.as_raw_handle(),0,0,1,0);}
 drop((old_lock,old_main,wal,shm));
 let error=result.err().unwrap();assert!(error.contains("文件身份"),"{error}");
 }
}

#[test]
fn current_store_and_process_lock_cannot_be_replaced_while_owned(){
 let f=Fixture::new();let mut store=Store::open(&f.0,false).unwrap();
 let lock_path=f.0.join("collector.lock");let lock=floatboard_core::runtime::ProcessLock::acquire(&lock_path).unwrap();
 assert!(fs::remove_file(&lock_path).is_err());assert!(fs::remove_file(f.0.join("events.sqlite3")).is_err());
 lock.verify_current().unwrap();assert!(store.tasks().unwrap().is_empty());
 drop((lock,store));fs::remove_file(lock_path).unwrap();
}

#[test]
fn sqlite_codes_survive_snapshot_without_private_error_text(){
 let f=Fixture::new();fs::write(f.0.join("events.sqlite3"),b"not a sqlite database: secret fixture").unwrap();
 let mut engine=floatboard_core::desktop::Engine::new(f.0.clone(),f.0.join("fixture.exe"));
 let snapshot=engine.call("snapshot",serde_json::json!({"notify":false})).unwrap();
 let diagnostic=&snapshot["storage"]["diagnostic"];
 assert_eq!(diagnostic["stage"],"timing.read");assert_eq!(diagnostic["sqlite_code"],26);assert_eq!(diagnostic["sqlite_extended_code"],26);
 let saved=fs::read_to_string(f.0.join("runtime/storage-last-error.json")).unwrap();
 assert!(!saved.contains("secret"));assert!(!saved.contains("select"));assert!(saved.contains("timing.read"));
}

#[test]
fn metadata_inventory_preserves_live_lock_ownership(){
 let f=Fixture::new();let _store=Store::open(&f.0,false).unwrap();
 let lock_path=f.0.join("collector.lock");let _lock=floatboard_core::runtime::ProcessLock::acquire(&lock_path).unwrap();
 assert!(floatboard_core::file_identity::split_owners(&f.0).unwrap().is_empty());
 assert!(floatboard_core::runtime::ProcessLock::acquire(&lock_path).is_err(),"metadata inspection must retain the owner's byte lock");
}

#[test]
fn unrelated_detached_file_does_not_block_an_intact_database_owner(){
 let f=Fixture::new();let _store=Store::open(&f.0,false).unwrap();
 let _lock=floatboard_core::runtime::ProcessLock::acquire(&f.0.join("collector.lock")).unwrap();
 let unrelated=f.0.join("unrelated.tmp");fs::write(&unrelated,b"fixture").unwrap();
 let _detached=fs::OpenOptions::new().read(true).share_mode(7).open(&unrelated).unwrap();fs::remove_file(&unrelated).unwrap();
 let result=floatboard_core::file_identity::split_owners(&f.0).unwrap();
 assert!(result.is_empty(),"unrelated detached metadata must not be mistaken for a replaced main or lock");
}

#[test]
fn atomic_replace_waits_for_a_transient_windows_reader_without_losing_old_value(){
 let f=Fixture::new();let path=f.0.join("pause.json");fs::write(&path,b"{\"generation\":1}").unwrap();
 let reader=fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
 let (started_tx,started_rx)=std::sync::mpsc::channel();let (done_tx,done_rx)=std::sync::mpsc::channel();
 let output=path.clone();let writer=std::thread::spawn(move||{started_tx.send(()).unwrap();done_tx.send(floatboard_core::store::atomic_json(&output,&serde_json::json!({"generation":2}))).unwrap();});
 started_rx.recv().unwrap();
 let first=done_rx.recv_timeout(std::time::Duration::from_millis(40));
 assert!(matches!(first,Err(std::sync::mpsc::RecvTimeoutError::Timeout)),"transient reader should be retried, got {first:?}");
 assert_eq!(floatboard_core::store::read_json(&path).unwrap()["generation"],1);
 drop(reader);done_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap().unwrap();writer.join().unwrap();
 assert_eq!(floatboard_core::store::read_json(&path).unwrap()["generation"],2);
}

#[test]
fn atomic_replace_persistent_denial_keeps_old_value_and_numeric_diagnostic(){
 let f=Fixture::new();let path=f.0.join("pause.json");fs::write(&path,b"{\"generation\":1}").unwrap();
 let _reader=fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
 let error=floatboard_core::store::atomic_json(&path,&serde_json::json!({"generation":2})).unwrap_err();
 let diagnostic=floatboard_core::storage_error::diagnostic(&error);
 assert_eq!(diagnostic["stage"],"atomic.replace");assert_eq!(diagnostic["win32_code"],5);
 assert_eq!(floatboard_core::store::read_json(&path).unwrap()["generation"],1);
}

#[test]
fn occupied_starting_or_missing_heartbeat_is_never_reported_ready(){
 let f=Fixture::new();let _owner=floatboard_core::runtime::ProcessLock::acquire(&f.0.join("collector.lock")).unwrap();
 assert!(floatboard_core::runtime::spawn_worker(&f.0).unwrap_err().contains("worker.no_health"));
 for status in ["starting","storage_error"]{
  floatboard_core::store::atomic_json(&f.0.join("health.json"),&serde_json::json!({"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"at":floatboard_core::now(),"status":status})).unwrap();
  assert!(floatboard_core::runtime::spawn_worker(&f.0).unwrap_err().contains("worker.not_ready"));
 }
 for protocol in [serde_json::Value::Null,serde_json::json!(1)]{
  floatboard_core::store::atomic_json(&f.0.join("health.json"),&serde_json::json!({"pid":std::process::id(),"version":"0.7.0","migration_protocol":protocol,"at":floatboard_core::now(),"status":"ok"})).unwrap();
  assert!(floatboard_core::runtime::spawn_worker(&f.0).unwrap_err().contains("worker.legacy_protocol"));
  assert!(!f.0.join("worker-shutdown.json").exists());
 }
}

struct StopWorker(PathBuf);
impl Drop for StopWorker{fn drop(&mut self){
 let _=floatboard_core::store::atomic_json(&self.0.join("worker-shutdown.json"),&serde_json::json!({}));
 let until=std::time::Instant::now()+std::time::Duration::from_secs(4);
 while floatboard_core::runtime::ProcessLock::acquire(&self.0.join("collector.lock")).is_err()&&std::time::Instant::now()<until{std::thread::sleep(std::time::Duration::from_millis(10));}
}}

#[test]
fn startup_waits_for_healthy_store_and_records_normal_exit(){
 let f=Fixture::new();let worker=StopWorker(f.0.clone());floatboard_core::runtime::spawn_worker(&f.0).unwrap();
 let health=floatboard_core::store::read_json(&f.0.join("health.json")).unwrap();assert_eq!(health["status"],"ok");assert_eq!(health["version"],env!("CARGO_PKG_VERSION"));
 drop(worker);
 let until=std::time::Instant::now()+std::time::Duration::from_secs(2);
 loop{if let Ok(exit)=floatboard_core::store::read_json(&f.0.join("runtime/worker-last-exit.json")){assert_eq!(exit["pid"],health["pid"]);assert_eq!(exit["status"],"normal_exit");assert_eq!(exit["reason"],"shutdown_request");break;}assert!(std::time::Instant::now()<until);std::thread::sleep(std::time::Duration::from_millis(10));}
}

#[test]
fn pause_write_failure_records_stage_and_win32_code_before_worker_exits(){
 let f=Fixture::new();let _stop=StopWorker(f.0.clone());
 let mut child=std::process::Command::new(env!("CARGO_BIN_EXE_FloatboardReporter")).args(["--worker","--data-dir"]).arg(&f.0).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
 let until=std::time::Instant::now()+std::time::Duration::from_secs(6);
 loop{if floatboard_core::store::read_json(&f.0.join("health.json")).is_ok_and(|v|v["status"]=="ok"){break;}assert!(std::time::Instant::now()<until);std::thread::sleep(std::time::Duration::from_millis(10));}
 fs::create_dir(f.0.join(floatboard_core::migration::ACK)).unwrap();
 floatboard_core::store::atomic_json(&f.0.join(floatboard_core::migration::STOP),&serde_json::json!({"nonce":"fixture"})).unwrap();
 loop{if let Some(status)=child.try_wait().unwrap(){assert!(!status.success());break;}assert!(std::time::Instant::now()<until);std::thread::sleep(std::time::Duration::from_millis(10));}
 let exit=floatboard_core::store::read_json(&f.0.join("runtime/worker-last-exit.json")).unwrap();
 assert_eq!(exit["status"],"error_exit");assert_eq!(exit["diagnostic"]["stage"],"worker.pause_ack");assert_eq!(exit["diagnostic"]["win32_code"],5);
}
