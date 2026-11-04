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
fn result(root:&Path,value:Value)->Result<()>{atomic_json(&base(root)?.join("latest.json"),&value)}
pub fn status(root:&Path)->Result<Value>{
    let path=base(root)?.join("latest.json");crate::migration::check_path(&path)?;
    if !path.exists(){return Ok(json!({"status":"not_started"}));}
    let mut record=limited(&path)?;
    if record["status"]=="preparing"{
        if let (Some(source),Some(destination))=(record["source"].as_str(),record["destination"].as_str()){
            if crate::migration::resolve_root(Path::new(source))?==Path::new(destination){record["status"]=json!("awaiting_health");record["health_deadline"]=json!(crate::now()+60.);result(root,record.clone())?;}
            else if record["at"].as_f64().is_some_and(|at|crate::now()-at>90.) && crate::runtime::ProcessLock::acquire(&base(root)?.join("executor.lock")).is_ok(){record["status"]=json!("failed");result(root,record.clone())?;}
        }
    }
    if record["status"]=="awaiting_health" && record["health_deadline"].as_f64().is_some_and(|at|crate::now()>at) && crate::runtime::ProcessLock::acquire(&base(root)?.join("executor.lock")).is_ok(){record["status"]=json!("health_failed");result(root,record.clone())?;}
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
/// Caller has already gated Engine and drained foreground notifications/SQLite.
pub fn prepare(root:&Path)->Result<Value>{
    let candidate=crate::repair_transaction::prepare(root)?;
    let id=candidate["id"].as_str().or_else(||candidate["operation_id"].as_str()).ok_or("修复准备缺少操作身份")?;
    let exe=std::env::current_exe().map_err(|_|"看板路径不可用")?;
    crate::migration::check_path(&exe)?;
    let helper=crate::runtime::reporter_path()?;crate::migration::check_path(&helper)?;
    let nonce=uuid::Uuid::new_v4().to_string();let request=base(root)?.join(format!("request-{nonce}.json"));
    atomic_json(&request,&json!({"version":1,"root":root,"id":id,"nonce":nonce,"pid":std::process::id(),"exe":exe,"isolated":std::env::args().any(|a|a=="--isolated-instance")}))?;
    result(root,json!({"status":"preparing","nonce":nonce,"at":crate::now(),"source":root,"destination":candidate["destination"],"recovered_tasks":candidate["recovered_tasks"]}))?;
    let mut child=match Command::new(helper).arg("--repair-execute").arg(&request).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn(){
        Ok(child)=>child,Err(_)=>{let _=result(root,json!({"status":"failed","nonce":nonce}));return Err("修复执行器无法启动，保留当前看板".into());}
    };
    let until=Instant::now()+Duration::from_secs(20);
    while Instant::now()<until{
        if limited(&request.with_extension("ready")).is_ok_and(|v|v["nonce"]==nonce){return Ok(json!({"status":"ready","recovered_tasks":candidate["recovered_tasks"]}));}
        if request.with_extension("error").exists()||child.try_wait().map_err(|_|"修复执行器状态不可用")?.is_some(){return Err("修复执行器未准备好，原数据未替换".into());}
        std::thread::sleep(Duration::from_millis(50));
    }
    atomic_json(&request.with_extension("cancel"),&json!({"nonce":nonce}))?;
    Err("修复准备超时，原数据未替换".into())
}

pub fn execute(request:&Path)->Result<()>{
    let outcome=execute_inner(request);
    if outcome.is_err(){
        if let Ok(req)=limited(request){
            if let (Some(root),Some(nonce))=(req["root"].as_str(),req["nonce"].as_str()){
                if uuid::Uuid::parse_str(nonce).is_ok() && base(Path::new(root)).is_ok_and(|p|request==p.join(format!("request-{nonce}.json"))){
                    let _=atomic_json(&request.with_extension("error"),&json!({"nonce":nonce}));
                    // Do not overwrite a committed result when restart alone failed.
                    if status(Path::new(root)).is_ok_and(|v|v["status"]=="preparing"&&v["nonce"]==nonce){let _=result(Path::new(root),json!({"status":"failed","nonce":nonce,"message":"未能安全完成修复，原数据已保留。"}));}
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
    let _=result(&root,json!({"status":if committed{"awaiting_health"}else{"blocked"},"nonce":nonce,"health_deadline":crate::now()+60.,"source":root,"destination":destination,"recovered_tasks":tasks,"message":if committed{"数据已切换，正在确认看板和后续上报是否正常。"}else{"暂未完成修复。原数据已保留；可能仍有旧版上报连接占用，请在会话正常结束后重试。"}}));
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
    record["status"]=json!("completed");record["message"]=json!("任务数据已修复，原数据已保留。部分历史记录与耗时可能无法恢复。");
    result(root,record)
}
