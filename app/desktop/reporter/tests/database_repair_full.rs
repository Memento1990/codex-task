//! Synthetic fixtures only: complete recovery must retain events and episodes.
use floatboard_core::{database_repair,store::{self,Store}};
use serde_json::{json,Value};
use std::{fs,path::{Path,PathBuf}};

struct Fixture(PathBuf);
impl Fixture {
 fn new()->Self{let p=std::env::temp_dir().join(format!("floatboard-full-recovery-{}",uuid::Uuid::new_v4()));fs::create_dir_all(p.join("data")).unwrap();Self(p)}
 fn root(&self)->PathBuf{self.0.join("data")}
}
impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
fn report(revision:i64,status:&str)->Value{
 json!({"event_id":format!("fixture-event-{revision}"),"kind":"progress","session_id":"fixture","host_id":"test","recorded_at":1700000000.+revision as f64*10.,"report":{"revision":revision,"title":if revision<2{"first"}else{"second"},"status":status,"plan":[{"id":"s","title":"step","status":if status=="completed"{"completed"}else{"inProgress"}}],"blockers":[],"evidence":[{"label":"synthetic"}]}})
}
fn hashes(root:&Path)->Vec<(String,String)>{["events.sqlite3","events.sqlite3-wal","events.sqlite3-shm"].iter().filter_map(|n|fs::read(root.join(n)).ok().map(|v|(n.to_string(),store::hash(&v)))).collect()}
fn rows(s:&Store,table:&str)->Vec<String>{let sql=match table{"events"=>"select id||'|'||task_id||'|'||kind||'|'||at||'|'||payload from events order by id","tasks"=>"select id||'|'||payload from tasks order by id","completed_tasks"=>"select id||'|'||source_task_id||'|'||revision||'|'||payload from completed_tasks order by id",_=>panic!("fixture table")};s.db.prepare(sql).unwrap().query_map([],|r|r.get(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()}

#[test]
fn complete_recovery_preserves_wal_events_timing_and_all_completion_episodes(){
 let f=Fixture::new();let mut source=Store::open(&f.root(),false).unwrap();
 for (revision,status) in [(0,"running"),(1,"completed"),(2,"running"),(3,"completed")]{source.ingest(&report(revision,status)).unwrap();}
 let expected_tasks=source.tasks().unwrap();assert_eq!(expected_tasks.len(),2);
 let expected_events=rows(&source,"events");let expected_current=rows(&source,"tasks");let expected_archives=rows(&source,"completed_tasks");assert_eq!(expected_events.len(),4);assert_eq!(expected_archives.len(),2);
 // Keep the synthetic writer open: the source has uncheckpointed WAL pages.
 let before=hashes(&f.root());let result=database_repair::call(&f.root(),"recover_copy",&json!({"confirmed":true})).unwrap();
 assert_eq!(before,hashes(&f.root()));assert_eq!(result["status"],"full_recovery","{result}");assert_eq!(result["partial"],false);assert_eq!(result["recovery"]["mode"],"full");assert_eq!(result["recovery"]["history_recovered"],true);assert_eq!(result["recovery"]["timing_recovered"],true);assert_eq!(result["recovery"]["events_recovered"],4);assert_eq!(result["recovery"]["archives_recovered"],2);assert_eq!(result["coordinated_snapshot"],false);assert_eq!(result["apply_supported"],false);
 let dest=f.0.join(result["paths"]["recovered_database"].as_str().unwrap()).parent().unwrap().to_path_buf();let mut recovered=Store::open(&dest,true).unwrap();
 assert_eq!(rows(&recovered,"events"),expected_events);assert_eq!(rows(&recovered,"tasks"),expected_current);assert_eq!(rows(&recovered,"completed_tasks"),expected_archives);assert_eq!(recovered.tasks().unwrap(),expected_tasks);
 assert!(!dest.join("events.sqlite3-wal").exists());assert!(!dest.join("events.sqlite3-shm").exists());
}

#[test]
fn healthy_empty_database_is_classified_complete_but_does_not_enable_replacement(){
 let f=Fixture::new();drop(Store::open(&f.root(),false).unwrap());let before=hashes(&f.root());
 let result=database_repair::call(&f.root(),"recover_copy",&json!({"confirmed":true})).unwrap();
 assert_eq!(result["status"],"full_recovery","{result}");assert_eq!(result["recovery"]["recovered_tasks"],0);assert_eq!(before,hashes(&f.root()));assert!(floatboard_core::repair_transaction::prepare(&f.root()).is_err());
}
