use floatboard_core::{repair_restart,store,migration};
use serde_json::json;
use std::fs;
#[test]
fn abandoned_health_wait_expires_only_after_executor_releases_lock(){
 let base=std::env::temp_dir().join(format!("floatboard-repair-deadline-{}",uuid::Uuid::new_v4()));let root=base.join("data");fs::create_dir_all(&root).unwrap();
 let control=base.join("control/database-restart");let latest=control.join("latest.json");
 let record=json!({"status":"awaiting_health","health_deadline":floatboard_core::now()-1.,"source":root,"destination":root,"recovered_tasks":1});store::atomic_json(&latest,&record).unwrap();
 let executor=floatboard_core::runtime::ProcessLock::acquire(&control.join("executor.lock")).unwrap();
 assert_eq!(repair_restart::status(&root).unwrap()["status"],"awaiting_health");drop(executor);
 assert_eq!(repair_restart::status(&root).unwrap()["status"],"health_failed");
 let future=json!({"status":"awaiting_health","health_deadline":floatboard_core::now()+60.});store::atomic_json(&latest,&future).unwrap();assert_eq!(repair_restart::status(&root).unwrap()["status"],"awaiting_health");
 assert!(!root.join(migration::REDIRECT).exists());fs::remove_dir_all(base).unwrap();
}
#[test]
fn health_requires_committed_route_readable_tasks_and_collector_then_recovers_normal_launch(){
 let base=std::env::temp_dir().join(format!("floatboard-repair-health-{}",uuid::Uuid::new_v4()));let source=base.join("data");let dest=base.join("data-repaired-fixture");fs::create_dir_all(&source).unwrap();fs::create_dir_all(&dest).unwrap();
 let latest=base.join("control/database-restart/latest.json");let record=json!({"status":"health_failed","nonce":uuid::Uuid::new_v4().to_string(),"source":source,"destination":dest,"recovered_tasks":1});store::atomic_json(&latest,&record).unwrap();
 let healthy=json!({"storage":{"status":"ok"},"healthy":true,"tasks":[{"id":"task"}]});
 repair_restart::acknowledge(&dest,&healthy).unwrap();assert_eq!(repair_restart::status(&dest).unwrap()["status"],"health_failed");
 store::atomic_json(&source.join(migration::REDIRECT),&json!({"version":1,"path":dest})).unwrap();
 for bad in [json!({"storage":{"status":"error"},"healthy":true,"tasks":[{}]}),json!({"storage":{"status":"ok"},"healthy":false,"tasks":[{}]}),json!({"storage":{"status":"ok"},"healthy":true,"tasks":[]})]{repair_restart::acknowledge(&dest,&bad).unwrap();assert_eq!(repair_restart::status(&dest).unwrap()["status"],"health_failed");}
 repair_restart::acknowledge(&dest,&healthy).unwrap();assert_eq!(repair_restart::status(&dest).unwrap()["status"],"completed");fs::remove_dir_all(base).unwrap();
}
