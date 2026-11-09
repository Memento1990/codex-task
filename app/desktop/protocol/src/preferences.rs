use serde_json::{json,Value};
use crate::Result;
pub fn defaults()->Value{json!({"auto_start":false,"show_taskbar":false,"theme":"dark","appearance_revision":2,"notifications":true,"notification_seconds":3,"dock_hover_summary":true,"dock_summary_mode":"hover","startup_command":""})}
pub fn validate_window(value:&Value)->Result<()>{
    for(key,value)in value.as_object().ok_or("窗口设置格式无效")?{let valid=match key.as_str(){"pinned"|"details_collapsed"=>value.is_boolean(),"selected"|"filter"=>value.as_str().is_some_and(|s|s.len()<=500),"tab"=>value.as_u64().is_some_and(|n|n<=2),"width"=>value.as_f64().is_some_and(|n|(392.0..=100000.).contains(&n)),"height"=>value.as_f64().is_some_and(|n|(570.0..=100000.).contains(&n)),"x"|"y"=>value.as_f64().is_some_and(|n|n.is_finite()&&n.abs()<=100000.),_=>false};if !valid{return Err("窗口设置无效".into());}}Ok(())
}
pub fn validate_options(value:&Value)->Result<Value>{
    if !value.is_object(){return Err("设置格式无效".into());}let mut out=defaults();for key in ["auto_start","show_taskbar","theme","notifications","notification_seconds","dock_hover_summary","dock_summary_mode"]{if let Some(value)=value.get(key){out[key]=value.clone();}}
    if ["auto_start","show_taskbar","notifications","dock_hover_summary"].iter().any(|k|!out[k].is_boolean()){return Err("开关设置无效".into());}
    if value.get("dock_summary_mode").is_none(){out["dock_summary_mode"]=json!(if out["dock_hover_summary"]==false{"never"}else{"hover"});}
    if ![Some("always"),Some("hover"),Some("never")].contains(&out["dock_summary_mode"].as_str()){return Err("任务摘要显示模式无效".into());}
    out["dock_hover_summary"]=json!(out["dock_summary_mode"]!="never");
    if ![Some("light"),Some("dark")].contains(&out["theme"].as_str()){return Err("主题设置无效".into());}
    if !out["notification_seconds"].as_u64().is_some_and(|n|(1..=60).contains(&n)){return Err("通知停留时间须为 1–60 秒".into());}Ok(out)
}
pub fn merge_options(previous:&Value,patch:&Value)->Result<Value>{
    if !patch.is_object(){return Err("设置格式无效".into());}
    let mut merged=previous.clone();
    for key in ["auto_start","show_taskbar","theme","notifications","notification_seconds","dock_hover_summary","dock_summary_mode"]{if let Some(value)=patch.get(key){merged[key]=value.clone();}}
    if patch.get("dock_summary_mode").is_none()&&patch.get("dock_hover_summary").is_some(){merged["dock_summary_mode"]=json!(if merged["dock_hover_summary"]==false{"never"}else{"hover"});}
    let mut out=validate_options(&merged)?;out["startup_command"]=previous.get("startup_command").cloned().unwrap_or(json!(""));Ok(out)
}
