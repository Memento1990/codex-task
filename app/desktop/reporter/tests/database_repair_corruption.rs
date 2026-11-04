pub use floatboard_core::{Result,store};
#[path="../src/database_repair.rs"]mod database_repair;
use std::{fs,io::{Seek,SeekFrom,Write}};

#[test]
fn damaged_first_task_overflow_does_not_hide_other_indexed_tasks(){
 let p=std::env::temp_dir().join(format!("floatboard-repair-task-corrupt-{}",uuid::Uuid::new_v4()));let root=p.join("data");
 let s=store::Store::open(&root,false).unwrap();
 for id in ["broken:first","good:second","good:third"] {
  let task=json!({"id":id,"revision":1,"status":"completed","plan":[],"blockers":[],"evidence":[],"title":"x".repeat(16000)});
  s.db.execute("insert into tasks(id,payload) values(?1,?2)",rusqlite::params![id,task.to_string()]).unwrap();
 }
 let page:i64=s.db.query_row("select rootpage from sqlite_master where name='tasks'",[],|r|r.get(0)).unwrap();
 let size:i64=s.db.query_row("pragma page_size",[],|r|r.get(0)).unwrap();s.db.execute_batch("pragma wal_checkpoint(TRUNCATE)").unwrap();drop(s);
 let path=root.join("events.sqlite3");let mut bytes=fs::read(&path).unwrap();let mut start=((page-1)*size) as usize;
 while bytes[start]==5 {let first=start+u16::from_be_bytes([bytes[start+12],bytes[start+13]]) as usize;let child=u32::from_be_bytes(bytes[first..first+4].try_into().unwrap()) as usize;start=(child-1)*size as usize;}
 assert_eq!(bytes[start],13,"fixture tasks root must be a table leaf");
 let cell=start+u16::from_be_bytes([bytes[start+8],bytes[start+9]]) as usize;
 fn varint(b:&[u8],at:&mut usize)->usize{let mut v=0;for _ in 0..8{let n=b[*at];*at+=1;v=(v<<7)|(n&127) as usize;if n<128{return v;}}let n=b[*at];*at+=1;(v<<8)|n as usize}
 let mut at=cell;let payload=varint(&bytes,&mut at);let _rowid=varint(&bytes,&mut at);
 let usable=size as usize;let min_local=((usable-12)*32/255)-23;let max_local=usable-35;
 assert!(payload>max_local);let mut local=min_local+(payload-min_local)%(usable-4);if local>max_local{local=min_local;}
 // An out-of-range overflow link makes only the first row unreadable.
 let overflow=u32::from_be_bytes(bytes[at+local..at+local+4].try_into().unwrap()) as usize;
 let offset=(overflow-1)*usable;let invalid_page=(bytes.len()/usable+100) as u32;bytes[offset..offset+4].copy_from_slice(&invalid_page.to_be_bytes());fs::write(&path,&bytes).unwrap();
 let original=store::hash(&bytes);
 let report=database_repair::call(&root,"recover_copy",&json!({"confirmed":true})).unwrap();
 assert_eq!(report["status"],"partial_recovery");assert_eq!(report["recovery"]["recovered_tasks"],2);assert_eq!(report["recovery"]["indexed_fallback"],true);assert!(report["recovery"]["unreadable_rows"].as_u64().unwrap()>=1);assert_eq!(report["recovery"]["omitted_tasks"],1);assert_eq!(report["recovery"]["scan_complete"],false);assert_eq!(report["recovery"]["destination_integrity_ok"],true);
 assert_eq!(store::hash(&fs::read(&path).unwrap()),original);fs::remove_dir_all(p).unwrap();
}
use serde_json::json;
#[test]
fn damaged_middle_leaf_keeps_first_pass_results_without_duplicates(){
 let p=std::env::temp_dir().join(format!("floatboard-repair-middle-{}",uuid::Uuid::new_v4()));let root=p.join("data");
 let s=store::Store::open(&root,false).unwrap();
 for id in ["good:first","broken:middle","good:last"] {let task=json!({"id":id,"revision":1,"status":"completed","plan":[],"blockers":[],"evidence":[],"title":"x".repeat(16000)});s.db.execute("insert into tasks values(?1,?2)",rusqlite::params![id,task.to_string()]).unwrap();}
 let page:i64=s.db.query_row("select rootpage from sqlite_master where name='tasks'",[],|r|r.get(0)).unwrap();let size:i64=s.db.query_row("pragma page_size",[],|r|r.get(0)).unwrap();s.db.execute_batch("pragma wal_checkpoint(TRUNCATE)").unwrap();drop(s);
 let path=root.join("events.sqlite3");let mut bytes=fs::read(&path).unwrap();let start=((page-1)*size) as usize;assert_eq!(bytes[start],5);
 let cells=u16::from_be_bytes([bytes[start+3],bytes[start+4]]) as usize;assert!(cells>=2,"fixture must have multiple leaves");
 let cell=start+u16::from_be_bytes([bytes[start+14],bytes[start+15]]) as usize;let child=u32::from_be_bytes(bytes[cell..cell+4].try_into().unwrap()) as usize;
 let offset=(child-1)*size as usize;assert_eq!(bytes[offset],13);bytes[offset..offset+size as usize].fill(0);fs::write(&path,&bytes).unwrap();
 let report=database_repair::call(&root,"recover_copy",&json!({"confirmed":true})).unwrap();println!("middle_summary={}",report["recovery"]);
 assert_eq!(report["status"],"partial_recovery");assert_eq!(report["recovery"]["recovered_tasks"],2);assert_eq!(report["recovery"]["scan_complete"],false);assert_eq!(report["recovery"]["destination_integrity_ok"],true);assert_eq!(fs::read(&path).unwrap(),bytes);fs::remove_dir_all(p).unwrap();
}
#[test]
#[ignore = "opt-in disposable evidence copy; never point at production"]
fn disposable_evidence_recovers_independently_readable_tasks(){
 let input=std::path::PathBuf::from(std::env::var_os("FLOATBOARD_REPAIR_EVIDENCE_COPY").expect("explicit evidence-copy path required"));
 assert!(input.components().any(|c|c.as_os_str()=="artifacts"),"only repository artifact copies accepted");
 let p=std::env::temp_dir().join(format!("floatboard-evidence-test-{}",uuid::Uuid::new_v4()));let root=p.join("data");fs::create_dir_all(&root).unwrap();
 for name in ["events.sqlite3","events.sqlite3-wal","events.sqlite3-shm"]{if input.join(name).exists(){fs::copy(input.join(name),root.join(name)).unwrap();}}
 let before=store::hash(&fs::read(root.join("events.sqlite3")).unwrap());
 let report=database_repair::call(&root,"recover_copy",&json!({"confirmed":true})).unwrap();
 println!("recovery_summary={}",report["recovery"]);
 assert_eq!(report["status"],"partial_recovery");assert_eq!(report["recovery"]["recovered_tasks"],18);assert!(report["recovery"]["unreadable_rows"].as_u64().unwrap()>=8);assert_eq!(report["recovery"]["omitted_tasks"],8);assert_eq!(report["recovery"]["destination_integrity_ok"],true);
 assert_eq!(store::hash(&fs::read(root.join("events.sqlite3")).unwrap()),before);fs::remove_dir_all(p).unwrap();
}
#[test]
fn damaged_events_btree_still_recovers_task_snapshots_without_invented_timing(){
 let p=std::env::temp_dir().join(format!("floatboard-repair-corrupt-{}",uuid::Uuid::new_v4()));let root=p.join("data");
 let s=store::Store::open(&root,false).unwrap();
 s.db.execute("insert into tasks values('h:s',?1)",[json!({"id":"h:s","revision":4,"status":"completed","plan":[],"blockers":[],"evidence":[]}).to_string()]).unwrap();
 let page:i64=s.db.query_row("select rootpage from sqlite_master where name='events'",[],|r|r.get(0)).unwrap();let size:i64=s.db.query_row("pragma page_size",[],|r|r.get(0)).unwrap();s.db.execute_batch("pragma wal_checkpoint(TRUNCATE)").unwrap();drop(s);
 let path=root.join("events.sqlite3");let mut f=fs::OpenOptions::new().write(true).open(&path).unwrap();f.seek(SeekFrom::Start(((page-1)*size)as u64)).unwrap();f.write_all(&vec![0;size as usize]).unwrap();f.sync_all().unwrap();drop(f);
 let hash=store::hash(&fs::read(&path).unwrap());let report=database_repair::call(&root,"recover_copy",&json!({"confirmed":true})).unwrap();assert_eq!(hash,store::hash(&fs::read(&path).unwrap()));assert_eq!(report["source_copy_integrity_ok"],false);assert_eq!(report["status"],"partial_recovery");assert_eq!(report["recovery"]["recovered_tasks"],1);assert_eq!(report["recovery"]["events_recovered"],0);assert_eq!(report["recovery"]["timing_recovered"],false);assert_eq!(report["recovery"]["destination_integrity_ok"],true);assert!(report["recovery"]["omitted_events"].is_null() || report["recovery"]["omitted_events"]==0);fs::remove_dir_all(p).unwrap();
}
