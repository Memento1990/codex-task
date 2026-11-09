use std::{fs::{self,OpenOptions},io::Write,path::Path};
use serde_json::Value;
use crate::{Result,windows::wide};
pub fn read_json(path:&Path)->Result<Value>{serde_json::from_slice(&fs::read(path).map_err(|_|"文件不可读")?).map_err(|_|"JSON格式无效".into())}
pub fn atomic_json(path:&Path,value:&Value)->Result<()>{atomic_bytes(path,&serde_json::to_vec(value).map_err(|_|"JSON编码失败")?)}
pub fn atomic_bytes(path:&Path,bytes:&[u8])->Result<()>{
    let parent=path.parent().ok_or("文件路径无效")?;fs::create_dir_all(parent).map_err(|_|"目录不可写")?;
    let temp=parent.join(format!(".floatboard-{}.tmp",uuid::Uuid::new_v4()));
    let result=(||{let mut file=OpenOptions::new().create_new(true).write(true).open(&temp).map_err(|_|"临时文件不可写")?;
        file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|"文件提交失败")?;drop(file);
        let parent=fs::canonicalize(parent).map_err(|_|"目录不可解析")?;
        if unsafe{windows_sys::Win32::Storage::FileSystem::MoveFileExW(wide(parent.join(temp.file_name().unwrap())).as_ptr(),wide(parent.join(path.file_name().unwrap())).as_ptr(),0x1|0x8)}==0{return Err("文件原子提交失败".into());}Ok(())})();
    let _=fs::remove_file(temp);result
}
pub fn crypt(bytes:&[u8],encrypt:bool)->Result<Vec<u8>>{
    use windows_sys::Win32::{Security::Cryptography::*,Foundation::LocalFree};
    let input=CRYPT_INTEGER_BLOB{cbData:bytes.len().try_into().map_err(|_|"加密内容过大")?,pbData:bytes.as_ptr() as *mut u8};let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()};
    let ok=unsafe{if encrypt{CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}else{CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}};
    if ok==0{return Err("当前用户凭据加解密失败".into());}let data=unsafe{std::slice::from_raw_parts(output.pbData,output.cbData as usize)}.to_vec();unsafe{LocalFree(output.pbData.cast());}Ok(data)
}
