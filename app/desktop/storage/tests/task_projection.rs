use std::{fs,path::PathBuf};
use floatboard_storage::{now,store::Store};
use serde_json::{json,Value};

struct Temp(PathBuf);
impl Temp{fn new()->Self{let root=std::env::temp_dir().join(format!("floatboard-projection-{}",uuid::Uuid::new_v4().simple()));fs::create_dir_all(&root).unwrap();Self(root)}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
fn event(kind:&str,at:f64)->Value{json!({"event_id":uuid::Uuid::new_v4().simple().to_string(),"recorded_at":at,"kind":kind,"session_id":"fixture","host_id":"test","cwd":"C:/isolated"})}
fn progress(revision:i64,status:&str,at:f64)->Value{
    let mut event=event("progress",at);
    event["report"]=json!({"revision":revision,"title":"projection task","status":status,"plan":[{"id":"step","title":"verify","status":if status=="completed"{"completed"}else{"inProgress"}}],"blockers":[],"evidence":[{"label":"report","reference":"C:/isolated/result.md"}]});event
}
fn task(store:&mut Store)->Value{store.tasks().unwrap().into_iter().find(|task|task["archived"]!=true).unwrap()}

#[test]
fn evidence_first_report_is_stable_across_reorder_removal_restart_and_episodes(){
    // This timestamp exposed a one-ULP JSON parse drift in release preparation.
    let temp=Temp::new();let at=1793876180.9820483_f64;let mut store=Store::open(&temp.0,false).unwrap();
    let mut first=progress(0,"running",at);first["report"]["evidence"][0]["at"]=json!(1);first["report"]["evidence"][0]["id"]=json!("untrusted-id");
    store.ingest(&first).unwrap();let initial=task(&mut store)["evidence"][0].clone();
    let stored_at:f64=store.db.query_row("select at from events order by rowid limit 1",[],|row|row.get(0)).unwrap();assert_eq!(stored_at.to_bits(),at.to_bits());
    assert_eq!(initial["at"],at);assert_eq!(initial["time_source"],"first_reported");assert_ne!(initial["id"],"untrusted-id");
    let mut next=progress(1,"waiting",at+10.);next["report"]["evidence"]=json!([{"label":"second","reference":"https://example.test/check"},first["report"]["evidence"][0]]);
    store.ingest(&next).unwrap();let evidence=task(&mut store)["evidence"].clone();assert_eq!(evidence[1],initial);assert_eq!(evidence[0]["at"],at+10.);assert_ne!(evidence[0]["id"],initial["id"]);
    let mut removed=progress(2,"running",at+20.);removed["report"]["evidence"]=json!([]);store.ingest(&removed).unwrap();
    store.ingest(&progress(3,"completed",at+30.)).unwrap();assert_eq!(task(&mut store)["evidence"][0],initial);
    store.ingest(&progress(1,"running",at+31.)).unwrap();assert_eq!(task(&mut store)["evidence"][0],initial);
    drop(store);let mut store=Store::open(&temp.0,false).unwrap();assert_eq!(task(&mut store)["evidence"][0],initial);
    store.ingest(&progress(4,"running",at+40.)).unwrap();let reopened=task(&mut store)["evidence"][0].clone();
    assert_eq!(reopened["at"],at+40.);assert_ne!(reopened["id"],initial["id"]);
    let archive=store.tasks().unwrap().into_iter().find(|task|task["archived"]==true).unwrap();assert_eq!(archive["evidence"][0],initial);
    drop(store);let mut store=Store::open(&temp.0,true).unwrap();assert_eq!(task(&mut store)["evidence"][0],reopened);
    let snapshot=store.tasks().unwrap();let archive=snapshot.iter().find(|task|task["archived"]==true).unwrap();assert_eq!(archive["evidence"][0],initial);
    let decoded:Vec<Value>=serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();assert_eq!(decoded,snapshot);
}

#[test]
fn waiting_start_tracks_current_waiting_segment_and_ignores_hooks_children_and_old_revisions(){
    let temp=Temp::new();let at=now()-100.;let mut store=Store::open(&temp.0,false).unwrap();
    store.ingest(&progress(0,"running",at)).unwrap();assert!(task(&mut store)["waiting_since"].is_null());
    store.ingest(&progress(1,"waiting",at+10.)).unwrap();assert_eq!(task(&mut store)["waiting_since"],at+10.);
    store.ingest(&progress(2,"waiting",at+20.)).unwrap();store.ingest(&event("Stop",at+21.)).unwrap();
    let mut child=progress(99,"running",at+22.);child["agent_id"]=json!("child");store.ingest(&child).unwrap();
    store.ingest(&progress(1,"running",at+23.)).unwrap();assert_eq!(task(&mut store)["waiting_since"],at+10.);
    store.ingest(&progress(3,"running",at+30.)).unwrap();assert!(task(&mut store)["waiting_since"].is_null());
    store.ingest(&progress(4,"waiting",at+40.)).unwrap();assert_eq!(task(&mut store)["waiting_since"],at+40.);
    drop(store);let mut store=Store::open(&temp.0,true).unwrap();assert_eq!(task(&mut store)["waiting_since"],at+40.);
}

#[test]
fn projection_preserves_partial_and_cumulative_blocker_timers_without_inventing_segments(){
    let temp=Temp::new();let at=now()-100.;let mut store=Store::open(&temp.0,false).unwrap();
    let mut first=progress(0,"running",at);first["report"]["plan"][0]["status"]=json!("completed");store.ingest(&first).unwrap();
    let current=task(&mut store);assert_eq!(current["plan"][0]["timing"]["partial"],true);assert!(current["plan"][0]["timing"]["started_at"].is_null());
    for (revision,offset,open) in [(1,10.,true),(2,20.,false),(3,30.,true)]{
        let mut report=progress(revision,if open{"blocked"}else{"running"},at+offset);report["report"]["blockers"]=json!([{"id":"b","title":"approval","open":open}]);store.ingest(&report).unwrap();
    }
    let current=task(&mut store);assert_eq!(current["blockers"][0]["timing"]["seconds"],10.);assert_eq!(current["blockers"][0]["timing"]["since"],at+30.);
    assert!(current.get("segments").is_none());assert!(current.get("history").is_none());
}

#[test]
fn legacy_rows_without_business_events_keep_unknown_times_and_readonly_storage(){
    let temp=Temp::new();let store=Store::open(&temp.0,false).unwrap();
    let legacy=json!({"id":"legacy","title":"legacy task","revision":3,"status":"waiting","plan":[],"blockers":[],"evidence":[{"label":"old evidence","reference":"C:/old/result.md"}],"updated_at":now()-100.});
    store.db.execute("insert into tasks values(?1,?2)",["legacy",&legacy.to_string()]).unwrap();
    let mut archive=legacy.clone();archive["id"]=json!("archive:legacy");archive["status"]=json!("completed");archive["archived"]=json!(true);archive["source_task_id"]=json!("another-task");
    store.db.execute("insert into completed_tasks values(?1,?2,?3,?4)",rusqlite::params!["archive:legacy","another-task",3,archive.to_string()]).unwrap();
    drop(store);let mut store=Store::open(&temp.0,true).unwrap();let tasks=store.tasks().unwrap();assert_eq!(tasks.len(),2);
    for task in tasks{assert!(task["waiting_since"].is_null());assert!(task["evidence"][0]["at"].is_null());assert_eq!(task["evidence"][0]["time_source"],"unknown");}
    let saved:String=store.db.query_row("select payload from tasks where id='legacy'",[],|row|row.get(0)).unwrap();assert_eq!(saved,legacy.to_string());
    let saved:String=store.db.query_row("select payload from completed_tasks where id='archive:legacy'",[],|row|row.get(0)).unwrap();assert_eq!(saved,archive.to_string());
}
