//! Versioned application installation; never replaces a running Reporter.
use std::{fs,io::{Read,Write,Seek},path::{Path,PathBuf},process::{Command,Stdio},time::{Duration,Instant}};
use serde_json::{json,Value};
use std::os::windows::process::CommandExt;
use crate::{Result,store::{atomic_json,read_json},runtime::ProcessLock};
const MAX_EXPANDED:u64=300*1024*1024;
fn limited_file(path:&Path,limit:u64)->Result<Vec<u8>>{let f=fs::File::open(path).map_err(|_|"更新证明不可读")?;if f.metadata().map_err(|_|"更新证明信息不可读")?.len()>limit{return Err("更新证明超过大小限制".into());}let mut bytes=Vec::new();f.take(limit+1).read_to_end(&mut bytes).map_err(|_|"更新证明读取失败")?;if bytes.len() as u64>limit{return Err("更新证明过大".into());}Ok(bytes)}
fn checked(path:&Path)->Result<()>{crate::migration::check_path(path)}
fn app_root(exe:&Path)->Result<PathBuf>{
 let version=exe.parent().ok_or("应用位置无效")?;let versions=version.parent().ok_or("应用位置无效")?;
 if versions.file_name().and_then(|s|s.to_str())!=Some("versions"){return Err("此旧版便携目录不支持原地更新，请完整下载新版便携包一次".into());}
 let root=versions.parent().ok_or("应用位置无效")?.to_path_buf();checked(&root)?;Ok(root)
}
pub fn last_result()->Value{std::env::current_exe().ok().and_then(|p|app_root(&p).ok()).and_then(|root|read_json(&root.join("last-update.json")).ok()).unwrap_or(Value::Null)}
pub fn launch_entry(exe:&Path)->PathBuf{app_root(exe).ok().map(|root|root.join("CodexFloatboard.exe")).filter(|p|p.is_file()).unwrap_or_else(||exe.to_path_buf())}
fn version_name(value:&str)->Result<()> {let v=semver::Version::parse(value).map_err(|_|"更新版本号无效")?;if !v.pre.is_empty()||!v.build.is_empty()||v.to_string()!=value{return Err("只支持正式语义版本".into());}Ok(())}
fn version_exe(root:&Path,pointer:&Value)->Result<PathBuf>{let v=pointer["version"].as_str().ok_or("应用版本记录无效")?;version_name(v)?;let exe=root.join("versions").join(v).join("CodexFloatboard.exe");checked(&exe)?;if !exe.is_file(){return Err("应用版本文件缺失".into());}Ok(exe)}
pub fn launch()->Result<i32>{
 let exe=std::env::current_exe().map_err(|_|"启动位置不可用")?;let root=exe.parent().ok_or("启动位置不可用")?;checked(root)?;
 let _lock=ProcessLock::acquire(&root.join("update.lock")).map_err(|_|"更新正在进行，请稍后启动看板")?;
 recover(root)?;let target=version_exe(root,&read_json(&root.join("current.json"))?)?;
 Command::new(target).args(std::env::args().skip(1)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|_|"看板启动失败")?;Ok(0)
}
fn recover(root:&Path)->Result<()> {
 let path=root.join("update-journal.json");if !path.exists(){return Ok(());}checked(&path)?;let j=read_json(&path)?;
 let current=read_json(&root.join("current.json"))?;
 if current!=j["before"]&&current!=j["after"]{return Err("更新后应用入口发生外部变化，未覆盖".into());}
 let accepted=read_json(&root.join("update-health.json")).ok().is_some_and(|h|h["nonce"]==j["nonce"]&&h["version"]==j["after"]["version"]);
 if !accepted && j["new_pid"].as_u64().is_some_and(|pid|process_alive(pid as u32)){return Err("新版仍在运行且尚未确认，请正常关闭后再启动；未终止进程".into());}
 if !accepted{atomic_json(&root.join("current.json"),&j["before"])?;}
 atomic_json(&root.join("last-update.json"),&json!({"status":if accepted{"completed"}else{"rolled_back"},"version":j["after"]["version"],"at":crate::now()}))?;
 fs::remove_file(path).map_err(|_|"更新恢复记录不能确认")?;Ok(())
}
/// Called only after a successful renderer snapshot from the main application.
pub fn startup_gate()->Result<()> {
 if !std::env::args().any(|a|a=="--update-gate"){return Ok(());}
 let exe=std::env::current_exe().map_err(|_|"启动位置不可用")?;let root=app_root(&exe)?;let args:Vec<String>=std::env::args().collect();let nonce=args.iter().position(|a|a=="--update-health").and_then(|i|args.get(i+1)).ok_or("启动身份缺失")?;
 let until=Instant::now()+Duration::from_secs(15);
 while Instant::now()<until{if read_json(&root.join("update-journal.json")).ok().is_some_and(|j|j["nonce"]==*nonce&&j["new_pid"]==std::process::id()){return Ok(());}std::thread::sleep(Duration::from_millis(50));}
 Err("更新启动门闩未提交，未启动应用".into())
}
pub fn acknowledge()->Result<()> {
 let args:Vec<String>=std::env::args().collect();let Some(i)=args.iter().position(|a|a=="--update-health")else{return Ok(());};
 let nonce=args.get(i+1).ok_or("更新健康标记缺失")?;let exe=std::env::current_exe().map_err(|_|"应用位置不可用")?;let root=app_root(&exe)?;let j=read_json(&root.join("update-journal.json"))?;
 if j["nonce"]!=*nonce||j["after"]["version"]!=env!("CARGO_PKG_VERSION")||version_exe(&root,&j["after"])?!=exe{return Err("更新启动身份不匹配".into());}
 atomic_json(&root.join("update-health.json"),&json!({"nonce":nonce,"version":env!("CARGO_PKG_VERSION")}))
}
pub fn prepare(service:&crate::update::UpdateService,data_root:&Path)->Result<PathBuf>{
 let stage=service.begin_install()?;
 let exe=std::env::current_exe().map_err(|_|"应用位置不可用")?;let root=app_root(&exe)?;checked(&root)?;
 // Signed source evidence is rechecked independently by the child updater.
 let staging=crate::bridge::control_root(data_root).join("updates/install");checked(&staging)?;fs::create_dir_all(&staging).map_err(|_|"更新暂存目录不可写")?;
 let helper=staging.join(format!("FloatboardUpdater-{}.exe",uuid::Uuid::new_v4()));let source=exe.parent().unwrap().join("FloatboardUpdater.exe");checked(&source)?;
 fs::copy(&source,&helper).map_err(|_|"更新器缺失，请完整下载新版")?;
 let request=staging.join(format!("request-{}.json",uuid::Uuid::new_v4()));
 atomic_json(&request,&json!({"root":root,"archive":stage.archive,"manifest":stage.manifest_path,"signature":stage.signature_path,"pid":std::process::id(),"data_root":data_root,"args":std::env::args().skip(1).collect::<Vec<_>>() }))?;
 let mut child=Command::new(&helper).arg("--request").arg(&request).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"更新器启动失败")?;
 let deadline=Instant::now()+Duration::from_secs(30);
 while Instant::now()<deadline{
 if request.with_extension("ready").is_file(){return Ok(request);}
 if let Ok(error)=read_json(&request.with_extension("error")){return Err(error["error"].as_str().unwrap_or("更新准备失败").into());}
 if child.try_wait().ok().flatten().is_some(){return Err("更新器未完成准备，保留当前看板".into());}
 std::thread::sleep(Duration::from_millis(100));
 }
 atomic_json(&request.with_extension("cancel"),&json!({"cancelled":true}))?;Err("更新准备超时，保留当前看板".into())
}
#[cfg(test)]
fn extract(archive:&Path,destination:&Path)->Result<()> {extract_file(fs::File::open(archive).map_err(|_|"更新包不可读")?,destination)}
fn extract_file(file:fs::File,destination:&Path)->Result<()> {
 checked(destination)?;fs::create_dir_all(destination).map_err(|_|"应用目录不可写")?;
 let mut zip=zip::ZipArchive::new(file).map_err(|_|"更新包格式无效")?;
 if zip.len()>10{return Err("更新包文件数异常".into());}let mut total=0u64;let mut seen=std::collections::HashSet::new();
 for i in 0..zip.len(){let mut entry=zip.by_index(i).map_err(|_|"更新包条目无效")?;let name=entry.name().to_owned();
 if !["CodexFloatboard.exe","FloatboardReporter.exe","FloatboardUpdater.exe","LICENSE","THIRD-PARTY-NOTICES.txt","README.txt"].contains(&name.as_str())||!seen.insert(name.clone())||entry.is_dir()||entry.unix_mode().is_some_and(|m|m&0o170000==0o120000){return Err("更新包包含不允许的路径或条目".into());}
 total=total.checked_add(entry.size()).ok_or("更新包过大")?;if total>MAX_EXPANDED{return Err("解压体积超限".into());}
 let target=destination.join(name);checked(&target)?;let mut out=fs::OpenOptions::new().create_new(true).write(true).open(target).map_err(|_|"更新目标文件已存在或不可写")?;
 let count=std::io::copy(&mut entry.by_ref().take(MAX_EXPANDED+1),&mut out).map_err(|_|"更新包解压失败")?;if count!=entry.size(){return Err("更新包条目长度错误".into());}out.flush().map_err(|_|"更新文件保存失败")?;out.sync_all().map_err(|_|"更新文件确认失败")?;
 }
 for name in ["CodexFloatboard.exe","FloatboardReporter.exe","FloatboardUpdater.exe"]{if !seen.contains(name){return Err("更新包缺少程序文件".into());}}Ok(())
}
fn process_alive(pid:u32)->bool{use windows_sys::Win32::{Foundation::{CloseHandle,WAIT_TIMEOUT},System::Threading::{OpenProcess,WaitForSingleObject,PROCESS_SYNCHRONIZE}};unsafe{let h=OpenProcess(PROCESS_SYNCHRONIZE,0,pid);if h.is_null(){return false;}let live=WaitForSingleObject(h,0)==WAIT_TIMEOUT;CloseHandle(h);live}}
fn wait_pid(pid:u32,timeout:Duration)->Result<()> {
 use windows_sys::Win32::{Foundation::{CloseHandle,WAIT_OBJECT_0},System::Threading::{OpenProcess,WaitForSingleObject,PROCESS_SYNCHRONIZE}};
 unsafe{let handle=OpenProcess(PROCESS_SYNCHRONIZE,0,pid);if handle.is_null(){return Ok(());}let result=WaitForSingleObject(handle,timeout.as_millis() as u32);CloseHandle(handle);if result!=WAIT_OBJECT_0{return Err("旧看板尚未退出，取消更新；没有终止进程".into());}}Ok(())
}
pub fn apply_request(request:&Path)->Result<()> {
 checked(request)?;let req=read_json(request)?;let root=PathBuf::from(req["root"].as_str().ok_or("更新目标缺失")?);checked(&root)?;
 let _lock=ProcessLock::acquire(&root.join("update.lock"))?;recover(&root)?;
 let manifest_path=PathBuf::from(req["manifest"].as_str().ok_or("更新清单缺失")?);let signature=PathBuf::from(req["signature"].as_str().ok_or("更新签名缺失")?);checked(&manifest_path)?;checked(&signature)?;
 let manifest=crate::update::verify_manifest(&limited_file(&manifest_path,64*1024)?,&limited_file(&signature,1024)?)?;
 let value=serde_json::to_value(&manifest).map_err(|_|"更新清单无效")?;let version=value["version"].as_str().ok_or("更新版本缺失")?;version_name(version)?;
 let before=read_json(&root.join("current.json"))?;version_exe(&root,&before)?;
 if semver::Version::parse(version).map_err(|_|"版本无效")?<=semver::Version::parse(before["version"].as_str().ok_or("当前版本缺失")?).map_err(|_|"当前版本无效")?{return Err("拒绝降级或重复安装".into());}
 let archive=PathBuf::from(req["archive"].as_str().ok_or("更新包缺失")?);checked(&archive)?;
 use std::os::windows::fs::OpenOptionsExt;
 // Deny write/delete sharing while hashing and extracting the SAME open file.
 let mut archive_file=fs::OpenOptions::new().read(true).share_mode(1).open(&archive).map_err(|_|"更新包不可读或正在被修改")?;
 let size=value["size"].as_u64().ok_or("包大小缺失")?;
 if archive_file.metadata().map_err(|_|"更新包信息不可读")?.len()!=size{return Err("更新包大小不匹配".into());}
 use sha2::{Digest,Sha256};let mut digest=Sha256::new();let mut buffer=[0u8;65536];loop{let n=archive_file.read(&mut buffer).map_err(|_|"更新包校验失败")?;if n==0{break;}digest.update(&buffer[..n]);}
 if format!("{:x}",digest.finalize())!=value["sha256"].as_str().unwrap_or("").to_lowercase(){return Err("更新包校验失败".into());}archive_file.rewind().map_err(|_|"更新包重读失败")?;
 let destination=root.join("versions").join(version);checked(&destination)?;
 let staged=root.join("versions").join(format!(".staging-{}",uuid::Uuid::new_v4()));
 extract_file(archive_file,&staged)?;
 let pid=req["pid"].as_u64().filter(|p|*p>0&&*p<=u32::MAX as u64).ok_or("旧进程标识无效")? as u32;
 if request.with_extension("cancel").exists(){return Err("更新准备已取消".into());}
 atomic_json(&request.with_extension("ready"),&json!({"ready":true}))?;
 wait_pid(pid,Duration::from_secs(45))?;
 if request.with_extension("cancel").exists(){return Err("更新准备已取消".into());}
 // Rename only after the caller exits and cancellation has been ruled out.
 if destination.exists(){
 // A complete directory left by an interrupted/rolled-back attempt may be reused
 // only if every file matches the freshly extracted signed archive.
 for entry in fs::read_dir(&staged).map_err(|_|"暂存目录不可读")?{let entry=entry.map_err(|_|"暂存文件不可读")?;let existing=destination.join(entry.file_name());checked(&existing)?;if fs::read(existing).ok()!=fs::read(entry.path()).ok(){return Err("目标版本目录已有不同内容，未覆盖".into());}}
 if fs::read_dir(&destination).map_err(|_|"目标目录不可读")?.count()!=fs::read_dir(&staged).map_err(|_|"暂存目录不可读")?.count(){return Err("目标版本目录包含陌生文件，未覆盖".into());}
 }else{fs::rename(&staged,&destination).map_err(|_|"版本目录提交失败，未切换应用入口")?;}
 let nonce=uuid::Uuid::new_v4().to_string();let after=json!({"version":version});atomic_json(&root.join("update-journal.json"),&json!({"before":before,"after":after,"nonce":nonce}))?;atomic_json(&root.join("current.json"),&after)?;
 let target=version_exe(&root,&after)?;let data=req["data_root"].as_str().ok_or("任务数据目录缺失")?;
 let mut command=Command::new(target);command.args(["--data-dir",data,"--update-health",&nonce,"--update-gate"]);
 if req["args"].as_array().is_some_and(|a|a.iter().any(|v|v=="--isolated-instance")){command.arg("--isolated-instance");}
 command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
 let mut child=match command.spawn(){Ok(c)=>c,Err(_)=>{recover(&root)?;return Err("新版无法启动，已恢复旧入口".into());}};
 let mut journal=read_json(&root.join("update-journal.json"))?;journal["new_pid"]=json!(child.id());atomic_json(&root.join("update-journal.json"),&journal)?;
 let deadline=Instant::now()+Duration::from_secs(45);let mut healthy=false;
 while Instant::now()<deadline{if read_json(&root.join("update-health.json")).ok().is_some_and(|h|h["nonce"]==nonce&&h["version"]==version){healthy=true;break;}if child.try_wait().ok().flatten().is_some(){break;}std::thread::sleep(Duration::from_millis(200));}
 if !healthy && child.try_wait().ok().flatten().is_none(){
 atomic_json(&root.join("last-update.json"),&json!({"status":"awaiting_health","version":version,"at":crate::now(),"message":"新版仍运行但未确认；请正常关闭看板再启动以恢复旧入口"}))?;
 return Err("新版仍运行但未完成启动确认；未强制结束它。请正常关闭看板后重新启动以恢复旧入口".into());
 }
 recover(&root)?;
 if !healthy{
 // Only restart the old GUI if the new process has actually exited. Never kill it.
 if child.try_wait().ok().flatten().is_some(){let old=version_exe(&root,&before)?;let _=Command::new(old).args(["--data-dir",data]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();}
 return Err("新版未完成启动确认，已恢复旧入口；未强制结束任何进程，请重新启动看板".into());}Ok(())
}
#[cfg(test)]mod tests{
use super::*;
fn temp()->PathBuf{let p=std::env::temp_dir().join(format!("floatboard-install-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&p).unwrap();p}
fn archive(path:&Path,names:&[&str]){let mut z=zip::ZipWriter::new(fs::File::create(path).unwrap());for name in names{z.start_file(*name,zip::write::SimpleFileOptions::default()).unwrap();z.write_all(b"fake executable fixture").unwrap();}z.finish().unwrap();}
#[test]fn version_paths_reject_escape(){for s in ["../x","1.0.0/evil","1.0.0-beta","1.0.0+build","01.2.3"]{assert!(version_name(s).is_err());}assert!(version_name("0.9.1").is_ok());}
#[test]fn legacy_layout_refuses_update(){assert!(app_root(Path::new("C:/portable/CodexFloatboard.exe")).is_err());}
#[test]fn archive_whitelist_and_complete_programs(){let p=temp();let zip=p.join("good.zip");archive(&zip,&["CodexFloatboard.exe","FloatboardReporter.exe","FloatboardUpdater.exe"]);extract(&zip,&p.join("good")).unwrap();for (i,names) in [vec!["../escape.exe"],vec!["C:/escape.exe"],vec!["CodexFloatboard.exe"],vec!["nested/CodexFloatboard.exe"]].iter().enumerate(){let zip=p.join(format!("bad{i}.zip"));archive(&zip,names);assert!(extract(&zip,&p.join(format!("bad{i}"))).is_err());}fs::remove_dir_all(p).unwrap();}
#[test]fn recovery_rolls_back_only_its_own_pointer(){let p=temp();let before=json!({"version":"0.9.0"});let after=json!({"version":"0.9.1"});let journal=json!({"before":before,"after":after,"nonce":"fixture"});atomic_json(&p.join("current.json"),&after).unwrap();atomic_json(&p.join("update-journal.json"),&journal).unwrap();recover(&p).unwrap();assert_eq!(read_json(&p.join("current.json")).unwrap(),before);
atomic_json(&p.join("current.json"),&json!({"version":"0.9.2"})).unwrap();atomic_json(&p.join("update-journal.json"),&journal).unwrap();assert!(recover(&p).is_err());assert_eq!(read_json(&p.join("current.json")).unwrap()["version"],"0.9.2");fs::remove_dir_all(p).unwrap();}
#[test]fn recovery_commits_only_matching_health(){let p=temp();let after=json!({"version":"0.9.1"});atomic_json(&p.join("current.json"),&after).unwrap();atomic_json(&p.join("update-journal.json"),&json!({"before":{"version":"0.9.0"},"after":after,"nonce":"fixture"})).unwrap();atomic_json(&p.join("update-health.json"),&json!({"version":"0.9.1","nonce":"fixture"})).unwrap();recover(&p).unwrap();assert_eq!(read_json(&p.join("current.json")).unwrap(),after);assert!(!p.join("update-journal.json").exists());fs::remove_dir_all(p).unwrap();}
}
