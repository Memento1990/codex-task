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
    if ProcessLock::acquire(&root.join("collector.lock")).is_err(){return Ok(());}
    // During handoff the original worker owns origin locks and will activate
    // this root itself. Do not race it with a newly spawned destination worker.
    if let Ok(origins)=read_json(&root.join("migration-origins.json")) {
        if let Some(roots)=origins["roots"].as_array(){for old in roots {
            let old=Path::new(old.as_str().ok_or("旧数据目录记录无效")?);
            crate::migration::check_path(old)?;
            if ProcessLock::acquire(&old.join("collector.lock")).is_err(){return Ok(());}
        }}
    }
    Command::new(reporter_path()?).arg("--worker").arg("--data-dir").arg(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"采集器无法启动")?;Ok(())
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
pub fn worker(root:&Path)->Result<()>{worker_with_locks(root,Vec::new())}
fn worker_with_locks(root:&Path,mut retained:Vec<(std::path::PathBuf,ProcessLock)>)->Result<()>{
    let resolved=crate::migration::resolve_root(root)?; let root=resolved.as_path();
    if !retained.iter().any(|(path,_)|path==root){
        let deadline=std::time::Instant::now()+Duration::from_secs(5);
        loop {
            match ProcessLock::acquire(&root.join("collector.lock")){
                Ok(lock)=>{retained.push((root.to_path_buf(),lock));break;},
                Err(_) if retained.is_empty()=>return Ok(()),
                Err(_) if std::time::Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(20)),
                Err(_)=>return Err("迁移目标采集锁被占用，旧采集身份仍受保护".into()),
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
    let mut store=Store::open(root,false)?;let mut rejected=fs::read_dir(root.join("rejected")).map(|items|items.flatten().filter(|e|e.path().extension().is_some_and(|x|x=="json")).count()).unwrap_or(0);
    loop{
        if root.join("worker-shutdown.json").exists(){let _=fs::remove_file(root.join("worker-shutdown.json"));return Ok(());}
        // Pause SQLite, but retain every collector lock. The same worker
        // activates the destination without any old-collector ownership gap.
        if root.join(crate::migration::STOP).exists() {
            drop(store);
            loop {
                let stop=read_json(&root.join(crate::migration::STOP));
                if let Ok(stop)=stop {
                    atomic_json(&root.join(crate::migration::ACK),&json!({"nonce":stop["nonce"],"pid":std::process::id()}))?;
                }
                if let Ok(_migration)=ProcessLock::acquire(&root.join(crate::migration::LOCK)) {
                    // Migration either committed or its owner crashed/returned.
                    // Only while owning the lock may stale STOP be removed.
                    let current=crate::migration::resolve_root(root)?;
                    let _=fs::remove_file(root.join(crate::migration::STOP));
                    let _=fs::remove_file(root.join(crate::migration::ACK));
                    if current!=root {drop(_migration);return worker_with_locks(&current,retained);}
                    store=Store::open(root,false)?;
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let current=crate::migration::resolve_root(root)?;
        if current!=root {drop(store);return worker_with_locks(&current,retained);}
        if let Ok(_migration)=ProcessLock::acquire(&root.join(crate::migration::LOCK)) {
            if root.join(crate::migration::STOP).exists(){continue;}
            let result=(||{
                let (_,mut count)=drain(root,&mut store)?;
                if let Ok(origins)=read_json(&root.join("migration-origins.json")) {
                    if let Some(roots)=origins["roots"].as_array(){for old in roots {
                        let old=Path::new(old.as_str().ok_or("旧数据目录记录无效")?);
                        crate::migration::check_path(old)?;
                        // Only consume origins whose redirect still resolves here.
                        if crate::migration::resolve_root(old)?==root {
                            if let Ok(_old_migration)=ProcessLock::acquire(&old.join(crate::migration::LOCK)) {
                                crate::migration::check_path(&old.join("inbox"))?;
                                count+=drain(old,&mut store)?.1;
                            }
                        }
                    }}
                }
                Ok::<usize,String>(count)
            })();
            let state=match result{Ok(count)=>{rejected+=count;"ok"},Err(_)=>"storage_error"};
            let _=atomic_json(&root.join("health.json"),&json!({"pid":std::process::id(),"at":now(),"rejected":rejected,"status":state,"implementation":"rust","migration_protocol":1}));
        }
        std::thread::sleep(Duration::from_millis(350));
    }
}
