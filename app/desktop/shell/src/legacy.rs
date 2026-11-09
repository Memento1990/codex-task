//! Offline cutover inventory. This module never terminates a legacy process.
use std::{path::{Path,PathBuf},time::{Duration,Instant}};
use serde_json::{json,Value};
use crate::Result;
use floatboard_client::{files::read_json,windows::{Handle,process_image}};

pub fn inventory(roots:&[PathBuf])->Result<Value>{
 for root in roots{crate::paths::check_path(root)?;}
 use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE,System::Diagnostics::ToolHelp::*};
 let raw=unsafe{CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0)};if raw==INVALID_HANDLE_VALUE{return Err("旧实例进程清单不可读".into());}let snapshot=Handle(raw);let mut entry:PROCESSENTRY32W=unsafe{std::mem::zeroed()};entry.dwSize=std::mem::size_of::<PROCESSENTRY32W>() as u32;
 let mut found=Vec::new();let mut more=unsafe{Process32FirstW(snapshot.0,&mut entry)};
 while more!=0{if let Ok(image)=process_image(entry.th32ProcessID){let lower=image.to_string_lossy().to_lowercase();if roots.iter().any(|root|lower.starts_with(&(root.to_string_lossy().to_lowercase().replace('/',"\\").trim_end_matches(['\\','/']).to_owned()+"\\"))){found.push(json!({"pid":entry.th32ProcessID,"image":image}));}}more=unsafe{Process32NextW(snapshot.0,&mut entry)};}
 Ok(json!({"roots":roots,"processes":found,"normal_exit_required":!found.is_empty(),"processes_terminated":0}))
}
pub fn wait_normal_exit(roots:&[PathBuf],timeout:Duration)->Result<Value>{let until=Instant::now()+timeout;loop{let value=inventory(roots)?;if value["processes"].as_array().is_some_and(|v|v.is_empty()){return Ok(value);}if Instant::now()>=until{return Err("旧会话或写入者尚未正常退出，未迁移、未切换、未结束进程".into());}std::thread::sleep(Duration::from_millis(200));}}
pub fn verify_sources(validation:&Value)->Result<()>{
 use sha2::{Digest,Sha256};
 for item in validation["source_files"].as_array().ok_or("迁移来源文件清单缺失")?{let main=item["source"]["database"].as_str().ok_or("迁移来源路径缺失")?;for suffix in ["","-wal","-shm"]{let path=PathBuf::from(format!("{main}{suffix}"));let expected=&item["files"][suffix];if expected.is_null(){if path.exists(){return Err("迁移来源新增文件，必须重新取得最终稳定副本".into());}}else{let bytes=std::fs::read(path).map_err(|_|"迁移来源文件缺失或不可读")?;if expected["sha256"]!=format!("{:x}",Sha256::digest(&bytes)){return Err("迁移来源发生变化，必须重新预览和核对最终增量".into());}}}}
 for item in validation["state_sources"].as_array().into_iter().flatten(){let path=item["source"]["path"].as_str().ok_or("迁移设置来源路径缺失")?;let bytes=std::fs::read(path).map_err(|_|"迁移设置来源文件缺失")?;if item["sha256"]!=format!("{:x}",Sha256::digest(&bytes)){return Err("迁移设置来源发生变化，必须重新预览".into());}}
 Ok(())
}
pub fn roots(path:&Path)->Result<Vec<PathBuf>>{serde_json::from_value(read_json(path)?).map_err(|_|"旧程序根目录清单无效".into())}

fn startup_key(profile:&floatboard_client::Profile)->String{if profile.environment=="production"{r"Software\Microsoft\Windows\CurrentVersion\Run".into()}else{format!(r"Software\CodexFloatboardTests\{}\Run",profile.profile_id)}}
fn startup_at(key:&str,roots:&[PathBuf])->Result<Value>{
 use winreg::{RegKey,enums::{HKEY_CURRENT_USER,KEY_READ}};
 let registry=match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(key,KEY_READ){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Value::Null),Err(_)=>return Err("旧自启项不可读".into())};
 let command:String=match registry.get_value("CodexFloatboard"){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Value::Null),Err(_)=>return Err("旧自启项格式无效".into())};
 let trimmed=command.trim();let executable=if let Some(rest)=trimmed.strip_prefix('"'){rest.split('"').next()}else{trimmed.split_whitespace().next()}.ok_or("旧自启程序无效")?;
 let image=PathBuf::from(executable);let product=image.file_name().is_some_and(|v|v.to_string_lossy().eq_ignore_ascii_case("CodexFloatboard.exe")||v.to_string_lossy().eq_ignore_ascii_case("FloatboardLauncher.exe"));
 if !product||!roots.iter().any(|root|image.starts_with(root)){return Err("旧自启项归属不明确，未采用或删除".into());}
 Ok(json!({"key":key,"name":"CodexFloatboard","command":command}))
}
pub fn startup_preview(profile:&floatboard_client::Profile,roots:&[PathBuf])->Result<Value>{startup_at(&startup_key(profile),roots)}
pub fn remove_startup(profile:&floatboard_client::Profile,roots:&[PathBuf],expected:&Value)->Result<Value>{
 let current=startup_preview(profile,roots)?;
 if current.is_null(){return Ok(json!({"removed":false}));}if current!=*expected{return Err("旧自启项已变化，必须重新预览".into());}
 use winreg::{RegKey,enums::{HKEY_CURRENT_USER,KEY_SET_VALUE}};
 let registry=RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(startup_key(profile),KEY_SET_VALUE).map_err(|_|"旧自启项不可写")?;
 registry.delete_value("CodexFloatboard").map_err(|_|"旧自启项移除失败")?;Ok(json!({"removed":true,"before":current}))
}

#[cfg(test)]mod tests{use super::*;
#[test]fn startup_cleanup_is_exact_and_preserves_other_entries(){
 use winreg::{RegKey,enums::HKEY_CURRENT_USER};let root=std::env::temp_dir().join(format!("floatboard-startup-{}",uuid::Uuid::new_v4()));let profile=floatboard_client::Profile{profile_id:uuid::Uuid::new_v4().to_string(),environment:"development".into(),owner_sid:floatboard_client::windows::sid().unwrap(),root:root.clone(),port:59001,agent_exe:root.join("FloatboardAgent.exe"),gui_exe:root.join("CodexFloatboard.exe"),codex_home:root.join("codex")};let key=startup_key(&profile);let user=RegKey::predef(HKEY_CURRENT_USER);let(registry,_)=user.create_subkey(&key).unwrap();let command=format!("\"{}\"",profile.gui_exe.display());registry.set_value("CodexFloatboard",&command).unwrap();registry.set_value("OtherApp",&"untouched.exe").unwrap();let roots=[root];let preview=startup_preview(&profile,&roots).unwrap();assert!(remove_startup(&profile,&roots,&json!({})).is_err());remove_startup(&profile,&roots,&preview).unwrap();assert!(startup_preview(&profile,&roots).unwrap().is_null());assert_eq!(registry.get_value::<String,_>("OtherApp").unwrap(),"untouched.exe");drop(registry);user.delete_subkey_all(format!(r"Software\CodexFloatboardTests\{}",profile.profile_id)).unwrap();
}}
