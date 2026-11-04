use std::{fs::{self,File,OpenOptions}, path::Path, process::{Command,Stdio}, io::{Seek,SeekFrom,Write}, time::Duration};
use std::os::windows::{io::AsRawHandle,process::CommandExt};
use windows_sys::Win32::Storage::FileSystem::{LockFile,UnlockFile};
use serde_json::json;
use crate::{Result,now,store::{Store,atomic_json,read_json}};

pub struct ProcessLock(File);
impl ProcessLock {
    pub fn acquire(path:&Path)->Result<Self>{
        fs::create_dir_all(path.parent().ok_or("进程锁路径无效")?).map_err(|_|"进程锁目录不可写")?;
        let mut file=OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(|_|"进程锁不可用")?;
        file.seek(SeekFrom::Start(0)).and_then(|_|file.write_all(b"0")).and_then(|_|file.flush()).map_err(|_|"进程锁不可写")?;
        if unsafe{LockFile(file.as_raw_handle(),0,0,1,0)}==0{return Err("进程已在运行".into());}Ok(Self(file))
    }
}
impl Drop for ProcessLock{fn drop(&mut self){unsafe{UnlockFile(self.0.as_raw_handle(),0,0,1,0);}}}
pub fn reporter_path()->Result<std::path::PathBuf>{
    let exe=std::env::current_exe().map_err(|_|"程序路径不可用")?;
    let sibling=exe.with_file_name("FloatboardReporter.exe");
    // Cargo integration tests live in target/debug/deps; use the binary Cargo
    // built for the same profile, never a globally installed reporter.
    if exe.parent().is_some_and(|p|p.file_name().is_some_and(|n|n=="deps")) && !sibling.exists(){return Ok(exe.parent().unwrap().parent().ok_or("程序路径不可用")?.join("FloatboardReporter.exe"));}
    Ok(sibling)
}
pub fn spawn_worker(root:&Path)->Result<()>{
    let resolved=crate::migration::resolve_root(root)?; let root=resolved.as_path();
    if root.join(crate::migration::STOP).exists(){
        if let Ok(_migration)=ProcessLock::acquire(&root.join(crate::migration::LOCK)) {
            let _=fs::remove_file(root.join(crate::migration::STOP));
            let _=fs::remove_file(root.join(crate::migration::ACK));
        }else{return Err("采集器正在安全停机迁移".into());}
    }
    // Upgrade only the background collector, never a Hook/MCP session process.
    // The legacy collector already supports this cooperative shutdown request;
    // queued inbox events survive the handoff. Serialize competing new launchers.
    let _handoff=ProcessLock::acquire(&root.join("collector-upgrade.lock"))?;
    let mut upgraded=false;
    if ProcessLock::acquire(&root.join("collector.lock")).is_err(){
        // Unknown locks may belong to a migration/test or an initializing worker.
        // Never request shutdown without a recent observed collector heartbeat.
        let Ok(health)=read_json(&root.join("health.json"))else{return Ok(());};
        if health["at"].as_f64().is_none_or(|at|now()-at>5.){return Ok(());}
        if health["version"]==env!("CARGO_PKG_VERSION"){return Ok(());}
        // Do not let an older resident Hook downgrade a newer collector.
        if health["version"].as_str().and_then(|s|semver::Version::parse(s).ok()).is_some_and(|v|v>semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap()){return Ok(());}
        atomic_json(&root.join("worker-shutdown.json"),&json!({"reason":"collector_upgrade","version":env!("CARGO_PKG_VERSION")}))?;
        let deadline=std::time::Instant::now()+Duration::from_secs(8);
        loop {
            if let Ok(_released)=ProcessLock::acquire(&root.join("collector.lock")){
                // Keep the request until the target collector owns its lock:
                // legacy launchers do not honor collector-upgrade.lock and may race.
                upgraded=true;break;
            }
            if std::time::Instant::now()>=deadline{return Err("旧采集器未确认安全交接，请稍后重试；会话未停止".into());}
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    // During handoff the original worker owns origin locks and will activate
    // this root itself. Do not race it with a newly spawned destination worker.
    if let Ok(origins)=read_json(&root.join("migration-origins.json")) {
        if let Some(roots)=origins["roots"].as_array(){for old in roots {
            let old=Path::new(old.as_str().ok_or("旧数据目录记录无效")?);
            crate::migration::check_path(old)?;
            if ProcessLock::acquire(&old.join("collector.lock")).is_err(){return Ok(());}
        }}
    }
    let launch=||Command::new(reporter_path()?).arg("--worker").arg("--data-dir").arg(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"采集器无法启动".to_string());
    let mut child=launch()?;
    if !upgraded{return Ok(());}
    let deadline=std::time::Instant::now()+Duration::from_secs(8);
    loop {
        if read_json(&root.join("health.json")).is_ok_and(|h|h["pid"]==child.id()&&h["version"]==env!("CARGO_PKG_VERSION")&&h["status"]=="ok"){return Ok(());}
        if child.try_wait().map_err(|_|"采集器状态不可用")?.is_some(){
            if std::time::Instant::now()>=deadline{return Err("采集器启动失败，事件仍保留在邮箱中".into());}
            atomic_json(&root.join("worker-shutdown.json"),&json!({"reason":"collector_upgrade","version":env!("CARGO_PKG_VERSION")}))?;
            std::thread::sleep(Duration::from_millis(375));
            child=launch()?;
        }
        if std::time::Instant::now()>=deadline{return Err("采集器尚未确认就绪，请稍后重试".into());}
        std::thread::sleep(Duration::from_millis(25));
    }
}
pub fn drain(root:&Path,store:&mut Store)->Result<(usize,usize)>{
    let inbox=root.join("inbox");crate::migration::check_path(&inbox)?;fs::create_dir_all(&inbox).map_err(|_|"事件邮箱不可用")?;
    let mut files=fs::read_dir(&inbox).map_err(|_|"事件邮箱不可读")?.filter_map(|e|e.ok()).filter(|e|e.path().extension().is_some_and(|x|x=="json")).collect::<Vec<_>>();
    files.sort_by_key(|e|e.metadata().and_then(|m|m.modified()).ok());let (mut accepted,mut rejected)=(0,0);
    for file in files.into_iter().take(500){
        let path=file.path();let size=file.metadata().map_err(|_|"事件文件不可读")?.len();
        let checked=if size>1_000_000{Err("事件过大".into())}else{read_json(&path)};
        let result=checked.and_then(|event|store.ingest(&event));
        match result{
            Ok(_)=>{if let Ok(event)=read_json(&path){if event["kind"]=="progress" && event["connection_generation"]==crate::bridge::generation(root) && event["reporter_version"]==env!("CARGO_PKG_VERSION"){let _=crate::bridge::observe_event(root,"consumed",event["session_id"].as_str(),event["event_id"].as_str());}}fs::remove_file(path).map_err(|_|"事件确认失败")?;accepted+=1;},
            Err(error) if error.contains("库")||error.contains("保存")||error.contains("不可读")=>return Err(error),
            Err(_)=>{let dir=root.join("rejected");fs::create_dir_all(&dir).map_err(|_|"异常事件目录不可用")?;fs::rename(&path,dir.join(file.file_name())).map_err(|_|"异常事件隔离失败")?;rejected+=1;}
        }
    }Ok((accepted,rejected))
}
pub const REPAIR_PROTOCOL:u64=1;

/// Proof that the identified collector has closed SQLite while retaining every
/// historical collector identity. Not a proof against arbitrary direct writers.
pub struct RepairPause {
    root:std::path::PathBuf,
    nonce:String,
    identity:serde_json::Value,
}
impl RepairPause {
    pub fn root(&self)->&Path{&self.root}
    pub fn nonce(&self)->&str{&self.nonce}
    pub fn verify(&self)->Result<()>{
        let stop=read_json(&self.root.join(crate::migration::STOP))?;
        if stop["nonce"]!=self.nonce || stop["instance"]!=self.identity["instance"] || stop["repair_protocol"]!=REPAIR_PROTOCOL {return Err("采集器修复请求已变化，未切换数据".into());}
        let ack=read_json(&self.root.join(crate::migration::ACK))?;
        if ack["nonce"]!=self.nonce || ack["repair_protocol"]!=REPAIR_PROTOCOL || ack["pid"]!=self.identity["pid"] || ack["instance"]!=self.identity["instance"] || ack["roots"]!=self.identity["roots"] || ack["instance_lock"]!=self.identity["instance_lock"] || !fresh(&ack) {
            return Err("采集器修复暂停凭据已失效，未切换数据".into());
        }
        check_collector_identity(&self.root,&ack)
    }
}
fn fresh(value:&serde_json::Value)->bool{value["at"].as_f64().is_some_and(|at|at.is_finite()&&(0.0..=2.0).contains(&(now()-at)))}
fn check_collector_identity(root:&Path,value:&serde_json::Value)->Result<()>{
    let instance=value["instance"].as_str().ok_or("采集器缺少修复身份")?;
    uuid::Uuid::parse_str(instance).map_err(|_|"采集器修复身份无效")?;
    let lock=std::path::PathBuf::from(value["instance_lock"].as_str().ok_or("采集器修复身份无效")?);
    crate::migration::check_path(&lock)?;
    let roots=value["roots"].as_array().ok_or("采集器旧目录身份无效")?;
    if !roots.iter().any(|p|p.as_str().is_some_and(|p|Path::new(p)==root)) || value["pid"].as_u64().is_none_or(|pid|pid==0||pid>u32::MAX as u64) {
        return Err("采集器修复身份无效".into());
    }
    if lock.file_name().is_none_or(|name|name!=std::ffi::OsStr::new(&format!("collector-instance-{instance}.lock"))) || !roots.iter().any(|p|p.as_str().is_some_and(|p|Some(Path::new(p))==lock.parent())) {
        return Err("采集器修复身份锁无效".into());
    }
    let origins_path=root.join("migration-origins.json");
    if origins_path.exists(){
        let origins=read_json(&origins_path)?;
        for origin in origins["roots"].as_array().ok_or("旧数据目录记录无效")? {
            let path=Path::new(origin.as_str().ok_or("旧数据目录记录无效")?);
            crate::migration::check_path(path)?;
            if crate::migration::resolve_root(path)?!=root || !roots.iter().any(|p|p.as_str().is_some_and(|p|Path::new(p)==path)){return Err("采集器未保护全部旧数据目录".into());}
        }
    }
    for path in roots {
        let path=Path::new(path.as_str().ok_or("采集器旧目录身份无效")?);
        crate::migration::check_path(path)?;
        if crate::migration::resolve_root(path)?!=root || ProcessLock::acquire(&path.join("collector.lock")).is_ok(){return Err("采集器旧目录保护已失效".into());}
    }
    if ProcessLock::acquire(&lock).is_ok(){return Err("采集器已退出，未切换数据".into());}
    Ok(())
}
struct RepairStop(std::path::PathBuf,String);
impl Drop for RepairStop{fn drop(&mut self){
    if read_json(&self.0.join(crate::migration::STOP)).is_ok_and(|v|v["nonce"]==self.1){let _=fs::remove_file(self.0.join(crate::migration::STOP));}
}}
/// Caller must close its own Store and separately quiesce notifications and
/// integration before entry. The callback may commit a redirect only after
/// `pause.verify()`. Never upgrades/stops an occupied legacy collector.
pub fn with_repair_pause<T>(root:&Path, action:impl FnOnce(&RepairPause)->Result<T>)->Result<T>{
    let root=crate::migration::resolve_root(root)?;
    let _upgrade=ProcessLock::acquire(&root.join("collector-upgrade.lock")).map_err(|_|"采集器正在交接，请稍后修复")?;
    let _migration=ProcessLock::acquire(&root.join(crate::migration::LOCK)).map_err(|_|"数据目录正在操作，请稍后修复")?;
    if root.join(crate::migration::STOP).exists(){return Err("已有采集器暂停请求，请稍后修复".into());}
    if let Ok(vacant)=ProcessLock::acquire(&root.join("collector.lock")) {
        // Start only our current reporter. Never send a shutdown request to an
        // existing legacy owner or allow spawn_worker to upgrade it.
        drop(vacant);
        Command::new(reporter_path()?).arg("--worker").arg("--data-dir").arg(&root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"修复采集器无法启动")?;
    }
    let deadline=std::time::Instant::now()+Duration::from_secs(5);
    let identity=loop {
        if let Ok(h)=read_json(&root.join("health.json")) {
            if fresh(&h) && h["repair_protocol"]==REPAIR_PROTOCOL && check_collector_identity(&root,&h).is_ok(){break h;}
        }
        if std::time::Instant::now()>=deadline{return Err("采集器不支持安全修复或未就绪；请等待旧会话正常结束后重试".into());}
        std::thread::sleep(Duration::from_millis(20));
    };
    let nonce=uuid::Uuid::new_v4().to_string();
    atomic_json(&root.join(crate::migration::STOP),&json!({"version":1,"nonce":nonce,"at":now(),"repair_protocol":REPAIR_PROTOCOL,"instance":identity["instance"]}))?;
    let _stop=RepairStop(root.clone(),nonce.clone());
    let pause=RepairPause{root,nonce,identity};
    let pause_deadline=std::time::Instant::now()+Duration::from_secs(5);
    loop {
        if pause.verify().is_ok(){break;}
        if std::time::Instant::now()>=pause_deadline{return Err("采集器未确认安全暂停，未切换数据".into());}
        std::thread::sleep(Duration::from_millis(20));
    }
    action(&pause)
}

pub fn worker(root:&Path)->Result<()>{
    let root=crate::migration::resolve_root(root)?;
    let instance=uuid::Uuid::new_v4().to_string();
    // Unlike a PID, this lock cannot identify a later recycled process. Keep it
    // alive outside recursive redirects for the entire worker lifetime.
    let instance_path=root.join(format!("collector-instance-{instance}.lock"));
    let _instance=ProcessLock::acquire(&instance_path)?;
    worker_with_locks(&root,Vec::new(),&instance,&instance_path)
}
fn worker_with_locks(root:&Path,mut retained:Vec<(std::path::PathBuf,ProcessLock)>,instance:&str,instance_path:&Path)->Result<()>{
    let resolved=crate::migration::resolve_root(root)?; let root=resolved.as_path();
    if !retained.iter().any(|(path,_)|path==root){
        loop {
            match ProcessLock::acquire(&root.join("collector.lock")){
                Ok(lock)=>{retained.push((root.to_path_buf(),lock));break;},
                Err(_) if retained.is_empty()=>return Ok(()),
                // Never surrender origin identities after a committed redirect,
                // even if destination activation is delayed by another owner.
                Err(_)=>{
                    for (old,_) in &retained{let _=atomic_json(&old.join("health.json"),&json!({"pid":std::process::id(),"at":now(),"status":"activation_blocked","repair_protocol":REPAIR_PROTOCOL,"instance":instance}));}
                    std::thread::sleep(Duration::from_millis(100));
                },
            }
        }
    }
    // Reserve old collector identities for this worker's entire lifetime. Old
    // reporter binaries probe these locks before spawning and cannot reopen a
    // second collector against a retained pre-migration database.

    if let Ok(origins)=read_json(&root.join("migration-origins.json")) {
        if let Some(roots)=origins["roots"].as_array(){for old in roots {
            let old=Path::new(old.as_str().ok_or("旧数据目录记录无效")?);
            crate::migration::check_path(old)?;
            if crate::migration::resolve_root(old)?==root && !retained.iter().any(|(path,_)|path==old) {
                retained.push((old.to_path_buf(),ProcessLock::acquire(&old.join("collector.lock")).map_err(|_| "旧目录采集器仍在运行，请退出旧版本后重启")?));
            }
        }}
    }
    // Only the requested replacement acknowledges an upgrade request, after
    // obtaining collector ownership. A racing legacy collector exits normally.
    if read_json(&root.join("worker-shutdown.json")).is_ok_and(|v|v["reason"]=="collector_upgrade"&&v["version"]==env!("CARGO_PKG_VERSION")){
        fs::remove_file(root.join("worker-shutdown.json")).map_err(|_|"采集器交接确认失败")?;
    }
    // An unreadable Store must not terminate the owner: repairs need a live
    // collector to close SQLite and reserve historical collector identities.
    let mut store=None;let mut rejected=fs::read_dir(root.join("rejected")).map(|items|items.flatten().filter(|e|e.path().extension().is_some_and(|x|x=="json")).count()).unwrap_or(0);
    atomic_json(&root.join("health.json"),&json!({"pid":std::process::id(),"at":now(),"status":"starting","implementation":"rust","version":env!("CARGO_PKG_VERSION"),"migration_protocol":1,"repair_protocol":REPAIR_PROTOCOL,"instance":instance,"instance_lock":instance_path,"roots":retained.iter().map(|(path,_)|path).collect::<Vec<_>>() }))?;
    loop{
        if root.join("worker-shutdown.json").exists(){let _=fs::remove_file(root.join("worker-shutdown.json"));return Ok(());}
        // Pause SQLite, but retain every collector lock. The same worker
        // activates the destination without any old-collector ownership gap.
        if root.join(crate::migration::STOP).exists() {
            store=None;
            loop {
                let stop=read_json(&root.join(crate::migration::STOP));
                if let Ok(stop)=stop {
                    atomic_json(&root.join(crate::migration::ACK),&json!({"nonce":stop["nonce"],"pid":std::process::id(),"repair_protocol":REPAIR_PROTOCOL,"instance":instance,"instance_lock":instance_path,"roots":retained.iter().map(|(path,_)|path).collect::<Vec<_>>(),"at":now()}))?;
                }
                if let Ok(_migration)=ProcessLock::acquire(&root.join(crate::migration::LOCK)) {
                    // Migration either committed or its owner crashed/returned.
                    // Only while owning the lock may stale STOP be removed.
                    let current=crate::migration::resolve_root(root)?;
                    let _=fs::remove_file(root.join(crate::migration::STOP));
                    let _=fs::remove_file(root.join(crate::migration::ACK));
                    if current!=root {drop(_migration);return worker_with_locks(&current,retained,instance,instance_path);}
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let current=crate::migration::resolve_root(root)?;
        if current!=root {drop(store);return worker_with_locks(&current,retained,instance,instance_path);}
        if let Ok(_migration)=ProcessLock::acquire(&root.join(crate::migration::LOCK)) {
            if root.join(crate::migration::STOP).exists(){continue;}
            let result=(||{
                if store.is_none(){store=Some(Store::open(root,false)?);}
                let store=store.as_mut().ok_or("任务库不可用")?;
                // Opening SQLite can succeed while task pages remain corrupt.
                store.tasks()?;
                let (_,mut count)=drain(root,store)?;
                if let Ok(origins)=read_json(&root.join("migration-origins.json")) {
                    if let Some(roots)=origins["roots"].as_array(){for old in roots {
                        let old=Path::new(old.as_str().ok_or("旧数据目录记录无效")?);
                        crate::migration::check_path(old)?;
                        // Only consume origins whose redirect still resolves here.
                        if crate::migration::resolve_root(old)?==root {
                            if let Ok(_old_migration)=ProcessLock::acquire(&old.join(crate::migration::LOCK)) {
                                crate::migration::check_path(&old.join("inbox"))?;
                                count+=drain(old,store)?.1;
                            }
                        }
                    }}
                }
                Ok::<usize,String>(count)
            })();
            let state=match result{Ok(count)=>{rejected+=count;"ok"},Err(_)=>{store=None;"storage_error"}};
            let _=atomic_json(&root.join("health.json"),&json!({"pid":std::process::id(),"at":now(),"rejected":rejected,"status":state,"implementation":"rust","version":env!("CARGO_PKG_VERSION"),"migration_protocol":1,"repair_protocol":REPAIR_PROTOCOL,"instance":instance,"instance_lock":instance_path,"roots":retained.iter().map(|(path,_)|path).collect::<Vec<_>>() }));
        }
        std::thread::sleep(Duration::from_millis(350));
    }
}
