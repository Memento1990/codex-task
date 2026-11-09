use std::io::{Read,Write};
fn main(){if floatboard_client::build_identity::handle(){return;}
    // Hook failures are explicit diagnostics but never block the user's Codex work.
    let result=(||{
        let (tx,rx)=std::sync::mpsc::channel();std::thread::spawn(move||{let mut bytes=Vec::new();let result=std::io::stdin().take((floatboard_protocol::MAX_MESSAGE+1) as u64).read_to_end(&mut bytes).map(|_|bytes).map_err(|_|"Hook输入不可读");let _=tx.send(result);});
        let bytes=rx.recv_timeout(std::time::Duration::from_millis(800)).map_err(|_|"Hook输入超时")??;
        if bytes.len()>floatboard_protocol::MAX_MESSAGE{return Err("Hook输入过大".to_string());}
        let payload=serde_json::from_slice(&bytes).map_err(|_|"Hook输入格式无效")?;
        let args:Vec<String>=std::env::args().collect();let path=args.iter().position(|s|s=="--profile").and_then(|i|args.get(i+1)).map(std::path::PathBuf::from).map(Ok).unwrap_or_else(floatboard_client::Profile::configured_path)?;
        let profile=floatboard_client::Profile::load(&path)?;
        let value=floatboard_client::Client::new(profile).call("hook_context",payload)?;
        std::io::stdout().write_all(value.to_string().as_bytes()).map_err(|_|"Hook输出中断".to_string())
    })();
    if result.is_err(){eprintln!("Floatboard Hook未确认提交；Agent不可用或请求失败。");}
}
