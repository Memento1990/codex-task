//! Detect resident Reporters whose handles refer to replaced files at our paths.
//! Only handle metadata is queried; no foreign file bytes or locks are touched.
use std::{collections::{HashMap,HashSet},ffi::c_void,fs::{File,OpenOptions},os::windows::{fs::OpenOptionsExt,io::AsRawHandle},path::{Path,PathBuf}};
use serde::Serialize;
use windows_sys::Win32::{Foundation::{CloseHandle,DuplicateHandle,HANDLE,INVALID_HANDLE_VALUE},Storage::FileSystem::{GetFileInformationByHandle,GetFileType,GetFinalPathNameByHandleW,BY_HANDLE_FILE_INFORMATION,FILE_TYPE_DISK},System::Threading::{GetCurrentProcess,OpenProcess,QueryFullProcessImageNameW,PROCESS_DUP_HANDLE,PROCESS_QUERY_LIMITED_INFORMATION}};
#[link(name="ntdll")]
extern "system"{fn NtQuerySystemInformation(class:u32,buffer:*mut c_void,length:u32,needed:*mut u32)->i32;}
struct Handle(HANDLE);
impl Drop for Handle{fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
#[derive(Clone,Copy,PartialEq,Eq)]
struct Identity(u32,u32,u32);
fn identity(file:HANDLE)->crate::Result<Identity>{let mut info:BY_HANDLE_FILE_INFORMATION=unsafe{std::mem::zeroed()};if unsafe{GetFileInformationByHandle(file,&mut info)}==0{return Err("文件身份读取失败（stage=file_identity.metadata）".into());}Ok(Identity(info.dwVolumeSerialNumber,info.nFileIndexHigh,info.nFileIndexLow))}
fn normalized(path:&Path)->String{path.to_string_lossy().trim_start_matches(r"\\?\").replace('/',"\\").to_lowercase()}
fn handle_path(file:HANDLE)->Option<PathBuf>{let mut path=vec![0u16;32768];let n=unsafe{GetFinalPathNameByHandleW(file,path.as_mut_ptr(),path.len() as u32,8)};if n==0||n as usize>=path.len(){None}else{Some(PathBuf::from(String::from_utf16_lossy(&path[..n as usize])))}}
pub struct FileGuard{file:File,path:PathBuf}
impl FileGuard{
 pub fn open(path:&Path,create:bool)->crate::Result<Self>{
  let file=OpenOptions::new().read(true).write(create).create(create).truncate(false).share_mode(3).open(path).map_err(|_|"文件身份保护不可用（stage=file_identity.guard）")?;
  Ok(Self{file,path:path.into()})
 }
 pub fn verify(&self)->crate::Result<()>{
  verify_file(&self.file,&self.path)
 }
}
pub(crate) fn verify_file(file:&File,path:&Path)->crate::Result<()>{
 let current=OpenOptions::new().access_mode(0x80).share_mode(7).open(path).map_err(|_|"文件身份已变化（stage=file_identity.path）")?;
 if identity(file.as_raw_handle())?!=identity(current.as_raw_handle())?{return Err("文件身份已变化（stage=file_identity.replaced）".into());}Ok(())
}
#[derive(Serialize)]
pub struct Conflict{pub pid:u32,pub file:&'static str}
pub fn split_owners(root:&Path)->crate::Result<Vec<Conflict>>{
 let root=std::fs::canonicalize(root).map_err(|_|"文件身份目录不可用（stage=file_identity.root）")?;
 let mut expected=HashMap::new();for name in ["collector.lock","events.sqlite3"]{
  let path=root.join(name);match OpenOptions::new().access_mode(0x80).share_mode(7).open(&path){
   Ok(file)=>{expected.insert(normalized(&path),(name,identity(file.as_raw_handle())?));},
   Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
   Err(_)=>return Err("文件身份路径不可读（stage=file_identity.path）".into()),
  }
 }
 if expected.is_empty()&&!root.join("events.sqlite3-wal").exists()&&!root.join("events.sqlite3-shm").exists(){return Ok(vec![]);}
 let mut bytes=vec![0u64;131072];
 loop{let mut needed=0;let status=unsafe{NtQuerySystemInformation(64,bytes.as_mut_ptr().cast(),(bytes.len()*8) as u32,&mut needed)};if status==0{break;}if status!=0xC0000004u32 as i32||bytes.len()*8>=128*1024*1024{return Err("文件身份检查不可用（stage=file_identity.inventory）".into());}bytes.resize((bytes.len()*2).max(needed as usize/8+8192),0);}
 let count=bytes[0] as usize;if count>(bytes.len()*8-16)/40{return Err("文件身份检查无效（stage=file_identity.inventory）".into());}
 let mut processes:HashMap<usize,Option<Handle>>=HashMap::new();let mut conflicts=vec![];let mut seen=HashSet::new();let mut detached=HashSet::new();let mut shared_sidecars=HashSet::new();let mut current_owners:HashMap<usize,u8>=HashMap::new();
 for index in 0..count{
  let offset=2+index*5;let pid=bytes[offset+1] as usize;
  let process=processes.entry(pid).or_insert_with(||{
   let handle=unsafe{OpenProcess(PROCESS_DUP_HANDLE|PROCESS_QUERY_LIMITED_INFORMATION,0,pid as u32)};if handle.is_null(){return None;}let handle=Handle(handle);
   if pid!=std::process::id() as usize{let mut name=vec![0u16;32768];let mut n=name.len() as u32;if unsafe{QueryFullProcessImageNameW(handle.0,0,name.as_mut_ptr(),&mut n)}==0{return None;}let path=String::from_utf16_lossy(&name[..n as usize]).to_lowercase();if !path.ends_with("\\floatboardreporter.exe"){return None;}}
   Some(handle)
  });let Some(process)=process else{continue;};
  let mut copy=std::ptr::null_mut();if unsafe{DuplicateHandle(process.0,bytes[offset+2] as HANDLE,GetCurrentProcess(),&mut copy,0,0,2)}==0{continue;}let copy=Handle(copy);if copy.0==INVALID_HANDLE_VALUE||unsafe{GetFileType(copy.0)}!=FILE_TYPE_DISK{continue;}
  let Some(path)=handle_path(copy.0)else{continue;};let path=normalized(&path);
  if path.contains("\\$extend\\$deleted\\")||["events.sqlite3.","collector.lock."].iter().any(|prefix|path.starts_with(&normalized(&root.join(prefix)))){detached.insert(pid);}
  if ["events.sqlite3-wal","events.sqlite3-shm"].iter().any(|name|path==normalized(&root.join(name))){shared_sidecars.insert(pid);}
  let Some((name,current))=expected.get(&path)else{continue;};
  if identity(copy.0)?!=*current{if seen.insert((pid,*name)){conflicts.push(Conflict{pid:pid as u32,file:name});}}
  else{*current_owners.entry(pid).or_default()|=if *name=="events.sqlite3"{1}else{2};}
 }
 // Unlink erases the original name; backup renames leave a suffixed filename.
 // A resident Reporter
 // retaining detached disk objects alongside our SQLite sidecars without both
 // current main/collector identities is not a provable single owner. An intact
 // pair also excludes unrelated deleted temporary files in that same process.
 for pid in detached.intersection(&shared_sidecars){if current_owners.get(pid)!=Some(&3){conflicts.push(Conflict{pid:*pid as u32,file:"detached_database_handle"});}}
 Ok(conflicts)
}
pub fn ensure_no_split(root:&Path)->crate::Result<()>{if split_owners(root)?.is_empty(){Ok(())}else{Err("任务数据文件身份冲突，请先保存工作并正常重启 Windows；未打开数据库（stage=file_identity.split）".into())}}
