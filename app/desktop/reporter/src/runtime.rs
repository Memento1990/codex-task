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
pub fn reporter_path()->Result<std::path::PathBuf>{Ok(std::env::current_exe().map_err(|_|"程序路径不可用")?.with_file_name("FloatboardReporter.exe"))}
pub fn spawn_worker(root:&Path)->Result<()>{
    if ProcessLock::acquire(&root.join("collector.lock")).is_err(){return Ok(());}
    Command::new(reporter_path()?).arg("--worker").arg("--data-dir").arg(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|_|"采集器无法启动")?;Ok(())
}
pub fn drain(root:&Path,store:&mut Store)->Result<(usize,usize)>{
    let inbox=root.join("inbox");fs::create_dir_all(&inbox).map_err(|_|"事件邮箱不可用")?;
    let mut files=fs::read_dir(&inbox).map_err(|_|"事件邮箱不可读")?.filter_map(|e|e.ok()).filter(|e|e.path().extension().is_some_and(|x|x=="json")).collect::<Vec<_>>();
    files.sort_by_key(|e|e.metadata().and_then(|m|m.modified()).ok());let (mut accepted,mut rejected)=(0,0);
    for file in files.into_iter().take(500){
        let path=file.path();let size=file.metadata().map_err(|_|"事件文件不可读")?.len();
        let checked=if size>1_000_000{Err("事件过大".into())}else{read_json(&path)};
        let result=checked.and_then(|event|store.ingest(&event));
        match result{
            Ok(_)=>{fs::remove_file(path).map_err(|_|"事件确认失败")?;accepted+=1;},
            Err(error) if error.contains("库")||error.contains("保存")||error.contains("不可读")=>return Err(error),
            Err(_)=>{let dir=root.join("rejected");fs::create_dir_all(&dir).map_err(|_|"异常事件目录不可用")?;fs::rename(&path,dir.join(file.file_name())).map_err(|_|"异常事件隔离失败")?;rejected+=1;}
        }
    }Ok((accepted,rejected))
}
pub fn worker(root:&Path)->Result<()>{
    let _lock=match ProcessLock::acquire(&root.join("collector.lock")){Ok(lock)=>lock,Err(_)=>return Ok(())};
    let mut store=Store::open(root,false)?;let mut rejected=fs::read_dir(root.join("rejected")).map(|items|items.flatten().filter(|e|e.path().extension().is_some_and(|x|x=="json")).count()).unwrap_or(0);
    loop{let state=match drain(root,&mut store){Ok((_,count))=>{rejected+=count;"ok"},Err(_)=>"storage_error"};let _=atomic_json(&root.join("health.json"),&json!({"pid":std::process::id(),"at":now(),"rejected":rejected,"status":state,"implementation":"rust"}));std::thread::sleep(Duration::from_millis(350));}
}
