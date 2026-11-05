// Producer responses must fit Codex's 3s/5s hook budget while GUI readiness
// remains a separate synchronous check. Only UUID fixtures/children are used.
use std::{fs,io::{Read,Write},path::{Path,PathBuf},process::{Child,Command,Stdio},time::{Duration,Instant}};
use floatboard_core::{runtime::{self,ProcessLock},store::{self,Store}};
use serde_json::{json,Value};
const EXE:&str=env!("CARGO_BIN_EXE_FloatboardReporter");
struct Fixture(PathBuf);
impl Fixture{fn new()->Self{let path=std::env::temp_dir().join(format!("floatboard-producer-startup-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&path).unwrap();drop(Store::open(&path,false).unwrap());Self(path)}}
impl Drop for Fixture{fn drop(&mut self){if std::thread::panicking(){eprintln!("producer fixture retained: {}",self.0.display());}else{let _=fs::remove_dir_all(&self.0);}}}
fn command(root:&Path,mode:&str)->Command{let mut c=Command::new(EXE);c.args([mode,"--data-dir"]).arg(root).env_remove("FLOATBOARD_PREFLIGHT").env_remove("FLOATBOARD_NO_WORKER").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());c}
fn wait(mut condition:impl FnMut()->bool,seconds:u64){let until=Instant::now()+Duration::from_secs(seconds);loop{if condition(){break;}assert!(Instant::now()<until,"fixture did not reach expected state");std::thread::sleep(Duration::from_millis(10));}}
struct Worker{root:PathBuf,child:Child}
impl Worker{fn start(root:&Path)->Self{let mut c=command(root,"--worker");c.stdin(Stdio::null()).stdout(Stdio::null());Self{root:root.into(),child:c.spawn().unwrap()}}}
impl Drop for Worker{fn drop(&mut self){let _=store::atomic_json(&self.root.join("worker-shutdown.json"),&json!({}));wait(||self.child.try_wait().unwrap().is_some(),5);}}
fn response(root:&Path,mode:&str,input:&str)->(bool,Vec<u8>){
 let mut child=command(root,mode).spawn().unwrap();child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
 let mut stdout=child.stdout.take().unwrap();let(tx,rx)=std::sync::mpsc::channel();let reader=std::thread::spawn(move||{let mut bytes=vec![];stdout.read_to_end(&mut bytes).unwrap();let _=tx.send(bytes);});
 let timely=match rx.recv_timeout(Duration::from_millis(2500)){Ok(bytes)=>{assert!(child.wait().unwrap().success());reader.join().unwrap();return (true,bytes);},Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>false,Err(error)=>panic!("producer output failed: {error}")};
 // Preserve the initial timeout assertion while letting only our child reach
 // its existing bounded 8s startup wait and exit normally. No process-name kill.
 let bytes=rx.recv_timeout(Duration::from_secs(10)).unwrap();assert!(child.wait().unwrap().success());reader.join().unwrap();(timely,bytes)
}
fn background_done(root:&Path){
 wait(||root.join("runtime/worker-check.json").exists(),5);
 let value=store::read_json(&root.join("runtime/worker-check.json")).unwrap();assert!(value["status"]=="ok"||value["status"]=="error");
}

#[test]
fn background_check_keeps_legacy_and_duplicate_owner_protection(){
 let f=Fixture::new();let _owner=ProcessLock::acquire(&f.0.join("collector.lock")).unwrap();
 store::atomic_json(&f.0.join("health.json"),&json!({"pid":std::process::id(),"at":floatboard_core::now(),"version":"0.7.0","migration_protocol":1,"status":"ok"})).unwrap();
 let(timely,bytes)=response(&f.0,"--hook",&json!({"hook_event_name":"SessionStart","session_id":"fixture-legacy","cwd":"C:/isolated"}).to_string());
 assert!(timely);assert!(serde_json::from_slice::<Value>(&bytes).unwrap()["hookSpecificOutput"]["additionalContext"].is_string());background_done(&f.0);
 let check=store::read_json(&f.0.join("runtime/worker-check.json")).unwrap();assert_eq!(check["status"],"error");assert_eq!(check["diagnostic"]["stage"],"worker.legacy_protocol");assert!(!f.0.join("worker-shutdown.json").exists());assert!(ProcessLock::acquire(&f.0.join("collector.lock")).is_err());
}
#[test]
fn mcp_initialize_and_session_end_fit_deadline_while_real_worker_is_starting(){
 for mode in ["--mcp","--hook"]{
  let f=Fixture::new();let migration=ProcessLock::acquire(&f.0.join(floatboard_core::migration::LOCK)).unwrap();let worker=Worker::start(&f.0);
  wait(||store::read_json(&f.0.join("health.json")).is_ok_and(|v|v["status"]=="starting"),5);
  let input=if mode=="--mcp"{"{\"id\":1,\"method\":\"initialize\"}\n".into()}else{json!({"hook_event_name":"SessionEnd","session_id":"fixture-end","cwd":"C:/isolated"}).to_string()};
  let(timely,bytes)=response(&f.0,mode,&input);drop(migration);
  wait(||store::read_json(&f.0.join("health.json")).is_ok_and(|v|v["status"]=="ok"),5);
  background_done(&f.0);runtime::spawn_worker(&f.0).unwrap();drop(worker);
  let output:Value=serde_json::from_slice(&bytes).unwrap();if mode=="--mcp"{assert_eq!(output["id"],1);}else{assert_eq!(output,json!({}));}
  assert!(timely,"{mode} exceeded 2.5s while collector was starting; SessionEnd budget is only 3s");
 }
}
#[test]
fn session_binding_is_returned_before_an_existing_starting_owner_is_ready(){
 let f=Fixture::new();let _owner=ProcessLock::acquire(&f.0.join("collector.lock")).unwrap();
 store::atomic_json(&f.0.join("health.json"),&json!({"pid":std::process::id(),"at":floatboard_core::now(),"version":env!("CARGO_PKG_VERSION"),"status":"starting"})).unwrap();
 let(timely,bytes)=response(&f.0,"--hook",&json!({"hook_event_name":"SessionStart","session_id":"fixture-start","cwd":"C:/isolated"}).to_string());
 store::atomic_json(&f.0.join("health.json"),&json!({"pid":std::process::id(),"at":floatboard_core::now(),"version":env!("CARGO_PKG_VERSION"),"status":"ok"})).unwrap();background_done(&f.0);
 let output:Value=serde_json::from_slice(&bytes).unwrap();assert!(output["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("fixture-start"));
 assert!(store::session_path(&f.0,"fixture-start").is_file());assert!(timely,"binding response must fit within 2.5s even if storage readiness takes 8s");
}

#[test]
fn competing_background_checks_reuse_one_current_worker(){
 let f=Fixture::new();let worker=Worker::start(&f.0);wait(||store::read_json(&f.0.join("health.json")).is_ok_and(|v|v["status"]=="ok"),5);let pid=worker.child.id();
 let mut a=command(&f.0,"--ensure-worker").spawn().unwrap();let mut b=command(&f.0,"--ensure-worker").spawn().unwrap();
 wait(||a.try_wait().unwrap().is_some()&&b.try_wait().unwrap().is_some(),5);
 assert!(a.wait().unwrap().success()||b.wait().unwrap().success());
 assert_eq!(store::read_json(&f.0.join("health.json")).unwrap()["pid"],pid);assert!(ProcessLock::acquire(&f.0.join("collector.lock")).is_err());assert!(!f.0.join("worker-shutdown.json").exists());drop(worker);
}
