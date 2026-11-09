use std::{collections::HashSet, fs, path::Path};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest,Sha256};
use crate::{now, Result, timing::TimingIndex};

pub const EVENTS: &[&str] = &["SessionStart","SessionEnd","UserPromptSubmit","PreToolUse","PostToolUse","PermissionRequest","Stop","Interrupt","SubagentStart","SubagentStop"];
pub fn hash(value: &[u8]) -> String { format!("{:x}", Sha256::digest(value)) }
pub use floatboard_protocol::report::{text,validate_report};
// This archive is a completion *episode*: consecutive completed revisions update
// one record, even if the title changes. A subsequent non-completed report starts
// a new episode. Event revisions remain available for timing and deduplication.
fn save_completion(db:&Connection,task:&mut Value)->Result<()> {
    let source=task["id"].as_str().ok_or("任务标识无效")?.to_owned();
    let revision=task["revision"].as_i64().ok_or("任务修订无效")?;
    let archive=task["completion_archive_id"].as_str().map(str::to_owned).unwrap_or_else(||format!("archive:{}:{revision}",hash(source.as_bytes())));
    task["completion_archive_id"]=json!(archive);
    let mut snapshot=task.clone();snapshot.as_object_mut().unwrap().remove("history");
    let mut timing=TimingIndex::default();timing.update(db)?;timing.attach(&mut snapshot);
    snapshot["id"]=json!(archive);snapshot["source_task_id"]=json!(source);snapshot["archived"]=json!(true);
    snapshot["runtime"]=json!("idle");snapshot["active_tools"]=json!({});snapshot["agents"]=json!({});
    db.execute("insert into completed_tasks(id,source_task_id,revision,payload) values(?1,?2,?3,?4) on conflict(id) do update set revision=excluded.revision,payload=excluded.payload",params![archive,source,revision,snapshot.to_string()]).map_err(|error|crate::storage_error::sqlite("store.archive_write","完成记录保存失败",error))?;
    Ok(())
}
fn archived_projection(task:&mut Value){
    if task.get("waiting_since").is_none(){task["waiting_since"]=Value::Null;}
    if let Some(items)=task["evidence"].as_array_mut(){for item in items{if item.get("at").is_none(){item["at"]=Value::Null;item["time_source"]=json!("unknown");}}}
}
pub struct Store {pub db:Connection,timings:TimingIndex,identity:crate::file_identity::FileGuard}
impl Store {
    pub fn open(root:&Path,readonly:bool)->Result<Self>{
        if !readonly {fs::create_dir_all(root).map_err(|_|"本地目录不可写")?;}
        let identity=crate::file_identity::FileGuard::open(&root.join("events.sqlite3"),!readonly)?;
        let db=Connection::open_with_flags(root.join("events.sqlite3"),if readonly{OpenFlags::SQLITE_OPEN_READ_ONLY}else{OpenFlags::SQLITE_OPEN_READ_WRITE|OpenFlags::SQLITE_OPEN_CREATE}).map_err(|error|crate::storage_error::sqlite("store.open","本地任务库不可用",error))?;
        db.busy_timeout(std::time::Duration::from_secs(if readonly{2}else{8})).map_err(|error|crate::storage_error::sqlite("store.configure","任务库配置失败",error))?;
        if !readonly{db.execute_batch("pragma journal_mode=WAL; create table if not exists events(id text primary key,task_id text not null,kind text not null,at real not null,payload text not null); create table if not exists tasks(id text primary key,payload text not null); create index if not exists event_task_at on events(task_id,at);").map_err(|error|crate::storage_error::sqlite("store.initialize","任务库初始化失败",error))?;}
        if !readonly {db.execute_batch("create table if not exists completed_tasks(id text primary key,source_task_id text not null,revision integer not null,payload text not null);").map_err(|e|crate::storage_error::sqlite("store.schema","完成档案表不可用",e))?;}
        identity.verify()?;
        Ok(Self{db,timings:TimingIndex::default(),identity})
    }
    pub fn ingest(&mut self,event:&Value)->Result<bool>{self.ingest_with(event,|_,_|Ok(()))}
    /// Agent supplies receipt and notification writes; all commit with the projection.
    pub fn ingest_with(&mut self,event:&Value,after:impl FnOnce(&Connection,&Value)->Result<()>)->Result<bool>{
        self.identity.verify()?;
        let kind=event["kind"].as_str().filter(|k|EVENTS.contains(k)||["progress","plan"].contains(k)).ok_or("事件类型无效")?;
        let session=event["session_id"].as_str().filter(|s|!s.is_empty()).ok_or("事件身份无效")?;let host=event["host_id"].as_str().filter(|s|!s.is_empty()).ok_or("事件身份无效")?;
        let at=event["recorded_at"].as_f64().filter(|n|*n>0.&&*n<now()+300.).ok_or("事件时间无效")?;
        let id=text(event.get("event_id").unwrap_or(&json!("")),160)?;if id.is_empty(){return Err("事件标识缺失".into());}
        let mut safe=json!({"event_id":id,"kind":kind,"recorded_at":at,"session_id":session,"host_id":host});
        for key in ["cwd","turn_id","agent_id","tool_name","tool_use_id"]{safe[key]=json!(text(event.get(key).filter(|v|!v.is_null()).unwrap_or(&json!("")),if key=="cwd"{2000}else{200})?);}
        if kind=="progress"{safe["report"]=validate_report(&event["report"])?;}
        if kind=="plan"{safe["plan"]=validate_report(&json!({"revision":0,"status":"running","plan":event["plan"]}))?["plan"].clone();}
        let task_id=format!("{host}:{session}");
        let tx=self.db.transaction().map_err(|error|crate::storage_error::sqlite("store.begin","任务库写入失败",error))?;
        let exists:bool=tx.query_row("select exists(select 1 from events where id=?1)",[&id],|r|r.get(0)).map_err(|error|crate::storage_error::sqlite("store.tasks_read","任务库读取失败",error))?;if exists{return Ok(false);}
        let raw:Option<String>=tx.query_row("select payload from tasks where id=?1",[&task_id],|r|r.get(0)).optional().map_err(|error|crate::storage_error::sqlite("store.tasks_read","任务库读取失败",error))?;
        let mut task=match raw{Some(raw)=>serde_json::from_str(&raw).map_err(|_|"任务数据损坏 [stage=store.task_decode]")?,None=>json!({"id":task_id,"session_id":session,"host_id":host,"cwd":safe["cwd"],"title":"","status":"unreported","runtime":"unknown","runtime_at":0,"updated_at":0,"revision":-1,"plan":[],"blockers":[],"evidence":[],"current_step":"","agents":{},"active_tools":{}})};
        task.as_object_mut().ok_or("任务数据损坏 [stage=store.task_decode]")?;
        let agent=safe["agent_id"].as_str().unwrap_or("");let report=&safe["report"];let accepted_progress=kind=="progress"&&agent.is_empty()&&report["revision"].as_i64().unwrap_or(-1)>task["revision"].as_i64().unwrap_or(-1);
        if !agent.is_empty(){if at>=task["agents"][agent]["updated_at"].as_f64().unwrap_or(0.){task["agents"][agent]=json!({"runtime":if ["SubagentStop","Stop","SessionEnd"].contains(&kind){"idle"}else{"active"},"updated_at":at,"tool_name":safe["tool_name"]});}}
        else if kind=="progress"&&report["revision"].as_i64().unwrap_or(-1)>task["revision"].as_i64().unwrap_or(-1){if report["status"]!="completed"{task.as_object_mut().unwrap().remove("completion_archive_id");}for (k,v) in report.as_object().unwrap(){task[k]=v.clone();}}
        else if kind=="plan"&&at>=task["plan_at"].as_f64().unwrap_or(0.){if task["revision"].as_i64().unwrap_or(-1)<0{task["plan"]=safe["plan"].clone();task["plan_at"]=json!(at);}}
        else if at>=task["runtime_at"].as_f64().unwrap_or(0.)&&EVENTS.contains(&kind){
            task["runtime_at"]=json!(at);
            if ["Stop","SessionEnd","Interrupt"].contains(&kind){task["runtime"]=json!(if kind=="Interrupt"{"interrupted"}else{"idle"});task["active_tools"]=json!({});}
            else if kind=="PermissionRequest"{task["runtime"]=json!("waiting_approval");}
            else if ["SessionStart","UserPromptSubmit","PreToolUse","PostToolUse"].contains(&kind){task["runtime"]=json!("active");let call=safe["tool_use_id"].as_str().unwrap_or("");if !call.is_empty(){if kind=="PreToolUse"{task["active_tools"][call]=safe["tool_name"].clone();}else if kind=="PostToolUse"{task["active_tools"].as_object_mut().unwrap().remove(call);}}}
        }
        task["updated_at"]=json!(at.max(task["updated_at"].as_f64().unwrap_or(0.)));
        let stored=match kind {"progress"=>json!({"agent_id":agent,"report":safe["report"]}),"plan"=>json!({"agent_id":agent,"plan":safe["plan"]}),_=>json!({})};
        tx.execute("insert into events values(?1,?2,?3,?4,?5)",params![id,task_id,kind,at,stored.to_string()]).map_err(|error|crate::storage_error::sqlite("store.event_write","任务事件保存失败",error))?;
        if accepted_progress&&task["status"]=="completed" {save_completion(&tx,&mut task)?;}
        tx.execute("insert or replace into tasks values(?1,?2)",params![task_id,task.to_string()]).map_err(|error|crate::storage_error::sqlite("store.task_write","任务状态保存失败",error))?;
        after(&tx,&task)?;
        tx.commit().map_err(|error|crate::storage_error::sqlite("store.commit","任务事务保存失败",error))?;Ok(true)
    }
    pub fn tasks(&mut self)->Result<Vec<Value>>{
        self.identity.verify()?;
        self.timings.update(&self.db)?;
        let mut statement=self.db.prepare("select payload from tasks").map_err(|error|crate::storage_error::sqlite("store.tasks_read","任务库读取失败",error))?;
        let rows=statement.query_map([],|r|r.get::<_,String>(0)).map_err(|error|crate::storage_error::sqlite("store.tasks_read","任务库读取失败",error))?;
        let mut tasks=Vec::new();let mut current_completions=HashSet::new();for raw in rows{let mut task:Value=serde_json::from_str(&raw.map_err(|error|crate::storage_error::sqlite("store.tasks_read","任务库读取失败",error))?).map_err(|_|"任务数据损坏 [stage=store.task_decode]")?;task.as_object_mut().ok_or("任务数据损坏 [stage=store.task_decode]")?.remove("history");
            if task["revision"].as_i64().unwrap_or(-1)<0 {
                let Some(plan)=task["plan"].as_array().filter(|p|!p.is_empty())else{continue;};
                let title=plan[0]["title"].clone();
                if task["title"].as_str().unwrap_or("").trim().is_empty(){task["title"]=title;}
            }
            if task["status"]=="completed"{if let Some(id)=task["completion_archive_id"].as_str(){current_completions.insert(id.to_owned());}}
            self.timings.attach(&mut task);tasks.push(task);}
        // Old readonly databases intentionally do not run schema migration.
        let has_archive:bool=self.db.query_row("select exists(select 1 from sqlite_master where type='table' and name='completed_tasks')",[],|r|r.get(0)).map_err(|error|crate::storage_error::sqlite("store.archive_read","完成记录读取失败",error))?;
        if has_archive{let mut statement=self.db.prepare("select id,payload from completed_tasks").map_err(|error|crate::storage_error::sqlite("store.archive_read","完成记录读取失败",error))?;let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|error|crate::storage_error::sqlite("store.archive_read","完成记录读取失败",error))?;for row in rows{let(id,raw)=row.map_err(|error|crate::storage_error::sqlite("store.archive_read","完成记录读取失败",error))?;if current_completions.contains(&id){continue;}let mut archive:Value=serde_json::from_str(&raw).map_err(|_|"完成记录损坏 [stage=store.archive_decode]")?;archive.as_object_mut().ok_or("完成记录损坏 [stage=store.archive_decode]")?.remove("history");archived_projection(&mut archive);tasks.push(archive);}}
        tasks.sort_by(|a,b|b["updated_at"].as_f64().unwrap_or(0.).total_cmp(&a["updated_at"].as_f64().unwrap_or(0.)));Ok(tasks)
    }
    pub fn revision(&self,session:&str,host:&str)->Result<i64>{
        let raw:Option<String>=self.db.query_row("select payload from tasks where id=?1",[format!("{host}:{session}")],|r|r.get(0)).optional().map_err(|e|crate::storage_error::sqlite("store.revision","任务修订不可读",e))?;
        match raw{None=>Ok(-1),Some(raw)=>serde_json::from_str::<Value>(&raw).map_err(|_|"任务修订损坏".to_string())?["revision"].as_i64().ok_or_else(||"任务修订损坏".into())}
    }
}
