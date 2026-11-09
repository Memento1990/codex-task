use std::sync::Arc;
use axum::{Router,routing::{get,post},middleware::{self,Next},extract::{State,Request as HttpRequest},http::StatusCode,response::{IntoResponse,Response as HttpResponse}};
use rmcp::{ServerHandler,model::*,service::{RequestContext,RoleServer},ErrorData};
use rmcp::transport::streamable_http_server::{StreamableHttpService,StreamableHttpServerConfig,session::local::LocalSessionManager};
use tokio::{io::{AsyncBufReadExt,AsyncWriteExt,BufReader},net::windows::named_pipe::{NamedPipeServer,ServerOptions},sync::Semaphore};
use tokio_util::sync::CancellationToken;
use floatboard_protocol::{Request,Response,PROTOCOL_VERSION,MAX_MESSAGE};
use floatboard_client::{Profile,Result,windows::Security};
use serde_json::{json,Value};
use crate::auth;

pub type Dispatch=Arc<dyn Fn(Request)->Result<Value>+Send+Sync>;
#[derive(Clone)]
struct Mcp{profile:Profile,dispatch:Dispatch}
impl ServerHandler for Mcp{
    fn get_info(&self)->ServerConfig{let mut config=ServerConfig::new(ServerCapabilities::builder().enable_tools().build());config.server_info=Implementation::new(self.profile.server_name(),env!("CARGO_PKG_VERSION"));config}
    async fn list_tools(&self,_:Option<PaginatedRequestParams>,_:RequestContext<RoleServer>)->std::result::Result<ListToolsResult,ErrorData>{
        let report=serde_json::from_str::<Value>(include_str!("../../protocol/src/report-schema.json")).unwrap();
        let hook=json!({"type":"object","properties":{"session_id":{"type":"string"},"cwd":{"type":"string"},"hook_event_name":{"type":"string"}},"required":["session_id","cwd","hook_event_name"]});
        let mut result=ListToolsResult::default();result.tools=vec![Tool::new("hook_context","接收可信Hook事件并返回会话绑定上下文",hook.as_object().unwrap().clone()),Tool::new("report_progress","提交任务完整快照；仅数据库事务提交成功才返回accepted",report.as_object().unwrap().clone())];Ok(result)
    }
    async fn call_tool(&self,p:CallToolRequestParams,_:RequestContext<RoleServer>)->std::result::Result<CallToolResponse,ErrorData>{
        if !["hook_context","report_progress"].contains(&p.name.as_ref()){return Err(ErrorData::invalid_params("未知工具",None));}
        let request=Request{protocol:PROTOCOL_VERSION,profile_id:self.profile.profile_id.clone(),request_id:uuid::Uuid::new_v4().to_string(),method:p.name.to_string(),params:Value::Object(p.arguments.unwrap_or_default())};
        let dispatch=self.dispatch.clone();let result=tokio::task::spawn_blocking(move||dispatch(request)).await.unwrap_or_else(|_|Err("Agent请求中断".into()));
        let value=match result{Ok(value)=>CallToolResult::success(vec![ContentBlock::text(value.to_string())]),Err(error)=>CallToolResult::error(vec![ContentBlock::text(error)])};Ok(value.into())
    }
}
async fn host_guard(State(origin):State<String>,request:HttpRequest,next:Next)->HttpResponse{
    let host=origin.trim_start_matches("http://");
    if request.headers().get("host").and_then(|s|s.to_str().ok())!=Some(host)||request.headers().get("origin").is_some_and(|s|s.to_str().ok()!=Some(origin.as_str())){return StatusCode::FORBIDDEN.into_response();}
    next.run(request).await
}
async fn bearer_guard(State(auth):State<auth::Shared>,request:HttpRequest,next:Next)->HttpResponse{
    let token=request.headers().get("authorization").and_then(|s|s.to_str().ok()).and_then(|s|s.strip_prefix("Bearer "));
    let valid=token.is_some_and(|token|auth.lock().unwrap().valid(token));
    if !valid{let origin=auth.lock().unwrap().profile.origin();return (StatusCode::UNAUTHORIZED,[("www-authenticate",format!("Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource\""))]).into_response();}
    auth.lock().unwrap().authenticated_requests+=1;
    next.run(request).await
}
pub async fn http(profile:Profile,auth:auth::Shared,dispatch:Dispatch,cancel:CancellationToken)->Result<()>{
    let service_profile=profile.clone();let service_dispatch=dispatch.clone();
    let mut config=StreamableHttpServerConfig::default();config.legacy_session_mode=false;config.json_response=true;config.max_request_body_bytes=MAX_MESSAGE;config.cancellation_token=cancel.child_token();
    let service=StreamableHttpService::new(move||Ok(Mcp{profile:service_profile.clone(),dispatch:service_dispatch.clone()}),Arc::new(LocalSessionManager::default()),config);
    let mcp=Router::new().nest_service("/mcp",service).layer(middleware::from_fn_with_state(auth.clone(),bearer_guard));
    let router=Router::new()
        .route("/.well-known/oauth-authorization-server",get(auth::metadata))
        .route("/.well-known/oauth-protected-resource",get(auth::resource))
        .route("/.well-known/oauth-protected-resource/mcp",get(auth::resource))
        .route("/authorize",get(auth::authorize)).route("/register",post(auth::register)).route("/token",post(auth::token))
        .with_state(auth).merge(mcp).layer(axum::extract::DefaultBodyLimit::max(MAX_MESSAGE)).layer(middleware::from_fn_with_state(profile.origin(),host_guard));
    let listener=tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,profile.port)).await.map_err(|_|"Agent登记端口不可用，未切换端口")?;
    axum::serve(listener,router).with_graceful_shutdown(cancel.cancelled_owned()).await.map_err(|_|"HTTP监听失败".into())
}
fn pipe(profile:&Profile,first:bool)->Result<NamedPipeServer>{
    let mut security=Security::user()?;
    unsafe{ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).create_with_security_attributes_raw(profile.pipe(),&mut security.attributes as *mut _ as *mut std::ffi::c_void)}.map_err(|_|"Agent管道不可用".into())
}
pub async fn ipc(profile:Profile,instance:String,dispatch:Dispatch,cancel:CancellationToken)->Result<()>{
    let mut server=pipe(&profile,true)?;let slots=Arc::new(Semaphore::new(128));let mut handlers=tokio::task::JoinSet::new();
    loop{
        tokio::select!{_ = cancel.cancelled()=>break,result=server.connect()=>{result.map_err(|_|"IPC连接失败")?;}}
        let connected=server;server=pipe(&profile,false)?;
        let Ok(permit)=slots.clone().try_acquire_owned()else{continue};let dispatch=dispatch.clone();let profile=profile.clone();let instance=instance.clone();
        handlers.spawn(async move{let _permit=permit;let _=tokio::time::timeout(std::time::Duration::from_secs(10),async move{
            let mut reader=BufReader::new(connected);let mut bytes=Vec::new();
            loop{let part=reader.fill_buf().await.map_err(|_|"IPC读取中断")?;if part.is_empty(){return Ok::<_,String>(());}let n=part.iter().position(|b|*b==b'\n').map(|i|i+1).unwrap_or(part.len());if bytes.len()+n>MAX_MESSAGE{return Err("IPC请求过大".into());}let done=part[n-1]==b'\n';bytes.extend_from_slice(&part[..n]);reader.consume(n);if done{break;}}
            let result=match serde_json::from_slice::<Request>(&bytes){Ok(request) if request.protocol==PROTOCOL_VERSION&&request.profile_id==profile.profile_id=>tokio::task::spawn_blocking(move||dispatch(request)).await.unwrap_or_else(|_|Err("IPC操作中断".into())),_=>Err("IPC协议或profile不匹配".into())};
            let response=Response{protocol:PROTOCOL_VERSION,profile_id:profile.profile_id,agent_instance_id:instance,result:result.as_ref().ok().cloned(),error:result.err()};
            let mut bytes=serde_json::to_vec(&response).map_err(|_|"IPC响应编码失败")?;if bytes.len()>MAX_MESSAGE{return Err("IPC响应过大".into());}bytes.push(b'\n');reader.get_mut().write_all(&bytes).await.map_err(|_|"IPC响应中断")?;Ok(())
        }).await;});
        while handlers.try_join_next().is_some(){}
    }
    drop(server);while handlers.join_next().await.is_some(){}Ok(())
}
