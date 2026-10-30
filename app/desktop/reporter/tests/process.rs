use std::{fs,io::Write,path::PathBuf,process::{Command,Stdio,Child},time::{Duration,Instant}};
use serde_json::{json,Value};
use floatboard_core::{store::{self,Store},runtime};
const EXE:&str=env!("CARGO_BIN_EXE_FloatboardReporter");
struct Temp(PathBuf);
impl Temp{fn new()->Self{let path=std::env::temp_dir().join(format!("floatboard-process-{}",uuid::Uuid::new_v4().simple()));fs::create_dir_all(&path).unwrap();Self(path)}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
struct Worker(Child);
impl Drop for Worker{fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
fn wait(mut predicate:impl FnMut()->bool){let deadline=Instant::now()+Duration::from_secs(8);while Instant::now()<deadline{if predicate(){return;}std::thread::sleep(Duration::from_millis(25));}panic!("Windows process did not reach expected state");}
fn worker(root:&std::path::Path)->Worker{Worker(Command::new(EXE).arg("--worker").arg("--data-dir").arg(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap())}
fn run(mode:&str,root:&std::path::Path,input:&str)->std::process::Output{let mut child=Command::new(EXE).arg(mode).arg("--data-dir").arg(root).env("FLOATBOARD_PREFLIGHT","1").env("PYTHONIOENCODING","cp1252").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();child.wait_with_output().unwrap()}
#[test]
fn real_utf8_hook_and_mcp_roundtrip(){
    let temp=Temp::new();let hook=run("--hook",&temp.0,&json!({"hook_event_name":"SessionStart","session_id":"utf8","cwd":"C:/isolated"}).to_string());assert!(hook.status.success());let reply:Value=serde_json::from_slice(&hook.stdout).unwrap();assert!(reply["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("任务侧窗"));
    let binding=store::read_json(&store::session_path(&temp.0,"utf8")).unwrap();let args=json!({"session_id":"utf8","session_token":binding["session_token"],"revision":1,"title":"中文进度","status":"running","plan":[],"blockers":[],"evidence":[]});
    let requests=[json!({"id":1,"method":"initialize"}),json!({"id":2,"method":"tools/list"}),json!({"id":3,"method":"tools/call","params":{"name":"report_progress","arguments":args}})];let input=requests.iter().map(|v|format!("{v}\n")).collect::<String>();let output=run("--mcp",&temp.0,&input);assert!(output.status.success());let replies:Vec<Value>=String::from_utf8(output.stdout).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();assert_eq!(replies[2]["result"]["isError"],false);assert!(!serde_json::to_string(&replies).unwrap().contains(binding["session_token"].as_str().unwrap()));assert!(!temp.0.join("events.sqlite3").exists());
    let mut store=Store::open(&temp.0,false).unwrap();runtime::drain(&temp.0,&mut store).unwrap();assert_eq!(store.tasks().unwrap()[0]["title"],"中文进度");
}
#[test]
fn worker_singleton_restart_and_backlog(){
    let temp=Temp::new();let first=worker(&temp.0);wait(||temp.0.join("health.json").exists());let mut second=worker(&temp.0);wait(||second.0.try_wait().unwrap().is_some());assert!(second.0.wait().unwrap().success());drop(first);
    store::enqueue(&temp.0,json!({"kind":"Stop","session_id":"offline","host_id":"synthetic"})).unwrap();let third=worker(&temp.0);
    wait(||Store::open(&temp.0,true).and_then(|mut store|store.tasks()).is_ok_and(|tasks|tasks.first().is_some_and(|t|t["runtime"]=="idle")));
    assert_eq!(store::read_json(&temp.0.join("health.json")).unwrap()["implementation"],"rust");drop(third);
}
#[test]
fn malformed_hook_and_oversized_mcp_fail_open(){
    let temp=Temp::new();let hook=run("--hook",&temp.0,"[invalid");assert!(hook.status.success());assert_eq!(serde_json::from_slice::<Value>(&hook.stdout).unwrap(),json!({}));
    let input=format!("{}\n{{\"id\":9,\"method\":\"ping\"}}\n","x".repeat(1_000_010));let result=run("--mcp",&temp.0,&input);assert!(result.status.success());assert_eq!(serde_json::from_slice::<Value>(&result.stdout).unwrap()["id"],9);
}
#[test]
fn real_preflight_does_not_create_database(){let temp=Temp::new();floatboard_core::integration::preflight(&PathBuf::from(EXE),&temp.0).unwrap();assert!(!temp.0.join("events.sqlite3").exists());assert!(!temp.0.join("health.json").exists());}
