use super::*;
use std::{fs,io::Write};
use base64::{Engine,engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer,SigningKey};
use sha2::{Digest,Sha256};
const PROGRAMS:[&str;8]=["CodexFloatboard.exe","FloatboardAgent.exe","FloatboardHook.exe","FloatboardCtl.exe","FloatboardMaintenance.exe","FloatboardInstaller.exe","FloatboardUpdater.exe","FloatboardLauncher.exe"];
struct Fixture{root:PathBuf,profile:Profile,config:PathBuf}
impl Fixture{
 fn new()->Self{
  let root=floatboard_client::windows::local_home().unwrap().join("CodexFloatboard-Dev/installer-tests").join(uuid::Uuid::new_v4().to_string());fs::create_dir_all(root.join("data/codex")).unwrap();
  let bin=std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_path_buf();let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let port=listener.local_addr().unwrap().port();drop(listener);
  let previous=root.join("install/previous");fs::create_dir_all(&previous).unwrap();for name in PROGRAMS.iter().filter(|n|**n!="CodexFloatboard.exe"){fs::copy(bin.join(name),previous.join(name)).unwrap();}
  // GUI is verified by the independent Desktop suite. Installer tests never launch a window.
  fs::write(previous.join("CodexFloatboard.exe"),b"GUI fixture - never execute").unwrap();
  let profile=Profile{profile_id:uuid::Uuid::new_v4().to_string(),environment:"development".into(),owner_sid:floatboard_client::windows::sid().unwrap(),root:root.join("data"),port,agent_exe:previous.join("FloatboardAgent.exe"),gui_exe:previous.join("CodexFloatboard.exe"),codex_home:root.join("data/codex")};let config=root.join("registration.json");atomic_json(&config,&json!(profile)).unwrap();
  let init=Command::new(previous.join("FloatboardMaintenance.exe")).arg("initialize").arg("--profile").arg(&config).creation_flags(0x08000000).output().unwrap();assert!(init.status.success(),"{}",String::from_utf8_lossy(&init.stderr));
  let fixture=Self{root,profile,config};scheduled::register(&fixture.profile,&fixture.config).unwrap();scheduled::run(&fixture.profile,&fixture.config).unwrap();wait_health(&fixture.profile).unwrap();fixture
 }
 fn signed_candidate(&self)->Profile{
  let install=self.root.join("install");let archive=self.root.join("candidate.zip");let mut zip=zip::ZipWriter::new(fs::File::create(&archive).unwrap());let previous=self.profile.agent_exe.parent().unwrap();for name in PROGRAMS{zip.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();zip.write_all(&fs::read(previous.join(name)).unwrap()).unwrap();}zip.finish().unwrap();
  let bytes=fs::read(&archive).unwrap();let key=SigningKey::from_bytes(&[73u8;32]);let manifest=json!({"schema":2,"version":"2.0.0","platform":"windows-x86_64","minimum_version":"1.0.1","updater_protocol":2,"database_schema_min":2,"database_schema_max":2,"agent_protocol":2,"url":"https://github.com/Memento1990/codex-task/releases/download/v2.0.0/Floatboard-update-2.0.0-windows-x64.zip","size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))});
  let manifest_path=self.root.join("manifest.json");let signature_path=self.root.join("signature.sig");let bytes=serde_json::to_vec(&manifest).unwrap();fs::write(&manifest_path,&bytes).unwrap();fs::write(&signature_path,STANDARD.encode(key.sign(&bytes).to_bytes())).unwrap();
  let public=STANDARD.encode(key.verifying_key().to_bytes());crate::update_install::stage_with_key(&install,&archive,&manifest_path,&signature_path,&public).unwrap();
  let mut desired=self.profile.clone();desired.agent_exe=install.join("versions/2.0.0/FloatboardAgent.exe");desired.gui_exe=install.join("versions/2.0.0/CodexFloatboard.exe");desired
 }
}
impl Drop for Fixture{fn drop(&mut self){let current=Profile::load(&self.config).unwrap_or_else(|_|self.profile.clone());if std::thread::panicking(){eprintln!("installer evidence retained: {}",self.root.display());if let Ok(output)=Command::new("schtasks.exe").args(["/Query","/TN",&scheduled::name(&current),"/V","/FO","LIST"]).creation_flags(0x08000000).output(){let _=fs::write(self.root.join("task-failure.txt"),output.stdout);}}let _=Client::new(current.clone()).call("stop",json!({"confirmed":true}));let until=Instant::now()+Duration::from_secs(15);while Instant::now()<until{if let Ok(owner)=Instance::acquire(&self.profile.profile_id){drop(owner);break;}std::thread::sleep(Duration::from_millis(30));}let _=scheduled::remove_test(&current,&self.config);if !std::thread::panicking(){let _=fs::remove_dir_all(&self.root);}}}

#[test]fn signed_cutover_resumes_after_each_durable_phase_without_two_writers(){
 let fixture=Fixture::new();let desired=fixture.signed_candidate();let install=fixture.root.join("install");let journal_path=install.join("install-transaction.json");let old_db=Client::new(fixture.profile.clone()).call("identity",json!({})).unwrap()["database_instance_id"].clone();
 let mut journal=json!({"version":"2.0.0","config":fixture.config,"desired":desired,"before":fixture.profile,"phase":"prepared","test_interrupt_after":"quiescing"});atomic_json(&journal_path,&journal).unwrap();
 for phase in ["quiescing","backed_up","stopping","registered","healthy_paused"]{
  journal=read_json(&journal_path).unwrap();journal["test_interrupt_after"]=json!(phase);atomic_json(&journal_path,&journal).unwrap();let error=cutover(&install,&fixture.config,desired.clone(),"2.0.0",true).unwrap_err();assert!(error.contains("synthetic interrupted"),"{phase}: {error}");assert_eq!(read_json(&journal_path).unwrap()["phase"],phase);
 }
 journal=read_json(&journal_path).unwrap();journal.as_object_mut().unwrap().remove("test_interrupt_after");atomic_json(&journal_path,&journal).unwrap();let completed=cutover(&install,&fixture.config,desired.clone(),"2.0.0",true).unwrap();assert_eq!(completed["phase"],"healthy_paused");assert_eq!(completed["health"]["database_instance_id"],old_db);assert_eq!(completed["health"]["write_gate"],"paused");assert_eq!(completed["backup"]["integrity"],"ok");
 let client=Client::new(desired);assert!(client.call("hook_context",json!({"hook_event_name":"SessionStart","session_id":"must-not-write","cwd":"D:/synthetic"})).is_err());assert!(activate(&install,&fixture.config).unwrap_err().contains("GUI或OAuth"));assert_eq!(read_json(&install.join("current.json")).unwrap()["version"],"2.0.0");
}
#[test]fn mismatched_cutover_cannot_replace_an_unrelated_profile(){let f=Fixture::new();let mut unrelated=f.profile.clone();unrelated.profile_id=uuid::Uuid::new_v4().to_string();assert!(cutover(&f.root.join("install"),&f.config,unrelated,"2.0.0",true).unwrap_err().contains("不能更换profile"));}
