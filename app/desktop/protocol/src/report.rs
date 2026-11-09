use std::{collections::HashSet,sync::OnceLock};
use serde_json::{json,Value};
use crate::Result;
pub fn text(value: &Value, limit: usize) -> Result<String> {
    static SECRET: OnceLock<regex::Regex> = OnceLock::new();
    let pattern=SECRET.get_or_init(||regex::Regex::new(r"(?i)(?:sk-[a-z0-9_-]{16,}|(?:password|passwd|token|api[_-]?key)\s*[:=]\s*[^\s,;]{6,})").unwrap());
    Ok(pattern.replace_all(value.as_str().ok_or("文本字段类型错误")?.trim(), "[敏感值已过滤]").chars().take(limit).collect())
}
fn field(value:&Value, key:&str, default:&str, limit:usize)->Result<String> {text(value.get(key).unwrap_or(&json!(default)),limit)}
pub fn validate_report(value:&Value)->Result<Value>{
    if !value.is_object(){return Err("进度必须是对象".into());}
    let revision=value["revision"].as_u64().filter(|n|*n<=i64::MAX as u64).ok_or("revision 必须是非负整数")?;
    let status=value["status"].as_str().filter(|s|["running","blocked","waiting","completed","cancelled"].contains(s)).ok_or("任务状态无效")?;
    let mut lists=Vec::new();
    for key in ["plan","blockers","evidence"]{
        let raw=value.get(key).cloned().unwrap_or(json!([]));let raw=raw.as_array().filter(|a|a.len()<=100).ok_or("清单格式或数量无效")?;
        let mut items=Vec::new();let mut ids=HashSet::new();
        for item in raw {
            if key=="evidence"{items.push(if item.is_string(){json!({"label":text(item,300)?,"reference":""})}else if item.is_object(){json!({"label":text(item.get("label").ok_or("证据格式无效")?,300)?,"reference":field(item,"reference","",2000)?})}else{return Err("证据格式无效".into());});continue;}
            if !item.is_object(){return Err("清单格式无效".into());}
            let id=field(item,"id","",120)?;let title=field(item,"title","",300)?;
            if id.is_empty()||title.is_empty()||!ids.insert(id.clone()){return Err("清单标识与标题须非空且标识唯一".into());}
            if key=="plan"{
                let state=item["status"].as_str().filter(|s|["pending","inProgress","completed","blocked"].contains(s)).ok_or("步骤状态无效")?;
                items.push(json!({"id":id,"title":title,"status":state,"note":field(item,"note","",400)?}));
            }else{
                let open=item.get("open").unwrap_or(&Value::Bool(true)).as_bool().ok_or("卡点格式无效")?;
                let mut block=json!({"id":id,"title":title,"open":open});
                for k in ["reason","impact","owner","resolve_when"]{block[k]=json!(field(item,k,if k=="owner"{"Codex"}else{""},if k=="owner"{100}else{500})?);}items.push(block);
            }
        }lists.push(items);
    }
    let opened=lists[1].iter().any(|b|b["open"]==true);
    if status=="blocked"&&!opened{return Err("受阻状态须说明开放卡点".into());}
    if status=="completed"&&(lists[0].is_empty()||lists[0].iter().any(|s|s["status"]!="completed")||!lists[2].iter().any(|e|e["label"].as_str().is_some_and(|s|!s.is_empty()))||opened){return Err("完成须清单全完成、有证据且无未解除卡点".into());}
    Ok(json!({"revision":revision,"title":field(value,"title","",200)?,"status":status,"current_step":field(value,"current_step","",300)?,"plan":lists[0],"blockers":lists[1],"evidence":lists[2]}))
}
