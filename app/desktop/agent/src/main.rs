#![windows_subsystem = "windows"]
use floatboard_agent::{auth,transport,storage};
use floatboard_client::{Profile,Result,windows::{Instance,Security}};
use std::sync::Arc;
use serde_json::json;

fn main(){if floatboard_client::build_identity::handle(){return;}if let Err(error)=run(){eprintln!("{error}");std::process::exit(1);}}
fn run()->Result<()>{
    let args:Vec<String>=std::env::args().collect();
    let path=args.iter().position(|s|s=="--profile").and_then(|i|args.get(i+1)).map(std::path::PathBuf::from).map(Ok).unwrap_or_else(Profile::configured_path)?;
    let profile=Profile::load(&path)?;if std::fs::canonicalize(std::env::current_exe().map_err(|_|"当前程序不可读")?).map_err(|_|"当前程序不可解析")?!=std::fs::canonicalize(&profile.agent_exe).map_err(|_|"登记程序缺失")?{return Err("Agent版本不属于已登记profile".into());}if args.iter().any(|v|v=="--scheduled")&&!floatboard_client::scheduled::should_run(&profile)?{return Ok(());}let _owner=Instance::acquire(&profile.profile_id)?;
    std::fs::create_dir_all(&profile.root).map_err(|_|"Agent目录不可写")?;Security::protect(&profile.root)?;
    floatboard_client::files::atomic_json(&profile.root.join("profile.json"),&json!(profile))?;
    let auth=auth::Auth::load(profile.clone())?;let instance=uuid::Uuid::new_v4().to_string();let cancel=tokio_util::sync::CancellationToken::new();
    let storage=storage::Actor::start(profile.clone())?;let business=storage.dispatcher();
    if profile.environment=="production"{
        if business(floatboard_protocol::Request{protocol:2,profile_id:profile.profile_id.clone(),request_id:uuid::Uuid::new_v4().to_string(),method:"snapshot".into(),params:json!({})}).is_ok_and(|v|v["options"]["auto_start"]==true){let _=std::process::Command::new(&profile.gui_exe).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();}
    }
    let dispatch_profile=profile.clone();let dispatch_auth=auth.clone();let dispatch_instance=instance.clone();let stop=cancel.clone();
    let gui_ready=std::sync::atomic::AtomicBool::new(false);
    let dispatch:transport::Dispatch=Arc::new(move|request|{
        if request.method.starts_with("pairing_"){return dispatch_auth.lock().map_err(|_|"认证状态异常")?.call(&request.method,&request.params);}
        if request.method=="gui_ready"{if request.params["agent_instance_id"]!=dispatch_instance{return Err("GUI确认不属于本次Agent实例".into());}gui_ready.store(true,std::sync::atomic::Ordering::Release);return Ok(json!({"accepted":true}));}
        match request.method.as_str(){
            "stop" if request.params["confirmed"]==true=>{floatboard_client::scheduled::intent(&dispatch_profile,false)?;stop.cancel();Ok(json!({"stopping":true}))},
            _=>{let method=request.method.clone();let mut result=business(request)?;if ["snapshot","identity","diagnostics"].contains(&method.as_str())&&result.is_object(){result["profile_id"]=json!(dispatch_profile.profile_id);result["environment"]=json!(dispatch_profile.environment);result["agent_instance_id"]=json!(dispatch_instance);result["protocol_version"]=json!(floatboard_protocol::PROTOCOL_VERSION);if method=="identity"{result["gui_ready"]=json!(gui_ready.load(std::sync::atomic::Ordering::Acquire));let auth=dispatch_auth.lock().map_err(|_|"认证状态异常")?;result["authorized_clients"]=json!(auth.authorized_clients());result["authenticated_requests"]=json!(auth.authenticated_requests);}}Ok(result)}
        }
    });
    let runtime=tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|_|"Agent执行器不可用")?;
    runtime.block_on(async{
        let http=transport::http(profile.clone(),auth,dispatch.clone(),cancel.clone());let ipc=transport::ipc(profile,instance,dispatch,cancel.clone());
        tokio::pin!(http);tokio::pin!(ipc);
        tokio::select!{result=&mut http=>{cancel.cancel();let _=ipc.await;result},result=&mut ipc=>{cancel.cancel();let _=http.await;result}}
    })
}
