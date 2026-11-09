use std::path::PathBuf;
use serde_json::{json,Value};
use crate::{Client,Profile,Result,notices::ChangeDetector};
use floatboard_protocol::preferences::defaults;

pub struct Engine{pub root:PathBuf,pub client:Client,options:Value,notices:ChangeDetector,database_id:Option<String>,sequence:u64}
impl Engine{
    pub fn new(profile:Profile)->Self{Self{root:profile.root.clone(),client:Client::new(profile),options:defaults(),notices:Default::default(),database_id:None,sequence:0}}
    pub fn options(&self)->Value{self.options.clone()}
    pub fn call(&mut self,method:&str,mut params:Value)->Result<Value>{
        let id=uuid::Uuid::new_v4().to_string();
        if method=="preferences"{params["settings_revision"]=json!(self.options["settings_revision"].as_u64().unwrap_or(0));if params["settings_revision"].is_null(){params.as_object_mut().unwrap().remove("settings_revision");}}
        let notify=params["notify"]!=false;
        let result=self.client.call_id(method,params,&id);
        let result=if method=="preferences"&&result.is_err(){match self.client.call("request_result",json!({"request_id":id})){Ok(v) if v["committed"]==true=>Ok(v["result"].clone()),Ok(_)=>result,Err(_)=>Err("设置提交结果尚未确认，请等待重新连接后检查".into())}}else{result};
        let mut value=result?;
        if method=="snapshot"{
            let database=value["database_instance_id"].as_str().ok_or("快照数据库身份缺失")?;
            let sequence=value["commit_sequence"].as_u64().ok_or("快照提交序号缺失")?;
            if self.database_id.as_deref()==Some(database)&&sequence<self.sequence{return Err("快照提交序号倒退，请检查Agent与数据库身份".into());}
            self.sequence=sequence;
            if self.database_id.as_deref().is_some_and(|old|old!=database){self.notices=Default::default();}
            self.database_id=Some(database.to_string());self.options=value["options"].clone();
            if value["storage"]["status"]=="ok"{let tasks=value["tasks"].as_array().ok_or("快照任务格式无效")?;value["notices"]=json!(if notify&&self.options["notifications"]==true{self.notices.update(tasks)}else{Vec::new()});}
        }else if method=="preferences"{self.options=value.clone();}
        Ok(value)
    }
}
