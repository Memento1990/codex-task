use std::path::PathBuf;
use floatboard_client::{Profile,files::read_json};
use floatboard_protocol::Result;
use floatboard_agent::maintenance;
fn main(){if floatboard_client::build_identity::handle(){return;}if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
fn run()->Result<()>{
    let args=std::env::args().collect::<Vec<_>>();let get=|name:&str|args.iter().position(|s|s==name).and_then(|i|args.get(i+1)).map(PathBuf::from).ok_or_else(||format!("缺少{name}"));
    let result=match args.get(1).map(String::as_str){
        Some("preview")=>{let sources:Vec<maintenance::Source>=serde_json::from_value(read_json(&get("--sources")?)?).map_err(|_|"来源清单无效")?;let states:Vec<maintenance::StateSource>=if args.iter().any(|v|v=="--state-sources"){serde_json::from_value(read_json(&get("--state-sources")?)?).map_err(|_|"设置来源清单无效")?}else{Vec::new()};maintenance::preview(&sources,&states,&get("--output")?)?},
        Some("apply")=>maintenance::apply(&get("--preview")?,&read_json(&get("--resolutions")?)?,Profile::load(&get("--profile")?)?)?,
        Some("initialize")=>maintenance::initialize(Profile::load(&get("--profile")?)?)?,
        Some("check")=>maintenance::check(Profile::load(&get("--profile")?)?)?,
        Some("backup")=>{let p=Profile::load(&get("--profile")?)?;let _owner=floatboard_client::windows::Instance::acquire(&p.profile_id)?;let db=floatboard_agent::storage::Database::open(p)?;floatboard_agent::storage::put(&db.store.db,"write_gate",&serde_json::json!("paused"))?;db.backup("upgrade",None)?},
        Some("restore")=>maintenance::restore(&get("--backup")?,Profile::load(&get("--profile")?)?)?,
        Some("migrate-config")=>floatboard_agent::integration::migrate_legacy(&Profile::load(&get("--profile")?)?,&get("--legacy-receipt")?,&if args.iter().any(|v|v=="--confirmed"){read_json(&get("--confirmation")?)?}else{serde_json::json!({})})?,
        _=>return Err("使用 preview/apply/initialize/check/restore 离线命令".into())};
    println!("{result}");Ok(())
}
