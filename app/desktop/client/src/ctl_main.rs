use floatboard_client::{Profile,Client,Result};
use serde_json::json;
use std::path::PathBuf;
fn main(){if floatboard_client::build_identity::handle(){return;}if let Err(error)=run(){eprintln!("{error}");std::process::exit(1);}}
fn run()->Result<()>{
    let args:Vec<String>=std::env::args().collect();
    let get=|name:&str|args.iter().position(|s|s==name).and_then(|i|args.get(i+1)).cloned();
    let path=get("--profile").map(PathBuf::from).map(Ok).unwrap_or_else(Profile::configured_path)?;
    if args.get(1).map(String::as_str)==Some("init-test-profile"){
        if path.exists(){return Err("测试profile已存在，拒绝覆盖".into());}
        let root=PathBuf::from(get("--root").ok_or("缺少测试root")?);let agent=PathBuf::from(get("--agent").ok_or("缺少测试Agent")?);
        let profile=Profile{profile_id:uuid::Uuid::new_v4().to_string(),environment:"development".into(),owner_sid:floatboard_client::windows::sid()?,root:root.clone(),port:get("--port").ok_or("缺少端口")?.parse().map_err(|_|"端口无效")?,gui_exe:agent.with_file_name("CodexFloatboard.exe"),agent_exe:agent,codex_home:root.join("codex")};
        profile.validate()?;floatboard_client::files::atomic_json(&path,&json!(profile))?;return Ok(());
    }
    if args.get(1).map(String::as_str)==Some("test-task"){
        let p=Profile::load(&path)?;if p.environment!="development"{return Err("测试任务命令禁止正式profile".into());}
        return match get("--action").as_deref(){Some("register")=>floatboard_client::scheduled::register(&p,&path),Some("run")=>floatboard_client::scheduled::run(&p,&path),Some("remove")=>floatboard_client::scheduled::remove_test(&p,&path),_=>Err("测试任务使用register/run/remove".into())};
    }
    let method=get("--method").ok_or("缺少method")?;
    let params=if let Some(path)=get("--params"){floatboard_client::files::read_json(std::path::Path::new(&path))?}else{json!({})};
    let value=Client::new(Profile::load(&path)?).call(&method,params)?;println!("{}",value);Ok(())
}
