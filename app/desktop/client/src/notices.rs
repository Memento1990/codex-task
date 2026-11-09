use std::collections::HashMap;
use serde_json::{json,Value};
fn terminal(value:&Value)->bool{matches!(value["status"].as_str(),Some("completed"|"cancelled"))}
fn label(value:&str)->&str{match value{"running"=>"进行中","blocked"=>"受阻","waiting"=>"等待中","completed"=>"已完成","cancelled"=>"已取消",_=>"未汇报"}}
fn step_label(value:&str)->&str{match value{"pending"=>"待执行","inProgress"=>"进行中","completed"=>"已完成","blocked"=>"受阻",_=>value}}
#[derive(Default)]
pub struct ChangeDetector{previous:Option<HashMap<String,Value>>}
impl ChangeDetector{
    pub fn update(&mut self,tasks:&[Value])->Vec<Value>{
        let mut current=HashMap::new();let mut notices=Vec::new();
        for task in tasks.iter().filter(|t|t["archived"]!=true&&(t["status"]!="unreported"||t["plan"].as_array().is_some_and(|p|!p.is_empty()))){
            let id=task["id"].as_str().unwrap_or("").to_owned();let old=self.previous.as_ref().and_then(|p|p.get(&id));let mut lines=Vec::new();let mut tab=0;
            if self.previous.is_some(){
                if old.is_none()||old.is_some_and(|o|terminal(o)&&!terminal(task)&&o["title"]!=task["title"]){lines.push(format!("新增任务 · {}",label(task["status"].as_str().unwrap_or(""))));if let Some(blockers)=task["blockers"].as_array(){for b in blockers.iter().filter(|b|b["open"]==true){lines.push(format!("新增卡点 · {}",b["title"].as_str().unwrap_or("")));tab=1;}}}
                else if let Some(old)=old{
                    if old["status"]!=task["status"]{lines.push(format!("任务 · {} → {}",label(old["status"].as_str().unwrap_or("")),label(task["status"].as_str().unwrap_or(""))));}
                    if let Some(plan)=task["plan"].as_array(){for item in plan{let previous=old["plan"].as_array().and_then(|p|p.iter().find(|s|s["id"]==item["id"]));let title=item["title"].as_str().unwrap_or("");if previous.is_none(){lines.push(format!("新增清单 · {title}"));}else if previous.unwrap()["status"]!=item["status"]{lines.push(format!("{title} · {}",step_label(item["status"].as_str().unwrap_or(""))));}}}
                    if let Some(blockers)=task["blockers"].as_array(){for item in blockers{let previous=old["blockers"].as_array().and_then(|p|p.iter().find(|s|s["id"]==item["id"]));if previous.is_none()&&item["open"]==true||previous.is_some_and(|p|p["open"]!=item["open"]){let prefix=if previous.is_none(){"新增卡点"}else if item["open"]==true{"卡点待处理"}else{"卡点已解除"};lines.insert(0,format!("{prefix} · {}",item["title"].as_str().unwrap_or("")));tab=1;}}}
                    if let Some(blockers)=old["blockers"].as_array(){for item in blockers.iter().filter(|b|b["open"]==true){if !task["blockers"].as_array().is_some_and(|p|p.iter().any(|b|b["id"]==item["id"])){lines.insert(0,format!("卡点已解除 · {}",item["title"].as_str().unwrap_or("")));tab=1;}}}
                }
                if !lines.is_empty(){let remaining=lines.len().saturating_sub(3);lines.truncate(3);if remaining>0{lines.push(format!("另有 {remaining} 项变化"));}notices.push(json!({"task_id":id,"title":task["title"].as_str().filter(|s|!s.is_empty()).unwrap_or("任务更新"),"lines":lines,"tab":tab,"sticky":task["blockers"].as_array().is_some_and(|items|items.iter().any(|item|item["open"]==true))}));}
            }
            current.insert(id,json!({"id":task["id"],"title":task["title"],"status":task["status"],"plan":task["plan"],"blockers":task["blockers"]}));
        }self.previous=Some(current);notices
    }
}
