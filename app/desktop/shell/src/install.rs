//! Durable, resumable cutover. Once selected, a version is never automatically rolled back.
use std::{path::{Path,PathBuf},process::{Command,Stdio},time::{Duration,Instant}};
use std::os::windows::process::CommandExt;
use serde_json::{json,Value};
use floatboard_client::{Client,Profile,files::{read_json,atomic_json},scheduled,windows::Instance};
use crate::{Result,file_lock::ProcessLock};

pub fn preview(root:&Path,profile:&Profile)->Result<Value>{
    crate::paths::check_path(root)?;profile.validate()?;
    Ok(json!({"install_root":root,"profile_id":profile.profile_id,"data_root":profile.root,"task":scheduled::name(profile),"environment":profile.environment,"agent":profile.agent_exe,"protocol":2,"schema":2,"automatic_database_rollback":false,"config_scope":profile.server_name()}))
}
fn save(path:&Path,journal:&mut Value,phase:&str)->Result<()>{journal["phase"]=json!(phase);journal["updated_at"]=json!(crate::now());atomic_json(path,journal)?;#[cfg(test)]if journal["test_interrupt_after"]==phase{return Err("synthetic interrupted installer".into());}Ok(())}
fn offline_backup(profile:&Profile,config:&Path)->Result<Value>{let result=Command::new(profile.agent_exe.with_file_name("FloatboardMaintenance.exe")).args(["backup","--profile"]).arg(config).stdin(Stdio::null()).creation_flags(0x08000000).output().map_err(|_|"离线备份工具不可用")?;if !result.status.success(){return Err("升级前离线备份未完成".into());}serde_json::from_slice(&result.stdout).map_err(|_|"离线备份回执无效".into())}
fn wait_health(profile:&Profile)->Result<Value>{let until=Instant::now()+Duration::from_secs(20);let client=Client::new(profile.clone());loop{if let Ok(value)=client.call("identity",json!({})){if value["storage"]["status"]!="ok"{return Err("新Agent存储故障，未开放业务写入".into());}return Ok(value);}if Instant::now()>=until{return Err("Agent健康检查超时，未开放业务写入".into());}std::thread::sleep(Duration::from_millis(100));}}
fn offline_check(profile:&Profile,config:&Path)->Result<()>{let maintenance=profile.agent_exe.with_file_name("FloatboardMaintenance.exe");let result=Command::new(maintenance).args(["check","--profile"]).arg(config).stdin(Stdio::null()).creation_flags(0x08000000).output().map_err(|_|"离线校验工具不可用")?;if !result.status.success(){return Err("离线数据库校验失败，未启动Agent".into());}let value:Value=serde_json::from_slice(&result.stdout).map_err(|_|"离线校验回执无效")?;if value["integrity"]!="ok"{return Err("离线数据库完整性未通过".into());}Ok(())}

pub fn cutover(root:&Path,config:&Path,desired:Profile,version:&str,confirmed:bool)->Result<Value>{
    if !confirmed{return Err("安装切换必须确认预览".into());}preview(root,&desired)?;
    let _transaction=ProcessLock::acquire(&root.join("install.lock"))?;
    let journal_path=root.join("install-transaction.json");
    let registered=if config.exists(){Some(Profile::load(config)?)}else{None};
    if registered.as_ref().is_some_and(|p|p.profile_id!=desired.profile_id||p.owner_sid!=desired.owner_sid||p.root!=desired.root){return Err("更新不能更换profile或数据库目录；首次迁移须使用独立登记文件".into());}
    if journal_path.exists(){let previous=read_json(&journal_path)?;if previous["phase"]=="complete"&&previous["desired"]!=json!(desired){let history=root.join("install-history");std::fs::create_dir_all(&history).map_err(|_|"安装历史不可写")?;std::fs::rename(&journal_path,history.join(format!("{}.json",uuid::Uuid::new_v4()))).map_err(|_|"安装历史保存失败")?;}}
    let mut journal=if journal_path.exists(){let j=read_json(&journal_path)?;if j["desired"]!=json!(desired)||j["version"]!=version||j["config"]!=json!(config){return Err("尚有另一安装事务，请先检查并完成该事务".into());}j}else{json!({"version":version,"config":config,"desired":desired,"before":registered,"phase":"prepared","preview":preview(root,&desired)?})};
    let old:Option<Profile>=serde_json::from_value(journal["before"].clone()).map_err(|_|"安装事务旧身份无效")?;
    if journal["phase"]=="complete"{return Ok(journal);}
    atomic_json(&journal_path,&journal)?;
    if journal["phase"]=="prepared"{save(&journal_path,&mut journal,"quiescing")?;}
    if journal["phase"]=="quiescing"{
        if let Some(profile)=&old{scheduled::enabled(profile,config,false)?;let client=Client::new(profile.clone());
            if client.call("identity",json!({})).is_ok(){client.call("quiesce",json!({"confirmed":true}))?;journal["backup"]=client.call("backup",json!({"kind":"upgrade"}))?;}
            else{journal["backup"]=offline_backup(profile,config)?;}
        }
        save(&journal_path,&mut journal,"backed_up")?;
    }
    if journal["phase"]=="backed_up"{
        if let Some(profile)=&old{let client=Client::new(profile.clone());if client.call("identity",json!({})).is_ok(){client.call("stop",json!({"confirmed":true}))?;}}
        save(&journal_path,&mut journal,"stopping")?;
    }
    if journal["phase"]=="stopping"{
        let until=Instant::now()+Duration::from_secs(20);loop{match Instance::acquire(&desired.profile_id){Ok(owner)=>{drop(owner);break},Err(_) if Instant::now()<until=>std::thread::sleep(Duration::from_millis(100)),Err(e)=>return Err(e)}}
        offline_check(&desired,config_for_check(&desired,config,root)?.as_path())?;
        let _owner=Instance::acquire(&desired.profile_id)?;
        // Maintenance validation and Agent use the same mutex. The old task stays disabled.
        floatboard_client::files::atomic_bytes(&root.join("CodexFloatboard.exe"),&std::fs::read(desired.agent_exe.with_file_name("FloatboardLauncher.exe")).map_err(|_|"签名包缺少根启动器")?)?;atomic_json(config,&json!(desired))?;atomic_json(&root.join("current.json"),&json!({"version":version,"protocol":2}))?;
        scheduled::replace(old.as_ref(),&desired,config)?;save(&journal_path,&mut journal,"registered")?;
    }
    if journal["phase"]=="registered"{scheduled::run(&desired,config)?;let health=wait_health(&desired)?;if health["write_gate"]!="paused"{Client::new(desired.clone()).call("quiesce",json!({"confirmed":true}))?;return Err("新Agent意外开放写入，已暂停并停止安装事务".into());}let client=Client::new(desired.clone());let plan=client.call("preview",json!({}))?;journal["integration"]=client.call("connect",json!({"confirmed":true,"before_hash":plan["before_hash"]}))?;journal["health"]=health;save(&journal_path,&mut journal,"healthy_paused")?;}

    Ok(journal)
}
fn config_for_check(profile:&Profile,config:&Path,root:&Path)->Result<PathBuf>{let path=root.join("validation-profile.json");if path==config{return Err("校验profile路径冲突".into());}atomic_json(&path,&json!(profile))?;Ok(path)}

pub fn validate_initial_migration(profile:&Profile)->Result<()>{
    let validation=read_json(&profile.root.join("migration-validation.json"))?;
    if validation["integrity"]!="ok"||validation["resolutions_sha256"].as_str().is_none(){return Err("首次迁移结果尚未通过逐项核对".into());}crate::legacy::verify_sources(&validation)?;Ok(())
}

#[cfg(test)]mod tests;

pub fn activate(root:&Path,config:&Path)->Result<Value>{
    let _transaction=ProcessLock::acquire(&root.join("install.lock"))?;let path=root.join("install-transaction.json");let mut journal=read_json(&path)?;let desired=Profile::load(config)?;
    if journal["desired"]!=json!(desired)||journal["config"]!=json!(config){return Err("安装事务登记身份不匹配".into());}
    if journal["phase"]=="complete"{return Ok(journal);}
    if journal["phase"]!="healthy_paused"&&journal["phase"]!="activating"{return Err("安装事务尚未就绪".into());}
    let client=Client::new(desired);let health=client.call("identity",json!({}))?;
    if health["storage"]["status"]!="ok"||health["database_instance_id"]!=journal["health"]["database_instance_id"]{return Err("实际数据库验收未完成".into());}
    if journal["phase"]=="healthy_paused"{if health["gui_ready"]!=true||health["authenticated_requests"].as_u64().unwrap_or(0)==0{return Err("GUI或OAuth验收未完成，业务写入保持关闭".into());}journal["activation_health"]=health;save(&path,&mut journal,"activating")?;}
    client.call("resume",json!({"confirmed":true}))?;save(&path,&mut journal,"complete")?;atomic_json(&root.join("last-update.json"),&json!({"status":"complete","version":journal["version"],"message":"新后台、数据库、OAuth与看板校验通过"}))?;Ok(journal)
}
