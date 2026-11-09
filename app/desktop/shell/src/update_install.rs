//! Signed, versioned Agent installation with explicit database ownership transfer.
use std::{fs,io::{Read,Write,Seek},path::{Path,PathBuf},process::{Command,Stdio},time::{Duration,Instant}};
use serde_json::{json,Value};
use std::os::windows::process::CommandExt;
use crate::{Result,file_lock::ProcessLock};
use floatboard_client::files::{atomic_json,read_json};
const MAX_EXPANDED:u64=300*1024*1024;
fn limited_file(path:&Path,limit:u64)->Result<Vec<u8>>{let f=fs::File::open(path).map_err(|_|"更新证明不可读")?;if f.metadata().map_err(|_|"更新证明信息不可读")?.len()>limit{return Err("更新证明超过大小限制".into());}let mut bytes=Vec::new();f.take(limit+1).read_to_end(&mut bytes).map_err(|_|"更新证明读取失败")?;if bytes.len() as u64>limit{return Err("更新证明过大".into());}Ok(bytes)}
fn checked(path:&Path)->Result<()>{crate::paths::check_path(path)}
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
 if root.join("update-journal.json").exists(){return Err("检测到旧更新事务；请使用专用迁移安装程序，未自动回退".into());}Ok(())
}
pub fn prepare(service:&crate::update::UpdateService,data_root:&Path)->Result<PathBuf>{
 let stage=service.begin_install()?;
 let exe=std::env::current_exe().map_err(|_|"应用位置不可用")?;let root=app_root(&exe)?;checked(&root)?;
 // Signed source evidence is rechecked independently by the child updater.
 let staging=crate::paths::control_root(data_root).join("updates/install");checked(&staging)?;fs::create_dir_all(&staging).map_err(|_|"更新暂存目录不可写")?;
 let helper=staging.join(format!("FloatboardUpdater-{}.exe",uuid::Uuid::new_v4()));let source=exe.parent().unwrap().join("FloatboardUpdater.exe");checked(&source)?;
 fs::copy(&source,&helper).map_err(|_|"更新器缺失，请完整下载新版")?;
 let request=staging.join(format!("request-{}.json",uuid::Uuid::new_v4()));
 atomic_json(&request,&json!({"root":root,"archive":stage.archive,"manifest":stage.manifest_path,"signature":stage.signature_path,"pid":std::process::id(),"data_root":data_root,"profile_config":floatboard_client::Profile::configured_path()? }))?;
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
 if zip.len()>14{return Err("更新包文件数异常".into());}let mut total=0u64;let mut seen=std::collections::HashSet::new();
 for i in 0..zip.len(){let mut entry=zip.by_index(i).map_err(|_|"更新包条目无效")?;let name=entry.name().to_owned();
 if !["CodexFloatboard.exe","FloatboardAgent.exe","FloatboardHook.exe","FloatboardCtl.exe","FloatboardMaintenance.exe","FloatboardInstaller.exe","FloatboardUpdater.exe","FloatboardLauncher.exe","LICENSE","THIRD-PARTY-NOTICES.txt","README.txt","migration-install.ps1","MIGRATION.md"].contains(&name.as_str())||!seen.insert(name.clone())||entry.is_dir()||entry.unix_mode().is_some_and(|m|m&0o170000==0o120000){return Err("更新包包含不允许的路径或条目".into());}
 total=total.checked_add(entry.size()).ok_or("更新包过大")?;if total>MAX_EXPANDED{return Err("解压体积超限".into());}
 let target=destination.join(name);checked(&target)?;let mut out=fs::OpenOptions::new().create_new(true).write(true).open(target).map_err(|_|"更新目标文件已存在或不可写")?;
 let count=std::io::copy(&mut entry.by_ref().take(MAX_EXPANDED+1),&mut out).map_err(|_|"更新包解压失败")?;if count!=entry.size(){return Err("更新包条目长度错误".into());}out.flush().map_err(|_|"更新文件保存失败")?;out.sync_all().map_err(|_|"更新文件确认失败")?;
 }
 for name in ["CodexFloatboard.exe","FloatboardAgent.exe","FloatboardHook.exe","FloatboardCtl.exe","FloatboardMaintenance.exe","FloatboardInstaller.exe","FloatboardUpdater.exe","FloatboardLauncher.exe"]{if !seen.contains(name){return Err("更新包缺少程序文件".into());}}Ok(())
}
pub fn stage_signed(root:&Path,archive:&Path,manifest_path:&Path,signature_path:&Path)->Result<crate::update::Manifest>{
 stage_with_key(root,archive,manifest_path,signature_path,option_env!("FLOATBOARD_UPDATE_PUBLIC_KEY").ok_or("构建缺少公钥")?)
}
pub(crate) fn stage_with_key(root:&Path,archive:&Path,manifest_path:&Path,signature_path:&Path,key:&str)->Result<crate::update::Manifest>{
 checked(root)?;let bytes=limited_file(manifest_path,65536)?;let signature=limited_file(signature_path,1024)?;
 let manifest=crate::update::verify_signed_manifest(&bytes,std::str::from_utf8(&signature).map_err(|_|"签名编码无效")?,key)?;
 use std::os::windows::fs::OpenOptionsExt;use sha2::{Digest,Sha256};
 let mut file=fs::OpenOptions::new().read(true).share_mode(1).open(archive).map_err(|_|"安装包不可读")?;
 if file.metadata().map_err(|_|"安装包信息不可读")?.len()!=manifest.size{return Err("安装包长度不符".into());}
 let mut digest=Sha256::new();let mut buffer=[0u8;65536];loop{let n=file.read(&mut buffer).map_err(|_|"安装包校验失败")?;if n==0{break;}digest.update(&buffer[..n]);}
 if format!("{:x}",digest.finalize())!=manifest.sha256.to_lowercase(){return Err("安装包哈希不符".into());}file.rewind().map_err(|_|"安装包重读失败")?;
 let destination=root.join("versions").join(&manifest.version);let temporary=root.join("versions").join(format!(".verified-{}",uuid::Uuid::new_v4()));extract_file(file,&temporary)?;
 if destination.exists(){for entry in fs::read_dir(&temporary).map_err(|_|"已验证目录不可读")?{let entry=entry.map_err(|_|"已验证文件不可读")?;if fs::read(destination.join(entry.file_name())).ok()!=fs::read(entry.path()).ok(){return Err("目标版本内容不符，禁止覆盖".into());}}if fs::read_dir(&destination).map_err(|_|"版本目录不可读")?.count()!=fs::read_dir(&temporary).map_err(|_|"暂存目录不可读")?.count(){return Err("目标版本含额外文件，禁止采用".into());}fs::remove_dir_all(&temporary).map_err(|_|"已验证暂存目录清理失败")?;}else{fs::rename(&temporary,&destination).map_err(|_|"已验证安装包不能提交")?;}
 let proof=root.join("package-proofs").join(&manifest.version);fs::create_dir_all(&proof).map_err(|_|"安装证明不可写")?;floatboard_client::files::atomic_bytes(&proof.join("manifest.json"),&bytes)?;floatboard_client::files::atomic_bytes(&proof.join("signature.sig"),&signature)?;
 atomic_json(&proof.join("source.json"),&json!({"archive":archive,"manifest":manifest_path,"signature":signature_path}))?;Ok(manifest)
}
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
 let profile_path=PathBuf::from(req["profile_config"].as_str().ok_or("profile登记路径缺失")?);
 let mut desired=floatboard_client::Profile::load(&profile_path)?;desired.agent_exe=destination.join("FloatboardAgent.exe");desired.gui_exe=destination.join("CodexFloatboard.exe");
 crate::install::cutover(&root,&profile_path,desired.clone(),version,true)?;
 Command::new(&desired.gui_exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"新看板未启动，后台保持暂停")?;
 let until=Instant::now()+Duration::from_secs(90);loop{
  let health=floatboard_client::Client::new(desired.clone()).call("identity",json!({}))?;
  if health["gui_ready"]==true&&health["authenticated_requests"].as_u64().unwrap_or(0)>0{crate::install::activate(&root,&profile_path)?;return Ok(());}
  if Instant::now()>=until{return Err("新后台已就绪，等待GUI与Codex重新连接验收；请继续专用安装事务，未回退旧库".into());}
  std::thread::sleep(Duration::from_millis(100));
 }
}

#[cfg(test)]mod tests{
use super::*;
fn temp()->PathBuf{let p=std::env::temp_dir().join(format!("floatboard-install-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&p).unwrap();p}
fn archive(path:&Path,names:&[&str]){let mut z=zip::ZipWriter::new(fs::File::create(path).unwrap());for name in names{z.start_file(*name,zip::write::SimpleFileOptions::default()).unwrap();z.write_all(b"fake executable fixture").unwrap();}z.finish().unwrap();}
#[test]fn version_paths_reject_escape(){for s in ["../x","1.0.0/evil","1.0.0-beta","1.0.0+build","01.2.3"]{assert!(version_name(s).is_err());}assert!(version_name("0.9.1").is_ok());}
#[test]fn legacy_layout_refuses_update(){assert!(app_root(Path::new("C:/portable/CodexFloatboard.exe")).is_err());}
#[test]fn archive_whitelist_and_complete_programs(){let p=temp();let zip=p.join("good.zip");archive(&zip,&["CodexFloatboard.exe","FloatboardAgent.exe","FloatboardHook.exe","FloatboardCtl.exe","FloatboardMaintenance.exe","FloatboardInstaller.exe","FloatboardUpdater.exe","FloatboardLauncher.exe","migration-install.ps1","MIGRATION.md"]);extract(&zip,&p.join("good")).unwrap();assert!(p.join("good/migration-install.ps1").is_file());assert!(p.join("good/MIGRATION.md").is_file());for (i,names) in [vec!["../escape.exe"],vec!["C:/escape.exe"],vec!["CodexFloatboard.exe"],vec!["nested/CodexFloatboard.exe"],vec!["unowned.ps1"]].iter().enumerate(){let zip=p.join(format!("bad{i}.zip"));archive(&zip,names);assert!(extract(&zip,&p.join(format!("bad{i}"))).is_err());}fs::remove_dir_all(p).unwrap();}
#[test]fn old_update_journal_never_triggers_automatic_rollback(){let p=temp();let current=json!({"version":"2.0.0"});atomic_json(&p.join("current.json"),&current).unwrap();atomic_json(&p.join("update-journal.json"),&json!({"before":{"version":"1.0.0"}})).unwrap();assert!(recover(&p).is_err());assert_eq!(read_json(&p.join("current.json")).unwrap(),current);fs::remove_dir_all(p).unwrap();}
}
