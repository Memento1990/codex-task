//! Explicit migration installer. Signed package staging is delegated to the same verifier as updates.
use std::path::PathBuf;
use floatboard_client::{Profile,files::{read_json,atomic_json}};
use floatboard_shell::{Result,install};
fn main(){if floatboard_shell::build_identity::handle(){return;}if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
fn run()->Result<()>{
    let args=std::env::args().collect::<Vec<_>>();let get=|key:&str|args.iter().position(|v|v==key).and_then(|i|args.get(i+1)).map(PathBuf::from).ok_or_else(||format!("缺少{key}"));
    let root=get("--install-root")?;
    if args.get(1).map(String::as_str)==Some("stage"){let manifest=floatboard_shell::update_install::stage_signed(&root,&get("--archive")?,&get("--manifest")?,&get("--signature")?)?;println!("{}",serde_json::to_value(manifest).map_err(|_|"清单无效")?);return Ok(());}
    let profile=Profile::load(&get("--profile")?)?;
    match args.get(1).map(String::as_str){
        Some("preview")=>{let mut value=install::preview(&root,&profile)?;value["legacy"]=floatboard_shell::legacy::inventory(&floatboard_shell::legacy::roots(&get("--legacy-roots")?)?)?;value["legacy_startup"]=floatboard_shell::legacy::startup_preview(&profile,&floatboard_shell::legacy::roots(&get("--legacy-roots")?)?)?;value["migration_validation"]=read_json(&profile.root.join("migration-validation.json")).ok().unwrap_or_default();println!("{value}");Ok(())},
        Some("cutover") if args.iter().any(|a|a=="--confirmed")=>{floatboard_shell::legacy::wait_normal_exit(&floatboard_shell::legacy::roots(&get("--legacy-roots")?)?,std::time::Duration::from_secs(60))?;install::validate_initial_migration(&profile)?;let version=args.iter().position(|a|a=="--version").and_then(|i|args.get(i+1)).ok_or("缺少版本")?;let proof=read_json(&root.join("package-proofs").join(version).join("source.json"))?;floatboard_shell::update_install::stage_signed(&root,&PathBuf::from(proof["archive"].as_str().ok_or("安装证明缺失")?),&PathBuf::from(proof["manifest"].as_str().ok_or("安装清单缺失")?),&PathBuf::from(proof["signature"].as_str().ok_or("安装签名缺失")?))?;if profile.agent_exe!=root.join("versions").join(version).join("FloatboardAgent.exe"){return Err("profile不属于已验证安装版本".into());}if args.iter().any(|v|v=="--legacy-receipt")&&!root.join("legacy-integration-receipt.json").exists(){let mut command=std::process::Command::new(profile.agent_exe.with_file_name("FloatboardMaintenance.exe"));command.arg("migrate-config").arg("--profile").arg(get("--profile")?).arg("--legacy-receipt").arg(get("--legacy-receipt")?).arg("--confirmation").arg(get("--integration-confirmation")?).arg("--confirmed");let result=command.output().map_err(|_|"旧接入迁移工具不可用")?;if !result.status.success(){return Err("旧接入归属迁移失败；请检查预览，不覆盖非本产品配置".into());}let receipt:serde_json::Value=serde_json::from_slice(&result.stdout).map_err(|_|"旧接入迁移回执无效")?;atomic_json(&root.join("legacy-integration-receipt.json"),&receipt)?;}
        let startup=floatboard_shell::legacy::remove_startup(&profile,&floatboard_shell::legacy::roots(&get("--legacy-roots")?)?,&read_json(&get("--startup-confirmation")?)?)?;if !root.join("legacy-startup-receipt.json").exists(){atomic_json(&root.join("legacy-startup-receipt.json"),&startup)?;}
        let receipt=install::cutover(&root,&get("--registration")?,profile,version,true)?;atomic_json(&root.join("migration-install-receipt.json"),&receipt)?;println!("{receipt}");Ok(())},
        Some("activate") if args.iter().any(|a|a=="--confirmed")=>{let result=install::activate(&root,&get("--registration")?)?;println!("{result}");Ok(())},
        _=>Err("使用 preview / cutover --confirmed / activate --confirmed；本工具不自动批准冲突或正式切换".into())
    }
}
