use std::{os::windows::io::AsRawHandle,time::Duration};
use serde_json::Value;
use tokio::{io::{AsyncBufReadExt,AsyncWriteExt,BufReader},net::windows::named_pipe::ClientOptions};
use floatboard_protocol::{Request,Response,MAX_MESSAGE,PROTOCOL_VERSION};
use crate::{Profile,Result,windows};

#[derive(Clone)]
pub struct Client{pub profile:Profile}
impl Client {
    pub fn new(profile:Profile)->Self{Self{profile}}
    pub fn call(&self,method:&str,params:Value)->Result<Value>{self.call_id(method,params,&uuid::Uuid::new_v4().to_string())}
    pub fn call_id(&self,method:&str,params:Value,request_id:&str)->Result<Value>{
        let request=Request{protocol:PROTOCOL_VERSION,profile_id:self.profile.profile_id.clone(),request_id:request_id.into(),method:method.into(),params};
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|"IPC执行器不可用")?;
        runtime.block_on(async{tokio::time::timeout(Duration::from_millis(if method=="hook_context"{2000}else if method=="stop"{25000}else{5000}),self.exchange(&request)).await.map_err(|_|"Agent响应超时，提交结果尚未确认".to_string())?})
    }
    async fn exchange(&self,request:&Request)->Result<Value>{
        let mut pipe=loop{match ClientOptions::new().open(self.profile.pipe()){
            Ok(pipe)=>break pipe,
            Err(error) if error.raw_os_error()==Some(231)=>tokio::time::sleep(Duration::from_millis(5)).await,
            Err(_)=>return Err("Agent不可用；未确认提交，请稍后重试".into())
        }};
        let mut pid=0;
        if unsafe{windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId(pipe.as_raw_handle(),&mut pid)}==0{return Err("Agent管道身份不可验证".into());}
        let expected=std::fs::canonicalize(&self.profile.agent_exe).map_err(|_|"Agent登记程序缺失")?;
        if std::fs::canonicalize(windows::process_image(pid)?).map_err(|_|"Agent程序不可验证")?!=expected{return Err("Agent程序与登记身份不匹配".into());}
        // Hold the verified process object through stop; mutex release alone precedes process exit.
        let stopping=if request.method=="stop"{let handle=unsafe{windows_sys::Win32::System::Threading::OpenProcess(0x00100000,0,pid)};if handle.is_null(){return Err("Agent退出句柄不可用".into());}Some(windows::Handle(handle))}else{None};
        let mut bytes=serde_json::to_vec(request).map_err(|_|"IPC编码失败")?;
        if bytes.len()>MAX_MESSAGE{return Err("请求过大".into());}bytes.push(b'\n');
        pipe.write_all(&bytes).await.map_err(|_|"Agent请求中断，提交结果尚未确认")?;
        let mut reader=BufReader::new(pipe);let mut bytes=Vec::new();
        loop {let part=reader.fill_buf().await.map_err(|_|"Agent响应中断，提交结果尚未确认")?;if part.is_empty(){return Err("Agent响应不完整，提交结果尚未确认".into());}
            let n=part.iter().position(|b|*b==b'\n').map(|n|n+1).unwrap_or(part.len());
            if bytes.len()+n>MAX_MESSAGE{return Err("Agent响应过大".into());}let done=part[n-1]==b'\n';bytes.extend_from_slice(&part[..n]);reader.consume(n);if done{break;}}
        let response:Response=serde_json::from_slice(&bytes).map_err(|_|"Agent响应格式无效")?;
        if response.protocol!=PROTOCOL_VERSION||response.profile_id!=self.profile.profile_id{return Err("Agent协议或profile不匹配".into());}
        match (response.result,response.error){(Some(value),None)=>{if let Some(process)=stopping{if value["stopping"]==true{let status=tokio::task::spawn_blocking(move||{let process=process;unsafe{windows_sys::Win32::System::Threading::WaitForSingleObject(process.0,20000)}}).await.map_err(|_|"Agent退出等待中断")?;if status!=0{return Err("Agent尚未完全退出，未继续切换".into());}}}Ok(value)},(_,Some(error))=>Err(error),_=>Err("Agent响应不完整".into())}
    }
}
