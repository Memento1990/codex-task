//! Independent repair executor, hosted by the version-matched Reporter binary.
//! Never terminates a process and never rolls a committed database back.
use std::{fs, path::{Path,PathBuf}, process::{Command,Stdio}, time::{Duration,Instant}};
use std::os::windows::process::CommandExt;
use serde_json::{json,Value};
use crate::{Result, store::{atomic_json,read_json}};

fn base(root:&Path)->Result<PathBuf>{
    crate::migration::check_path(root)?;
    let base=root.parent().ok_or("修复目录无效")?.join("control/database-restart");
    crate::migration::check_path(&base)?;fs::create_dir_all(&base).map_err(|_|"修复记录无法保存")?;Ok(base)
}
fn result(root:&Path,mut value:Value)->Result<()>{if let Some(record)=value.as_object_mut(){record.remove("last_attempt");}atomic_json(&base(root)?.join("latest.json"),&value)}
pub fn status(root:&Path)->Result<Value>{
    let path=base(root)?.join("latest.json");crate::migration::check_path(&path)?;
    let mut record=if path.exists(){limited(&path)?}else{json!({"status":"not_started"})};
    if record["status"]=="preparing"{
        if let (Some(source),Some(destination))=(record["source"].as_str(),record["destination"].as_str()){
            if crate::migration::resolve_root(Path::new(source))?==Path::new(destination){record["status"]=json!("awaiting_health");record["health_deadline"]=json!(crate::now()+60.);result(root,record.clone())?;}
            else if record["at"].as_f64().is_some_and(|at|crate::now()-at>90.) && crate::runtime::ProcessLock::acquire(&base(root)?.join("executor.lock")).is_ok(){record["status"]=json!("failed");result(root,record.clone())?;}
        }
    }
    if record["status"]=="awaiting_health" && record["health_deadline"].as_f64().is_some_and(|at|crate::now()>at) && crate::runtime::ProcessLock::acquire(&base(root)?.join("executor.lock")).is_ok(){record["status"]=json!("health_failed");result(root,record.clone())?;}
    let attempt=base(root)?.join("last-attempt.json");
    if let Ok(attempt)=limited(&attempt){if attempt["at"].as_f64().unwrap_or(0.)>record["at"].as_f64().unwrap_or(0.){record["last_attempt"]=attempt;}}
    Ok(record)
}
fn limited(path:&Path)->Result<Value>{
    crate::migration::check_path(path)?;
    if fs::metadata(path).map_err(|_|"修复请求不可读")?.len()>16384{return Err("修复请求过大".into());}read_json(path)
}
struct Parent(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Parent{fn drop(&mut self){unsafe{windows_sys::Win32::Foundation::CloseHandle(self.0);}}}
impl Parent{
    fn pin(pid:u32,expected:&Path)->Result<Self>{
        use windows_sys::Win32::System::Threading::{OpenProcess,QueryFullProcessImageNameW,PROCESS_SYNCHRONIZE,PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe{
            let handle=OpenProcess(PROCESS_SYNCHRONIZE|PROCESS_QUERY_LIMITED_INFORMATION,0,pid);
            if handle.is_null(){return Err("无法核对待重启看板，未执行修复".into());}
            let parent=Self(handle);let mut buf=vec![0u16;32768];let mut len=buf.len() as u32;
            if QueryFullProcessImageNameW(handle,0,buf.as_mut_ptr(),&mut len)==0{return Err("无法核对看板身份".into());}
            let actual=PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
            if fs::canonicalize(actual).ok()!=fs::canonicalize(expected).ok(){return Err("看板进程身份不匹配".into());}Ok(parent)
        }
    }
    fn wait(&self)->Result<()>{
        use windows_sys::Win32::{Foundation::WAIT_OBJECT_0,System::Threading::WaitForSingleObject};
        if unsafe{WaitForSingleObject(self.0,45000)}!=WAIT_OBJECT_0{return Err("看板尚未正常退出，未切换数据".into());}Ok(())
    }
}
// These reasons are rendered as escaped text by the settings UI, never HTML.
// Bound persisted lower-layer errors without slicing a UTF-8 code point.
fn failure_reason(error:&str)->String{
    error.chars().filter(|c|!c.is_control()).take(240).collect()
}
fn failure_guidance(error:&str)->&'static str{
    if error.contains("采集器不支持安全修复") || error.contains("旧采集器未确认安全交接") || error.contains("file_identity") {
        "旧版采集器无法确认安全交接，关闭会话也可能仍有后台驻留。请先保存工作并正常重启 Windows，再打开看板重试。不要强行结束 Codex 或上报进程。"
    }else{""}
}
fn failure_message(stage:&str,error:&str)->String{
    let reason=failure_reason(error);let guidance=failure_guidance(error);
    format!("修复{stage}未完成，尚未切换数据。原因：{reason}。{guidance}原数据库和日志已保留。")
}
/// Caller has already gated Engine and drained foreground notifications/SQLite.
pub fn prepare(root:&Path)->Result<Value>{
    let outcome=prepare_inner(root);
    if let Err(error)=&outcome {
        // Preparation may fail before a helper/request exists. Persist that
        // stage instead of reverting the settings page to "not started".
        let message=failure_message("准备",error);
        // A new failed attempt is separate from the durable prior operation.
        // Keep its identity/commit facts, but never hide the latest reason.
        let attempt=json!({"status":"failed","stage":"prepare","reason":failure_reason(error),"message":format!("本次准备未完成：{}。现有数据位置与此前事务保持不变。{}",failure_reason(error),failure_guidance(error)),"at":crate::now()});
        if let Ok(base)=base(root){let _=atomic_json(&base.join("last-attempt.json"),&attempt);}
        // Never erase a prior helper/committed operation or claim that an
        // already redirected source was not switched. This record is only for
        // errors before an operation has established its lifecycle identity.
        let prior=base(root).ok().and_then(|p|{let p=p.join("latest.json");if p.exists(){limited(&p).ok()}else{Some(json!({"status":"not_started"}))}});
        let can_record=prior.as_ref().is_some_and(|v|matches!(v["status"].as_str(),Some("not_started"|"failed"|"blocked")))
            && crate::migration::resolve_root(root).is_ok_and(|current|current==root);
        if can_record {let _=result(root,json!({"status":"failed","stage":"prepare","reason":failure_reason(error),"message":message,"at":crate::now()}));}
    }
    outcome
}
fn prepare_inner(root:&Path)->Result<Value>{
    let candidate=crate::repair_transaction::prepare(root)?;
    let id=candidate["id"].as_str().or_else(||candidate["operation_id"].as_str()).ok_or("修复准备缺少操作身份")?;
    let exe=std::env::current_exe().map_err(|_|"看板路径不可用")?;
    crate::migration::check_path(&exe)?;
    let helper=crate::runtime::reporter_path()?;crate::migration::check_path(&helper)?;
    let nonce=uuid::Uuid::new_v4().to_string();let request=base(root)?.join(format!("request-{nonce}.json"));
    atomic_json(&request,&json!({"version":1,"root":root,"id":id,"nonce":nonce,"pid":std::process::id(),"exe":exe,"isolated":std::env::args().any(|a|a=="--isolated-instance")}))?;
    result(root,json!({"status":"preparing","nonce":nonce,"at":crate::now(),"source":root,"destination":candidate["destination"],"recovered_tasks":candidate["recovered_tasks"],"recovery_mode":candidate["recovery_mode"],"history_recovered":candidate["history_recovered"],"timing_recovered":candidate["timing_recovered"]}))?;
    let mut child=match Command::new(helper).arg("--repair-execute").arg(&request).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn(){
        Ok(child)=>child,Err(_)=>{let _=result(root,json!({"status":"failed","nonce":nonce}));return Err("修复执行器无法启动，保留当前看板".into());}
    };
    let until=Instant::now()+Duration::from_secs(20);
    while Instant::now()<until{
        if limited(&request.with_extension("ready")).is_ok_and(|v|v["nonce"]==nonce){return Ok(json!({"status":"ready","recovered_tasks":candidate["recovered_tasks"],"recovery_mode":candidate["recovery_mode"],"history_recovered":candidate["history_recovered"],"timing_recovered":candidate["timing_recovered"]}));}
        if request.with_extension("error").exists()||child.try_wait().map_err(|_|"修复执行器状态不可用")?.is_some(){return Err("修复执行器未准备好，原数据未替换".into());}
        std::thread::sleep(Duration::from_millis(50));
    }
    atomic_json(&request.with_extension("cancel"),&json!({"nonce":nonce}))?;
    Err("修复准备超时，原数据未替换".into())
}

pub fn execute(request:&Path)->Result<()>{
    let outcome=execute_inner(request);
    if let Err(error)=&outcome{
        if let Ok(req)=limited(request){
            if let (Some(root),Some(nonce))=(req["root"].as_str(),req["nonce"].as_str()){
                if uuid::Uuid::parse_str(nonce).is_ok() && base(Path::new(root)).is_ok_and(|p|request==p.join(format!("request-{nonce}.json"))){
                    let _=atomic_json(&request.with_extension("error"),&json!({"nonce":nonce}));
                    // Do not overwrite a committed result when restart alone failed.
                    if status(Path::new(root)).is_ok_and(|v|v["status"]=="preparing"&&v["nonce"]==nonce){let _=result(Path::new(root),json!({"status":"failed","stage":"executor","reason":failure_reason(error),"nonce":nonce,"message":failure_message("执行",error)}));}
                }
            }
        }
    }
    outcome
}
fn execute_inner(request:&Path)->Result<()>{
    let req=limited(request)?;let root=PathBuf::from(req["root"].as_str().ok_or("修复数据位置缺失")?);
    let nonce=req["nonce"].as_str().filter(|s|uuid::Uuid::parse_str(s).is_ok()).ok_or("修复身份无效")?;
    if request!=base(&root)?.join(format!("request-{nonce}.json")){return Err("修复请求位置无效".into());}
    let exe=PathBuf::from(req["exe"].as_str().ok_or("看板位置缺失")?);crate::migration::check_path(&exe)?;
    let self_exe=std::env::current_exe().map_err(|_|"执行器位置无效")?;
    if exe.file_name().and_then(|s|s.to_str())!=Some("CodexFloatboard.exe")||fs::canonicalize(exe.parent().ok_or("看板目录无效")?).ok()!=fs::canonicalize(self_exe.parent().ok_or("执行器目录无效")?).ok(){return Err("修复执行器和看板版本目录不一致".into());}
    let pid=req["pid"].as_u64().filter(|n|*n>0&&*n<=u32::MAX as u64).ok_or("看板身份无效")? as u32;
    let parent=Parent::pin(pid,&exe)?;
    let id=req["id"].as_str().ok_or("修复操作缺失")?;
    let _executor=crate::runtime::ProcessLock::acquire(&base(&root)?.join("executor.lock"))?;
    if request.with_extension("cancel").exists(){return Err("修复已取消".into());}
    atomic_json(&request.with_extension("ready"),&json!({"nonce":nonce}))?;
    parent.wait()?;
    if request.with_extension("cancel").exists(){return Err("修复已取消".into());}
    let applied=crate::repair_transaction::apply(&root,id);
    // A durable redirect is authoritative even if a later journal write failed.
    let transaction=crate::repair_transaction::status(&root).unwrap_or(Value::Null);
    let destination=crate::migration::resolve_root(&root).unwrap_or_else(|_|root.clone());
    let committed=destination!=root || transaction["phase"]=="committed";
    let tasks=applied.as_ref().ok().map(|v|v["recovered_tasks"].clone()).unwrap_or_else(||transaction["recovered_tasks"].clone());
    let error=applied.as_ref().err().map(String::as_str).unwrap_or("修复事务尚未提交");
    let message=if committed{"数据已切换，正在确认看板和后续上报是否正常。".to_owned()}else{failure_message("应用",error)};
    let _=result(&root,json!({"status":if committed{"awaiting_health"}else{"blocked"},"stage":if committed{"health"}else{"apply"},"reason":if committed{Value::Null}else{json!(failure_reason(error))},"nonce":nonce,"at":crate::now(),"health_deadline":crate::now()+60.,"source":root,"destination":destination,"recovered_tasks":tasks,"recovery_mode":transaction["recovery_mode"],"history_recovered":transaction["history_recovered"],"timing_recovered":transaction["timing_recovered"],"message":message}));
    let mut command=Command::new(exe);command.arg("--data-dir").arg(&root);
    if req["isolated"]==true{command.arg("--isolated-instance");}
    if committed{command.arg("--repair-health").arg(nonce);}
    let mut child=match command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn(){
        Ok(child)=>child,Err(_)=>{if committed{let _=mark_health_failed(&root,nonce);}return Err("数据已保留，但看板未能自动启动，请重新打开看板".into());}
    };
    if !committed{return Ok(());}
    let deadline=Instant::now()+Duration::from_secs(45);
    while Instant::now()<deadline{
        if status(&destination).is_ok_and(|s|s["status"]=="completed"&&s["nonce"]==nonce){return Ok(());}
        if child.try_wait().map_err(|_|"重启状态不可用")?.is_some(){break;}
        std::thread::sleep(Duration::from_millis(100));
    }
    // No rollback: the destination may already have accepted new events.
    mark_health_failed(&root,nonce)?;
    Ok(())
}
fn mark_health_failed(root:&Path,nonce:&str)->Result<()>{
    let mut record=status(root)?;
    if record["nonce"]==nonce && record["status"]=="awaiting_health"{record["status"]=json!("health_failed");result(root,record)?;}
    Ok(())
}
#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn reasons_are_bounded_unicode_text_not_html(){
        let input=format!("<tag>\n{}","错".repeat(300));
        let reason=failure_reason(&input);
        assert_eq!(reason.chars().count(),240);
        assert!(!reason.contains('\n'));
        assert!(reason.starts_with("<tag>")); // UI escapes text; never interpret it as markup.
        assert!(failure_message("准备",&input).contains(&reason));
    }
    #[test]
    fn legacy_pause_failure_has_actionable_non_destructive_guidance(){
        let error="采集器不支持安全修复或未就绪；请等待旧会话正常结束后重试";
        let message=failure_message("应用",error);
        assert!(message.contains(error));
        assert!(message.contains("请先保存工作"));
        assert!(message.contains("重启 Windows"));
        assert!(message.contains("不要强行结束"));
        assert!(message.contains("尚未切换数据"));
    }
}
/// Renderer can acknowledge only after an actual readable snapshot and fresh collector heartbeat.
pub fn acknowledge(root:&Path,snapshot:&Value)->Result<()>{
    let args:Vec<String>=std::env::args().collect();
    let mut record=status(root)?;
    if record["status"]!="awaiting_health" && record["status"]!="health_failed"{return Ok(());}
    // A later normal restart can finish verification after an interrupted launch.
    if let Some(index)=args.iter().position(|a|a=="--repair-health"){
        if args.get(index+1).is_none_or(|nonce|record["nonce"]!=*nonce){return Ok(());}
    }
    let destination=PathBuf::from(record["destination"].as_str().ok_or("修复目标缺失")?);
    let source=PathBuf::from(record["source"].as_str().ok_or("修复来源缺失")?);
    if crate::migration::resolve_root(&source)?!=root||destination!=root||crate::migration::resolve_root(&destination)?!=root||snapshot["storage"]["status"]!="ok"||snapshot["healthy"]!=true{return Ok(());}
    let recovered=record["recovered_tasks"].as_u64().unwrap_or(0);
    if recovered==0 || snapshot["tasks"].as_array().is_none_or(|tasks|(tasks.len() as u64)<recovered){return Ok(());}
    record["status"]=json!("completed");record["message"]=json!(if record["recovery_mode"]=="full"{"完整数据已恢复，事件、耗时和历史归档均已保留；启动验证已通过。"}else{"任务数据已修复，原数据已保留。部分历史记录与耗时可能无法恢复。"});
    result(root,record)
}
