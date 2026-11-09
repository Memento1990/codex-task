use std::{fs,path::{Path,PathBuf}};
use serde::{Deserialize,Serialize};
use crate::{Result,windows};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub profile_id:String,
    pub environment:String,
    pub owner_sid:String,
    pub root:PathBuf,
    pub port:u16,
    pub agent_exe:PathBuf,
    pub gui_exe:PathBuf,
    pub codex_home:PathBuf,
}
impl Profile {
    pub fn load(path:&Path)->Result<Self>{
        let bytes=fs::read(path).map_err(|_|"Agent尚未安装或profile不可读")?;
        if bytes.len()>65536{return Err("profile过大".into());}
        let profile:Self=serde_json::from_slice(&bytes).map_err(|_|"profile格式无效")?;profile.validate()?;Ok(profile)
    }
    pub fn validate(&self)->Result<()> {
        uuid::Uuid::parse_str(&self.profile_id).map_err(|_|"profile UUID无效")?;
        if self.owner_sid!=windows::sid()?||!self.root.is_absolute()||self.port==0||!self.agent_exe.is_absolute()||!self.gui_exe.is_absolute()||!self.codex_home.is_absolute(){return Err("profile身份或路径无效".into());}
        windows::check_path(&self.root)?;windows::check_path(&self.agent_exe)?;windows::check_path(&self.gui_exe)?;
        if self.environment=="production" {
            if self.root!=windows::user_home()?.join(".codex-floatboard/profiles").join(&self.profile_id){return Err("正式profile必须使用固定受管目录".into());}
        }else if self.environment=="development"{if !self.root.starts_with(windows::local_home()?.join("CodexFloatboard-Dev")){return Err("DEV数据必须位于隔离wrapper的受管沙箱".into());}}else{return Err("profile环境无效".into());}
        Ok(())
    }
    pub fn configured_path()->Result<PathBuf>{
        if let Some(path)=std::env::var_os("FLOATBOARD_PROFILE_CONFIG") {
            if std::env::var("FLOATBOARD_DEV_ISOLATED").as_deref()!=Ok("1"){return Err("自定义profile仅允许隔离DEV使用".into());}
            return Ok(path.into());
        }
        Ok(windows::user_home()?.join(".codex-floatboard/profile.json"))
    }
    pub fn pipe(&self)->String{format!(r"\\.\pipe\CodexFloatboard.{}.{}",self.owner_sid,self.profile_id)}
    pub fn origin(&self)->String{format!("http://127.0.0.1:{}",self.port)}
    pub fn server_name(&self)->&str{if self.environment=="production"{"codex_floatboard"}else{"codex_floatboard_dev"}}
}
