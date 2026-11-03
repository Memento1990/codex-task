use std::{fs, path::{Path,PathBuf}};
use floatboard_core::{migration, runtime::ProcessLock, store::{self,Store}};
use serde_json::json;

struct Sandbox(PathBuf);
impl Sandbox {
    fn new()->Self { let root=std::env::temp_dir().join(format!("floatboard-migration-{}",uuid::Uuid::new_v4().simple())); fs::create_dir_all(&root).unwrap(); Self(root) }
    fn path(&self,name:&str)->PathBuf { self.0.join(name) }
}
impl Drop for Sandbox { fn drop(&mut self){
    if let Ok(items)=fs::read_dir(&self.0){for entry in items.flatten(){if entry.path().is_dir(){let _=store::atomic_json(&entry.path().join("worker-shutdown.json"),&json!({}));}}}
    // Cooperatively stop only workers attached to this UUID sandbox.
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
    loop {
        let busy=fs::read_dir(&self.0).ok().into_iter().flatten().flatten().any(|entry|entry.path().is_dir() && ProcessLock::acquire(&entry.path().join("collector.lock")).is_err());
        if !busy || std::time::Instant::now()>=deadline{break;}
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _=fs::remove_dir_all(&self.0);
} }
fn seed(root:&Path){
    fs::create_dir_all(root).unwrap();
    let mut db=Store::open(root,false).unwrap();
    db.ingest(&json!({"event_id":"seed","kind":"SessionStart","session_id":"one","host_id":"test","recorded_at":floatboard_core::now()})).unwrap();
    store::register(root,"one","isolated").unwrap();
    store::atomic_json(&root.join("preferences.json"),&json!({"theme":"dark","auto_start":false})).unwrap();
    store::atomic_json(&root.join("integration.json"),&json!({"home":"isolated-codex-do-not-touch","reporter_version":env!("CARGO_PKG_VERSION"),"contribution":{"args":["--data-dir",root]}})).unwrap();
}
#[test]
fn consistent_snapshot_preserves_old_data_and_redirects_old_root(){
    let test=Sandbox::new();let old=test.path("old");let new=test.path("new");seed(&old);
    // Keep a connection alive and uncheckpointed WAL pages: raw DB copying loses these.
    let source=Store::open(&old,false).unwrap();
    source.db.execute("insert into events values('wal','test:one','SessionEnd',1,'{}')",[]).unwrap();
    assert_eq!(migration::migrate(&old,&new).unwrap(),new);
    assert_eq!(migration::resolve_root(&old).unwrap(),new);
    assert!(old.join("events.sqlite3").exists());assert!(old.join("sessions").exists());
    assert_eq!(fs::read(old.join("integration.json")).unwrap(),fs::read(new.join("integration.json")).unwrap());
    assert_eq!(fs::read(old.join("preferences.json")).unwrap(),fs::read(new.join("preferences.json")).unwrap());
    let target=Store::open(&new,true).unwrap();
    assert_eq!(target.db.query_row("select count(*) from events",[],|r|r.get::<_,i64>(0)).unwrap(),2);
}
#[test]
fn old_long_lived_progress_client_resolves_each_call(){
    let test=Sandbox::new();let old=test.path("old");let new=test.path("new");seed(&old);
    let record=store::register(&old,"one","isolated").unwrap();
    migration::migrate(&old,&new).unwrap();
    let result=floatboard_core::ingest::progress(&old,&json!({"session_id":"one","session_token":record["session_token"],"revision":1,"title":"moved","status":"running","plan":[],"blockers":[],"evidence":[]})).unwrap();
    let event=format!("{}.json",result["event_id"].as_str().unwrap());
    assert!(new.join("inbox").join(&event).exists());assert!(!old.join("inbox").join(&event).exists());
}
#[test]
fn rejects_nonempty_same_nested_relative_and_unc_destinations(){
    let test=Sandbox::new();let old=test.path("old");seed(&old);
    let busy=test.path("busy");fs::create_dir_all(&busy).unwrap();fs::write(busy.join("keep"),"untouched").unwrap();
    for invalid in [old.clone(),old.join("child"),test.0.clone(),busy.clone(),PathBuf::from("relative"),PathBuf::from(r"\\server\share\data")] {
        assert!(migration::migrate(&old,&invalid).is_err(),"{invalid:?}");
        assert!(!old.join(migration::REDIRECT).exists());
    }
    assert_eq!(fs::read(busy.join("keep")).unwrap(),b"untouched");
}
#[test]
fn uncooperative_collector_is_never_killed_or_copied(){
    let test=Sandbox::new();let old=test.path("old");let new=test.path("new");seed(&old);
    let _legacy=ProcessLock::acquire(&old.join("collector.lock")).unwrap();
    let error=migration::migrate(&old,&new).unwrap_err();
    assert!(error.contains("未安全停止"));assert!(!new.exists());
    assert!(!old.join(migration::REDIRECT).exists());assert!(!old.join(migration::STOP).exists());
}
#[test]
fn concurrent_migration_and_producer_fail_closed(){
    let test=Sandbox::new();let old=test.path("old");seed(&old);
    let _lock=ProcessLock::acquire(&old.join(migration::LOCK)).unwrap();
    assert!(migration::migrate(&old,&test.path("new")).is_err());
    assert!(migration::with_root(&old,|_|Ok(())).is_err());
}
#[test]
fn in_flight_notification_blocks_migration_before_copy(){
    let test=Sandbox::new();let old=test.path("old");let new=test.path("new");seed(&old);
    let _sending=ProcessLock::acquire(&old.join("notification-activity.lock")).unwrap();
    let error=migration::migrate(&old,&new).unwrap_err();
    assert!(error.contains("通知正在发送"));assert!(!new.exists());
    assert!(!old.join(migration::REDIRECT).exists());assert!(!old.join(migration::STOP).exists());
}
#[test]
fn real_process_handoff_preserves_pid_and_origin_collector_ownership(){
    let test=Sandbox::new();let old=test.path("old");let new=test.path("new");seed(&old);
    floatboard_core::runtime::spawn_worker(&old).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    let before=loop {
        if let Ok(health)=store::read_json(&old.join("health.json")){break health["pid"].clone();}
        assert!(std::time::Instant::now()<deadline);std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_ne!(before,json!(std::process::id()));
    let running=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let monitor_running=running.clone();let monitor_old=old.clone();
    let monitor=std::thread::spawn(move||{
        while monitor_running.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(ProcessLock::acquire(&monitor_old.join("collector.lock")).is_err(),"old binary could steal source collector identity");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
    migration::migrate(&old,&new).unwrap();
    floatboard_core::runtime::spawn_worker(&new).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    loop {
        if let Ok(health)=store::read_json(&new.join("health.json")){assert_eq!(health["pid"],before);break;}
        assert!(std::time::Instant::now()<deadline);std::thread::sleep(std::time::Duration::from_millis(10));
    }
    running.store(false,std::sync::atomic::Ordering::SeqCst);monitor.join().unwrap();
}
#[test]
fn stale_stop_after_crashed_migrator_is_recovered(){
    let test=Sandbox::new();let old=test.path("old");seed(&old);
    store::atomic_json(&old.join(migration::STOP),&json!({"nonce":"crashed"})).unwrap();
    floatboard_core::runtime::spawn_worker(&old).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    while !old.join("health.json").exists(){assert!(std::time::Instant::now()<deadline);std::thread::sleep(std::time::Duration::from_millis(10));}
    assert!(!old.join(migration::STOP).exists());
    assert!(ProcessLock::acquire(&old.join("collector.lock")).is_err());
}
#[test]
fn engine_rejects_unupgraded_integration_before_any_migration(){
    let test=Sandbox::new();let old=test.path("old");seed(&old);
    store::atomic_json(&old.join("integration.json"),&json!({"home":"isolated","contribution":{}})).unwrap();
    let mut engine=floatboard_core::desktop::Engine::new(old.clone(),test.path("fake-gui.exe"));
    let error=engine.migrate_data(&test.path("new")).unwrap_err();
    assert!(error.contains("新版重新连接"));assert!(!old.join(migration::REDIRECT).exists());assert!(!test.path("new").exists());
}
#[test]
fn engine_switch_closes_old_store_and_requires_restart_for_mutations(){
    let test=Sandbox::new();let old=test.path("old");let new=test.path("new");seed(&old);
    let mut engine=floatboard_core::desktop::Engine::new(old.clone(),test.path("fake-gui.exe"));
    assert_eq!(engine.call("data_location",json!({})).unwrap()["current"],json!(old));
    assert!(engine.call("migrate_data",json!({"path":new})).unwrap_err().contains("暂停上报"));
    let result=engine.call("migrate_data",json!({"path":new,"producers_stopped":true})).unwrap();
    assert_eq!(result["restart_required"],true);assert_eq!(engine.root,new);
    assert_eq!(engine.call("data_location",json!({})).unwrap()["current"],json!(new));
    assert!(engine.call("window_save",json!({"x":10})).is_err());
    assert!(engine.call("preferences",json!({})).is_err());
    assert!(!old.join("window.json").exists());assert!(!new.join("window.json").exists());
    assert!(engine.call("window_load",json!({})).is_ok());
}
#[test]
fn redirects_chain_and_corruption_fail_closed(){
    let test=Sandbox::new();let old=test.path("old");let next=test.path("next");let last=test.path("last");seed(&old);
    migration::migrate(&old,&next).unwrap();migration::migrate(&next,&last).unwrap();
    assert_eq!(migration::resolve_root(&old).unwrap(),last);
    store::atomic_json(&last.join(migration::REDIRECT),&json!({"version":1,"path":old})).unwrap();
    assert!(migration::resolve_root(&old).is_err());
    fs::write(last.join(migration::REDIRECT),"broken").unwrap();assert!(migration::resolve_root(&old).is_err());
}
#[test]
fn cooperative_worker_exits_without_kill_and_new_worker_drains_legacy_inbox(){
    let test=Sandbox::new();let old=test.path("old");let new=test.path("new");seed(&old);
    let worker_root=old.clone();
    let worker=std::thread::spawn(move||floatboard_core::runtime::worker(&worker_root));
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
    while !old.join("health.json").exists() {
        assert!(std::time::Instant::now()<deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    migration::migrate(&old,&new).unwrap();
    assert!(ProcessLock::acquire(&old.join("collector.lock")).is_err());
    store::enqueue(&old,json!({"kind":"SessionEnd","session_id":"one","host_id":"test"})).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
    loop {
        let store=Store::open(&new,true).unwrap();
        let count=store.db.query_row("select count(*) from events",[],|r|r.get::<_,i64>(0)).unwrap();
        if count==2 {break;}
        if std::time::Instant::now()>=deadline {
            store::atomic_json(&new.join("worker-shutdown.json"),&json!({})).unwrap();worker.join().unwrap().unwrap();
            panic!("legacy inbox was not consumed");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    store::atomic_json(&new.join("worker-shutdown.json"),&json!({})).unwrap();worker.join().unwrap().unwrap();
}
#[test]
fn reparse_directory_is_rejected_without_traversal(){
    let test=Sandbox::new();let old=test.path("old");let outside=test.path("outside");seed(&old);fs::create_dir_all(&outside).unwrap();
    // Junctions do not require the Windows symlink privilege.
    let link=old.join("junction");
    let status=std::process::Command::new("cmd").args(["/C","mklink","/J"]).arg(&link).arg(&outside).status().unwrap();
    assert!(status.success());
    assert!(migration::migrate(&old,&test.path("new")).unwrap_err().contains("reparse"));
    fs::remove_dir(&link).unwrap();
}
