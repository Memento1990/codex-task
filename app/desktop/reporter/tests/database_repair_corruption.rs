pub use floatboard_core::{Result,store};
#[path="../src/database_repair.rs"]mod database_repair;
use std::{fs,io::{Seek,SeekFrom,Write}};
use serde_json::json;
#[test]
fn damaged_events_btree_still_recovers_task_snapshots_without_invented_timing(){
 let p=std::env::temp_dir().join(format!("floatboard-repair-corrupt-{}",uuid::Uuid::new_v4()));let root=p.join("data");
 let s=store::Store::open(&root,false).unwrap();
 s.db.execute("insert into tasks values('h:s',?1)",[json!({"id":"h:s","revision":4,"status":"completed","plan":[],"blockers":[],"evidence":[]}).to_string()]).unwrap();
 let page:i64=s.db.query_row("select rootpage from sqlite_master where name='events'",[],|r|r.get(0)).unwrap();let size:i64=s.db.query_row("pragma page_size",[],|r|r.get(0)).unwrap();s.db.execute_batch("pragma wal_checkpoint(TRUNCATE)").unwrap();drop(s);
 let path=root.join("events.sqlite3");let mut f=fs::OpenOptions::new().write(true).open(&path).unwrap();f.seek(SeekFrom::Start(((page-1)*size)as u64)).unwrap();f.write_all(&vec![0;size as usize]).unwrap();f.sync_all().unwrap();drop(f);
 let hash=store::hash(&fs::read(&path).unwrap());let report=database_repair::call(&root,"recover_copy",&json!({"confirmed":true})).unwrap();assert_eq!(hash,store::hash(&fs::read(&path).unwrap()));assert_eq!(report["source_copy_integrity_ok"],false);assert_eq!(report["status"],"partial_recovery");assert_eq!(report["recovery"]["recovered_tasks"],1);assert_eq!(report["recovery"]["events_recovered"],0);assert_eq!(report["recovery"]["timing_recovered"],false);assert_eq!(report["recovery"]["destination_integrity_ok"],true);assert!(report["recovery"]["omitted_events"].is_null() || report["recovery"]["omitted_events"]==0);fs::remove_dir_all(p).unwrap();
}
