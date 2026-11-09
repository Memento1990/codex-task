use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::{Path, PathBuf}};
use windows_sys::Win32::{Foundation::*, Security::{*, Authorization::*}, System::Threading::*};
use crate::Result;

pub fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {value.as_ref().encode_wide().chain(Some(0)).collect()}
pub struct Handle(pub HANDLE);
// Win32 kernel handles support concurrent queries; ownership closes exactly once.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {fn drop(&mut self){unsafe{CloseHandle(self.0);}}}

pub fn sid() -> Result<String> {
    unsafe {
        let mut raw=std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut raw)==0{return Err("用户身份不可读".into());}
        let token=Handle(raw);let mut n=0;
        GetTokenInformation(token.0,TokenUser,std::ptr::null_mut(),0,&mut n);
        let mut bytes=vec![0usize; (n as usize+7)/8];
        if GetTokenInformation(token.0,TokenUser,bytes.as_mut_ptr().cast(),n,&mut n)==0{return Err("用户SID不可读".into());}
        let user=&*(bytes.as_ptr() as *const TOKEN_USER);let mut text=std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid,&mut text)==0{return Err("用户SID无效".into());}
        let mut len=0;while *text.add(len)!=0{len+=1;}
        let value=String::from_utf16_lossy(std::slice::from_raw_parts(text,len));LocalFree(text.cast());Ok(value)
    }
}

pub fn login_id()->Result<String>{unsafe{
    let mut raw=std::ptr::null_mut();if OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut raw)==0{return Err("登录身份不可读".into());}
    let token=Handle(raw);let mut stats:TOKEN_STATISTICS=std::mem::zeroed();let mut n=0;
    if GetTokenInformation(token.0,TokenStatistics,(&mut stats as *mut TOKEN_STATISTICS).cast(),std::mem::size_of::<TOKEN_STATISTICS>() as u32,&mut n)==0{return Err("登录会话不可读".into());}
    Ok(format!("{:08x}-{:08x}",stats.AuthenticationId.HighPart,stats.AuthenticationId.LowPart))
}}

pub struct Security {descriptor: *mut c_void, pub attributes: SECURITY_ATTRIBUTES}
impl Security {
    pub fn user() -> Result<Self> {
        let sddl=wide(format!("D:P(A;OICI;GA;;;{})",sid()?));let mut descriptor=std::ptr::null_mut();
        if unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut descriptor,std::ptr::null_mut())}==0{return Err("安全描述符创建失败".into());}
        Ok(Self{descriptor,attributes:SECURITY_ATTRIBUTES{nLength:std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,lpSecurityDescriptor:descriptor,bInheritHandle:0}})
    }
    pub fn protect(path:&Path)->Result<()> {
        let security=Self::user()?;let mut present=0;let mut defaulted=0;let mut acl=std::ptr::null_mut();
        if unsafe{GetSecurityDescriptorDacl(security.descriptor,&mut present,&mut acl,&mut defaulted)}==0{return Err("目录ACL读取失败".into());}
        let status=unsafe{SetNamedSecurityInfoW(wide(path).as_ptr(),SE_FILE_OBJECT,DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,std::ptr::null_mut(),std::ptr::null_mut(),acl,std::ptr::null())};
        if status!=0{return Err("目录ACL设置失败".into());}Ok(())
    }
}
impl Drop for Security{fn drop(&mut self){unsafe{LocalFree(self.descriptor);}}}

pub struct Instance{_handle:Handle}
impl Instance {
    pub fn acquire(profile:&str)->Result<Self>{
        uuid::Uuid::parse_str(profile).map_err(|_|"profile UUID无效")?;
        Self::acquire_named(profile)
    }
    pub fn acquire_named(name:&str)->Result<Self>{
        let mut security=Security::user()?;let name=wide(format!("Global\\CodexFloatboard.{}.{}",sid()?,name));
        let handle=unsafe{CreateMutexW(&mut security.attributes,0,name.as_ptr())};
        if handle.is_null(){return Err("Agent互斥对象不可用".into());}
        let existed=unsafe{GetLastError()}==ERROR_ALREADY_EXISTS;let handle=Handle(handle);
        if existed{return Err("此profile的Agent或维护工具已在运行".into());}Ok(Self{_handle:handle})
    }
}


pub fn user_home()->Result<PathBuf>{known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Profile)}
pub fn local_home()->Result<PathBuf>{known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_LocalAppData)}
fn known_folder(id:&windows_sys::core::GUID)->Result<PathBuf>{
    use windows_sys::Win32::{UI::Shell::{SHGetKnownFolderPath},System::Com::CoTaskMemFree};
    let mut text=std::ptr::null_mut();
    if unsafe{SHGetKnownFolderPath(id,0,std::ptr::null_mut(),&mut text)}<0{return Err("用户目录不可读".into());}
    let mut len=0;unsafe{while *text.add(len)!=0{len+=1;}}
    let path=PathBuf::from(String::from_utf16_lossy(unsafe{std::slice::from_raw_parts(text,len)}));unsafe{CoTaskMemFree(text.cast());}Ok(path)
}

pub fn process_image(pid:u32)->Result<PathBuf>{
    let raw=unsafe{OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid)};
    if raw.is_null(){return Err("Agent进程不可验证".into());}let process=Handle(raw);let mut name=vec![0u16;32768];let mut n=name.len() as u32;
    if unsafe{QueryFullProcessImageNameW(process.0,0,name.as_mut_ptr(),&mut n)}==0{return Err("Agent程序身份不可读".into());}
    Ok(PathBuf::from(String::from_utf16_lossy(&name[..n as usize])))
}

pub fn check_path(path:&Path)->Result<()>{
    use std::os::windows::fs::MetadataExt;
    if !path.is_absolute()||path.components().any(|c|matches!(c,std::path::Component::ParentDir|std::path::Component::CurDir)){return Err("路径必须是无跳转的绝对路径".into());}
    for part in path.ancestors(){match std::fs::symlink_metadata(part){Ok(m) if m.file_attributes()&0x400!=0=>return Err("受管路径包含重解析点".into()),Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err("受管路径不可验证".into())}}
    Ok(())
}
