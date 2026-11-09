use std::{fs::{File,OpenOptions},os::windows::{fs::OpenOptionsExt,io::AsRawHandle},path::Path};
use windows_sys::Win32::Storage::FileSystem::{LockFile,UnlockFile};
use crate::Result;
pub struct ProcessLock(File);
impl ProcessLock{pub fn acquire(path:&Path)->Result<Self>{std::fs::create_dir_all(path.parent().ok_or("锁路径无效")?).map_err(|_|"锁目录不可写")?;let file=OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(3).open(path).map_err(|_|"锁不可用")?;if unsafe{LockFile(file.as_raw_handle(),0,0,1,0)}==0{return Err("操作已在运行".into());}Ok(Self(file))}}
impl Drop for ProcessLock{fn drop(&mut self){unsafe{UnlockFile(self.0.as_raw_handle(),0,0,1,0);}}}
