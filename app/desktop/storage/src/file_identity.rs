//! Keep the registered directory and main file pinned for the owner's lifetime.
use std::{fs::{File,OpenOptions},os::windows::{fs::{OpenOptionsExt,MetadataExt},io::AsRawHandle},path::{Path,PathBuf}};
use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle,BY_HANDLE_FILE_INFORMATION};
use crate::Result;
fn identity(file:&File)->Result<(u32,u32,u32)>{let mut info:BY_HANDLE_FILE_INFORMATION=unsafe{std::mem::zeroed()};if unsafe{GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err("文件身份不可读 [stage=file_identity.metadata]".into());}if info.nNumberOfLinks>1{return Err("数据库硬链接被拒绝 [stage=file_identity.links]".into());}Ok((info.dwVolumeSerialNumber,info.nFileIndexHigh,info.nFileIndexLow))}
pub fn check_path(path:&Path)->Result<()>{for part in path.ancestors(){match std::fs::symlink_metadata(part){Ok(m) if m.file_attributes()&0x400!=0=>return Err("数据路径包含重解析点 [stage=file_identity.reparse]".into()),Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err("数据路径不可验证 [stage=file_identity.path]".into())}}Ok(())}
pub struct FileGuard{file:File,path:PathBuf,_directory:File}
impl FileGuard{
    pub fn open(path:&Path,create:bool)->Result<Self>{
        check_path(path)?;let parent=path.parent().ok_or("数据库目录无效")?;
        for suffix in ["-wal","-shm"]{let sidecar=PathBuf::from(format!("{}{suffix}",path.display()));check_path(&sidecar)?;if sidecar.exists(){let file=OpenOptions::new().read(true).share_mode(7).open(sidecar).map_err(|_|"数据库侧文件不可验证 [stage=file_identity.sidecar]")?;identity(&file)?;}}
        let directory=OpenOptions::new().read(true).custom_flags(0x02000000).share_mode(3).open(parent).map_err(|_|"目录身份保护不可用 [stage=file_identity.directory]")?;
        if !path.exists()&&["-wal","-shm"].iter().any(|s|PathBuf::from(format!("{}{s}",path.display())).exists()){return Err("孤立WAL/SHM禁止创建空库 [stage=file_identity.orphan]".into());}
        let file=OpenOptions::new().read(true).write(create).create(create).truncate(false).share_mode(3).open(path).map_err(|_|"数据库身份保护不可用 [stage=file_identity.guard]")?;
        identity(&file)?;Ok(Self{file,path:path.into(),_directory:directory})
    }
    pub fn verify(&self)->Result<()>{check_path(&self.path)?;let file=OpenOptions::new().read(true).share_mode(3).open(&self.path).map_err(|_|"数据库路径不可读 [stage=file_identity.path]")?;if identity(&file)?!=identity(&self.file)?{return Err("数据库文件身份变化 [stage=file_identity.replaced]".into());}Ok(())}
}
