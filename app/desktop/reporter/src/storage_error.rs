//! Preserve fixed stages and SQLite codes, never SQL, paths or error payloads.
use serde_json::{json,Value};
pub fn sqlite(stage:&'static str,message:&'static str,error:rusqlite::Error)->String{
 let code=error.sqlite_error().map(|e|e.extended_code).unwrap_or(-1);
 format!("{message} [stage={stage} sqlite={} extended={code}]",if code<0{-1}else{code&255})
}
pub fn io(stage:&'static str,message:&'static str,error:std::io::Error)->String{
 format!("{message} [stage={stage} win32={}]",error.raw_os_error().unwrap_or(-1))
}
pub fn diagnostic(error:&str)->Value{
 let stage=error.split("stage=").nth(1).and_then(|s|s.split(|c:char|!(c.is_ascii_alphanumeric()||c=='.'||c=='_')).next()).filter(|s|!s.is_empty()&&s.len()<=64).unwrap_or("storage.unknown");
 let number=|key:&str|error.split(key).nth(1).and_then(|s|s.split(|c:char|!(c.is_ascii_digit()||c=='-')).next()).and_then(|s|s.parse::<i32>().ok());
 json!({"stage":stage,"sqlite_code":number(" sqlite="),"sqlite_extended_code":number(" extended="),"win32_code":number(" win32=")})
}
