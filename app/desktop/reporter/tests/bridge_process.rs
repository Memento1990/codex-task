use std::{fs,path::PathBuf,process::{Command,Stdio},io::Write};
use serde_json::Value;
struct Sandbox(PathBuf);
impl Sandbox{fn new()->Self{let root=std::env::temp_dir().join(format!("floatboard-bridge-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();Self(root)}fn data(&self)->PathBuf{self.0.join("CodexFloatboard/data")}fn control(&self)->PathBuf{self.0.join("CodexFloatboard/control")}fn command(&self,exe:&std::path::Path)->Command{let mut c=Command::new(exe);c.env("LOCALAPPDATA",&self.0).env("CODEX_HOME",self.0.join("codex")).env("FLOATBOARD_PREFLIGHT","1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());c}}
impl Drop for Sandbox{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
fn call(mut c:Command,input:&str)->std::process::Output{let mut p=c.spawn().unwrap();p.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();p.wait_with_output().unwrap()}
#[test]fn stable_bridge_installs_roundtrips_and_refuses_tampered_payload(){
 let s=Sandbox::new();let source=std::path::Path::new(env!("CARGO_BIN_EXE_FloatboardReporter"));
 let mut c=s.command(source);c.arg("--connect");let result=call(c,"");assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stdout));
 let receipt:Value=serde_json::from_slice(&fs::read(s.control().join("integration.json")).unwrap()).unwrap();assert_eq!(receipt["status"],"applied");assert!(receipt["generation"].is_string());
 let base=s.control().join("runtime");let bridge=base.join("FloatboardBridge.exe");assert!(bridge.is_file());
 let config=fs::read_to_string(s.0.join("codex/config.toml")).unwrap();assert!(config.contains("mcp_tool"));assert!(!config.contains("--data-dir"));
 let input=concat!("{\"id\":1,\"method\":\"initialize\"}\n","{\"id\":2,\"method\":\"tools/list\"}\n");
 let mut c=s.command(&bridge);c.arg("--mcp");let output=call(c,input);assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));let lines:Vec<Value>=String::from_utf8_lossy(&output.stdout).lines().map(|l|serde_json::from_str(l).unwrap()).collect();assert_eq!(lines[0]["id"],1);assert!(lines[1]["result"]["tools"].as_array().unwrap().iter().any(|v|v["name"]=="hook_context"));
 // Exercise real Rust Hook -> authenticated progress -> DB consumption, without a worker.
 let request=serde_json::json!({"id":3,"method":"tools/call","params":{"name":"hook_context","arguments":{"hook_event_name":"SessionStart","session_id":"isolated-real-session","cwd":"C:/isolated"}}}).to_string()+"\n";
 let mut c=s.command(&bridge);c.env_remove("FLOATBOARD_PREFLIGHT").env("FLOATBOARD_NO_WORKER","1").arg("--mcp");let output=call(c,&(String::from("{\"id\":1,\"method\":\"initialize\"}\n")+&request));assert!(output.status.success());
 let binding:Value=serde_json::from_slice(&fs::read(floatboard_core::store::session_path(&s.data(),"isolated-real-session")).unwrap()).unwrap();
 let args=serde_json::json!({"session_id":"isolated-real-session","session_token":binding["session_token"],"revision":0,"title":"integration probe","status":"running","plan":[],"blockers":[],"evidence":[]});
 let request=serde_json::json!({"id":4,"method":"tools/call","params":{"name":"report_progress","arguments":args}}).to_string()+"\n";
 let mut c=s.command(&bridge);c.env_remove("FLOATBOARD_PREFLIGHT").env("FLOATBOARD_NO_WORKER","1").arg("--mcp");let output=call(c,&request);assert!(output.status.success());let response:Value=serde_json::from_slice(&output.stdout).unwrap();assert_eq!(response["result"]["isError"],false);
 let mut db=floatboard_core::store::Store::open(&s.data(),false).unwrap();for file in fs::read_dir(s.data().join("inbox")).unwrap(){let event:Value=serde_json::from_slice(&fs::read(file.unwrap().path()).unwrap()).unwrap();db.ingest(&event).unwrap();}drop(db);
 // Simulates a legacy worker: no new consumed-health file, only committed DB evidence.
 assert!(!s.data().join("connection-consumed.json").exists());
 let old_local=std::env::var_os("LOCALAPPDATA");std::env::set_var("LOCALAPPDATA",&s.0);
 let status=floatboard_core::bridge::decorate_connection(&s.data(),serde_json::json!({"state":"configured"}));
 if let Some(old)=old_local{std::env::set_var("LOCALAPPDATA",old);}else{std::env::remove_var("LOCALAPPDATA");}
 assert_eq!(status["state"],"sync_verified");
 let before=fs::read(base.join("active.json")).unwrap();let active:Value=serde_json::from_slice(&before).unwrap();let payload=base.join(active["payload"].as_str().unwrap());fs::write(&payload,b"tampered").unwrap();let mut c=s.command(&bridge);c.arg("--mcp");let output=call(c,input);assert!(!output.status.success());assert!(output.stdout.is_empty());assert_eq!(fs::read(base.join("active.json")).unwrap(),before);
 // Reconnecting cannot accept an unknown launcher or replace its bytes.
 fs::write(&bridge,b"foreign launcher").unwrap();let mut c=s.command(source);c.arg("--connect");let output=call(c,"");assert!(!output.status.success());assert_eq!(fs::read(&bridge).unwrap(),b"foreign launcher");
 assert!(s.data().join("events.sqlite3").exists());
}
