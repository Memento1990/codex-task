use std::{fs,path::Path,process::{Command,Stdio}};
use std::os::windows::process::CommandExt;
use crate::{Profile,Result,files::atomic_bytes};
use serde_json::json;
pub fn intent(profile:&Profile,running:bool)->Result<()>{crate::files::atomic_json(&profile.root.join("run-intent.json"),&json!({"profile_id":profile.profile_id,"login_id":crate::windows::login_id()?,"running":running}))}
pub fn should_run(profile:&Profile)->Result<bool>{
    let path=profile.root.join("run-intent.json");if !path.exists(){return Ok(true);}
    let value=crate::files::read_json(&path)?;if value["profile_id"]!=profile.profile_id||value["login_id"].as_str().is_none(){return Err("后台运行意图身份无效".into());}
    let running=value["running"].as_bool().ok_or("后台运行意图无效")?;Ok(value["login_id"]!=crate::windows::login_id()?||running)
}
fn first_tick()->Result<String>{unsafe{
    use windows_sys::Win32::{Foundation::{FILETIME,SYSTEMTIME},System::{SystemInformation::GetSystemTimeAsFileTime,Time::FileTimeToSystemTime}};
    let mut file:FILETIME=std::mem::zeroed();GetSystemTimeAsFileTime(&mut file);let ticks=((file.dwHighDateTime as u64)<<32|file.dwLowDateTime as u64)+600_000_000;
    let file=FILETIME{dwLowDateTime:ticks as u32,dwHighDateTime:(ticks>>32) as u32};let mut time:SYSTEMTIME=std::mem::zeroed();if FileTimeToSystemTime(&file,&mut time)==0{return Err("后台调度时间不可用".into());}
    Ok(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",time.wYear,time.wMonth,time.wDay,time.wHour,time.wMinute,time.wSecond))
}}
fn xml(value:&str)->String{value.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
pub fn name(profile:&Profile)->String{format!("CodexFloatboard-{}{}",if profile.environment=="production"{""}else{"Test-"},profile.profile_id)}
pub fn definition(profile:&Profile,config:&Path)->Result<String>{
    profile.validate()?;let sid=xml(&profile.owner_sid);let mut trigger=format!("<TimeTrigger><Repetition><Interval>PT1M</Interval></Repetition><StartBoundary>{}</StartBoundary><Enabled>true</Enabled></TimeTrigger>",first_tick()?);if profile.environment=="production"{trigger.push_str(&format!("<LogonTrigger><Enabled>true</Enabled><UserId>{sid}</UserId></LogonTrigger>"));}
    Ok(format!(r#"<?xml version="1.0" encoding="UTF-16"?><Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><RegistrationInfo><Description>Codex Floatboard profile {}</Description></RegistrationInfo><Triggers>{}</Triggers><Principals><Principal id="User"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals><Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit></Settings><Actions Context="User"><Exec><Command>{}</Command><Arguments>--scheduled --profile &quot;{}&quot;</Arguments></Exec></Actions></Task>"#,profile.profile_id,trigger,sid,xml(&profile.agent_exe.to_string_lossy()),xml(&config.to_string_lossy())))
}
fn command(args:&[&std::ffi::OsStr])->Result<String>{let result=Command::new("schtasks.exe").args(args).stdin(Stdio::null()).creation_flags(0x08000000).output().map_err(|_|"计划任务服务不可用")?;if !result.status.success(){return Err("计划任务操作失败；请检查当前用户任务权限".into());}if result.stdout.starts_with(&[255,254])||result.stdout.get(1)==Some(&0){let bytes=if result.stdout.starts_with(&[255,254]){&result.stdout[2..]}else{&result.stdout[..]};Ok(String::from_utf16_lossy(&bytes.chunks_exact(2).map(|b|u16::from_le_bytes([b[0],b[1]])).collect::<Vec<_>>()))}else{Ok(String::from_utf8_lossy(&result.stdout).into_owned())}}
pub fn verify(profile:&Profile,config:&Path)->Result<()>{
    let name=name(profile);let output=command(&["/Query".as_ref(),"/TN".as_ref(),name.as_ref(),"/XML".as_ref()])?;
    if !output.contains(&xml(&profile.owner_sid))||!output.contains(&format!("<Command>{}</Command>",xml(&profile.agent_exe.to_string_lossy())))||!output.contains(&xml(&config.to_string_lossy()))||output.contains("<LogonType>Password</LogonType>"){return Err("已登记的计划任务身份不匹配，未操作".into());}Ok(())
}
pub fn register(profile:&Profile,config:&Path)->Result<()>{
    register_inner(profile,config,verify(profile,config).is_ok())
}
pub fn replace(old:Option<&Profile>,profile:&Profile,config:&Path)->Result<()>{
    if verify(profile,config).is_ok(){return register_inner(profile,config,true);}
    if let Some(old)=old{verify(old,config)?;register_inner(profile,config,true)}else{register_inner(profile,config,false)}
}
fn register_inner(profile:&Profile,config:&Path,replace:bool)->Result<()>{
    let definition=definition(profile,config)?;let path=profile.root.join("agent-task.xml");let bytes=std::iter::once(0xfeffu16).chain(definition.encode_utf16()).flat_map(u16::to_le_bytes).collect::<Vec<_>>();atomic_bytes(&path,&bytes)?;
    let name=name(profile);let mut args=vec!["/Create".as_ref(),"/TN".as_ref(),name.as_ref(),"/XML".as_ref(),path.as_os_str()];if replace{args.push("/F".as_ref());}command(&args)?;verify(profile,config)
}
pub fn run(profile:&Profile,config:&Path)->Result<()>{verify(profile,config)?;intent(profile,true)?;command(&["/Run".as_ref(),"/TN".as_ref(),name(profile).as_ref()])?;Ok(())}
pub fn enabled(profile:&Profile,config:&Path,enabled:bool)->Result<()>{verify(profile,config)?;command(&["/Change".as_ref(),"/TN".as_ref(),name(profile).as_ref(),if enabled{"/ENABLE"}else{"/DISABLE"}.as_ref()])?;Ok(())}
pub fn remove_test(profile:&Profile,config:&Path)->Result<()>{if profile.environment=="production"{return Err("此入口仅清理测试任务".into());}verify(profile,config)?;command(&["/Delete".as_ref(),"/TN".as_ref(),name(profile).as_ref(),"/F".as_ref()])?;let _=fs::remove_file(profile.root.join("agent-task.xml"));Ok(())}

#[cfg(test)]mod tests{use super::*;
#[test]fn normal_stop_is_scoped_to_the_current_login_and_explicit_run_restores_intent(){let root=crate::windows::local_home().unwrap().join("CodexFloatboard-Dev/intent-unit").join(uuid::Uuid::new_v4().to_string());let p=Profile{profile_id:uuid::Uuid::new_v4().to_string(),environment:"development".into(),owner_sid:crate::windows::sid().unwrap(),root:root.clone(),port:45000,agent_exe:root.join("FloatboardAgent.exe"),gui_exe:root.join("CodexFloatboard.exe"),codex_home:root.join("codex")};assert!(should_run(&p).unwrap());intent(&p,false).unwrap();assert!(!should_run(&p).unwrap());let path=root.join("run-intent.json");let mut value=crate::files::read_json(&path).unwrap();value["login_id"]=json!("previous-login");crate::files::atomic_json(&path,&value).unwrap();assert!(should_run(&p).unwrap());intent(&p,true).unwrap();assert!(should_run(&p).unwrap());fs::remove_dir_all(root).unwrap();}
#[test]fn test_task_never_registers_logon_or_password(){let root=crate::windows::local_home().unwrap().join("CodexFloatboard-Dev/task-unit");let p=Profile{profile_id:uuid::Uuid::new_v4().to_string(),environment:"development".into(),owner_sid:crate::windows::sid().unwrap(),root:root.clone(),port:45000,agent_exe:root.join("FloatboardAgent.exe"),gui_exe:root.join("CodexFloatboard.exe"),codex_home:root.join("codex")};let xml=definition(&p,&root.join("profile.json")).unwrap();assert!(!xml.contains("<LogonTrigger>"));assert!(!xml.contains("<Password>"));assert!(xml.contains("InteractiveToken"));assert!(xml.contains("IgnoreNew"));assert!(xml.contains("<Repetition><Interval>PT1M</Interval>"));assert!(xml.contains("--scheduled --profile"));assert!(!xml.contains("RestartOnFailure"));assert!(name(&p).starts_with("CodexFloatboard-Test-"));}}
