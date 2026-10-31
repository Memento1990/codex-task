use std::io::{Read,Write};
fn main(){
    let root=floatboard_core::data_root();let args:Vec<String>=std::env::args().collect();
    let result=if args.iter().any(|a|a=="--worker"){floatboard_core::runtime::worker(&root)}
    else if args.iter().any(|a|a=="--mcp"){
        if std::env::var("FLOATBOARD_PREFLIGHT").as_deref()!=Ok("1"){let _=floatboard_core::runtime::spawn_worker(&root);}
        floatboard_core::ingest::serve_mcp(&root,std::io::stdin().lock(),std::io::stdout().lock())
    }else if args.iter().any(|a|a=="--connect"){
        let result=floatboard_core::integration::connect(&root);let failed=result.is_err();let value=match result{Ok(value)=>value,Err(message)=>serde_json::json!({"success":false,"message":message})};let _=writeln!(std::io::stdout(),"{value}");if failed{std::process::exit(1);}Ok(())
    }else{
        let mut bytes=Vec::new();
        let received=(||{
            std::io::stdin().take(1_000_001).read_to_end(&mut bytes).map_err(|_|"Hook 输入不可读".to_string())?;
            if bytes.len()>1_000_000{return Err("Hook 输入过大".to_string());}
            let value=serde_json::from_slice(&bytes).map_err(|_|"Hook 输入格式无效".to_string())?;
            floatboard_core::ingest::hook(&root,&value)
        })();
        let result=match received { Ok(value)=>value, Err(error)=>{
            // 仅记录固定诊断码和时间，不保存原始 stdin、提示词或凭据。
            let _=floatboard_core::store::atomic_json(&root.join("hook-status.json"),&serde_json::json!({"status":"error","at":floatboard_core::now(),"code":if error.contains("迁移"){"data_busy"}else{"hook_failed"}}));
            serde_json::json!({})
        }};
        if std::env::var("FLOATBOARD_PREFLIGHT").as_deref()!=Ok("1"){let _=floatboard_core::runtime::spawn_worker(&root);}let _=writeln!(std::io::stdout(),"{result}");Ok(())
    };
    if result.is_err(){eprintln!("本地任务服务不可用，Codex 可继续执行");std::process::exit(1);}
}
