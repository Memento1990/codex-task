use std::{collections::HashSet, fs::{self, OpenOptions}, io::Write, path::{Path, PathBuf}, sync::OnceLock};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use crate::{now, Result, timing::TimingIndex};

pub const EVENTS: &[&str] = &["SessionStart","SessionEnd","UserPromptSubmit","PreToolUse","PostToolUse","PermissionRequest","Stop","Interrupt","SubagentStart","SubagentStop"];
pub fn hash(value: &[u8]) -> String { format!("{:x}", Sha256::digest(value)) }
pub fn text(value: &Value, limit: usize) -> Result<String> {
    static SECRET: OnceLock<regex::Regex> = OnceLock::new();
    let pattern=SECRET.get_or_init(||regex::Regex::new(r"(?i)(?:sk-[a-z0-9_-]{16,}|(?:password|passwd|token|api[_-]?key)\s*[:=]\s*[^\s,;]{6,})").unwrap());
    Ok(pattern.replace_all(value.as_str().ok_or("文本字段类型错误")?.trim(), "[敏感值已过滤]").chars().take(limit).collect())
}
fn field(value:&Value, key:&str, default:&str, limit:usize)->Result<String> {text(value.get(key).unwrap_or(&json!(default)),limit)}
pub fn atomic_json(path:&Path,value:&Value)->Result<()> {
    let bytes=serde_json::to_vec(value).map_err(|_|"本地数据编码失败")?;
    atomic_bytes(path,&bytes)
}
pub fn atomic_bytes(path:&Path,bytes:&[u8])->Result<()> {
    let parent=path.parent().ok_or("本地数据路径无效")?;
    fs::create_dir_all(parent).map_err(|_|"本地目录不可写")?;
    let temporary=parent.join(format!(".floatboard-{}.tmp",uuid::Uuid::new_v4().simple()));
    let result=(||{
        let mut f=OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|_|"本地文件不可写")?;
        f.write_all(bytes).and_then(|_|f.sync_all()).map_err(|_|"本地文件保存失败")?;drop(f);
        // Windows rename 不覆盖目标，使用系统原子替换保留旧文件直到写入成功。
        use std::os::windows::ffi::OsStrExt;
        // Canonical parent uses Windows extended-length paths. Nested DEV
        // content-addressed runtime paths can exceed legacy MAX_PATH.
        let absolute_parent=fs::canonicalize(parent).map_err(|_|"本地目录不可解析")?;
        let from_path=absolute_parent.join(temporary.file_name().ok_or("本地临时文件名无效")?);
        let to_path=absolute_parent.join(path.file_name().ok_or("本地文件名无效")?);
        let from:Vec<u16>=from_path.as_os_str().encode_wide().chain(Some(0)).collect();
        let to:Vec<u16>=to_path.as_os_str().encode_wide().chain(Some(0)).collect();
        let ok=unsafe{windows_sys::Win32::Storage::FileSystem::MoveFileExW(from.as_ptr(),to.as_ptr(),windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING|windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH)};
        if ok==0 {return Err("本地文件替换失败".into());} Ok(())
    })();let _=fs::remove_file(temporary);result
}
pub fn read_json(path:&Path)->Result<Value>{serde_json::from_slice(&fs::read(path).map_err(|_|"本地文件不可读")?).map_err(|_|"本地数据格式无效".into())}
pub fn session_path(root:&Path,id:&str)->PathBuf {root.join("sessions").join(format!("{}.json",hash(id.as_bytes())))}
pub fn register(root:&Path,id:&str,cwd:&str)->Result<Value>{
    if id.is_empty()||id.chars().count()>160{return Err("会话标识无效".into());}
    let path=session_path(root,id);
    if !path.exists(){
        let token=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
        let record=json!({"session_id":id,"session_token":token,"host_id":std::env::var("COMPUTERNAME").unwrap_or_else(|_|"localhost".into()),"cwd":text(&json!(cwd),2000)?,"created_at":now()});
        let temp=path.with_extension(format!("{}.tmp",uuid::Uuid::new_v4().simple()));
        atomic_json(&temp,&record)?;
        if let Err(e)=fs::hard_link(&temp,&path){if e.kind()!=std::io::ErrorKind::AlreadyExists{let _=fs::remove_file(&temp);return Err("会话绑定保存失败".into());}}
        let _=fs::remove_file(temp);
    }
    let record=read_json(&path)?;
    if record["session_id"]!=id{return Err("会话标识不匹配".into());} Ok(record)
}
pub fn authenticate(root:&Path,value:&Value)->Result<Value>{
    let id=value["session_id"].as_str().ok_or("缺少会话绑定信息")?;
    let token=value["session_token"].as_str().ok_or("缺少会话绑定信息")?;
    let record=read_json(&session_path(root,id)).map_err(|_|"会话尚未登记，请先启用 Hook")?;
    let saved=record["session_token"].as_str().ok_or("会话绑定信息不匹配")?;
    // 比较固定长度摘要，避免明文 token 比较的提前返回。
    let a=Sha256::digest(saved.as_bytes());let b=Sha256::digest(token.as_bytes());
    let mismatch=a.iter().zip(b.iter()).fold(0u8,|v,(a,b)|v|(a^b));
    if mismatch!=0||record["session_id"]!=id{return Err("会话绑定信息不匹配".into());}Ok(record)
}
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
pub fn enqueue(root:&Path,mut event:Value)->Result<String>{
    let id=uuid::Uuid::new_v4().simple().to_string();event["version"]=json!(1);event["event_id"]=json!(id);event["recorded_at"]=json!(now());
    atomic_json(&root.join("inbox").join(format!("{id}.json")),&event)?;Ok(id)
}

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
    db.execute("insert into completed_tasks(id,source_task_id,revision,payload) values(?1,?2,?3,?4) on conflict(id) do update set revision=excluded.revision,payload=excluded.payload",params![archive,source,revision,snapshot.to_string()]).map_err(|_|"完成记录保存失败")?;
    Ok(())
}
fn migrate_retention(db:&Connection)->Result<()> {
    db.execute_batch("create table if not exists completed_tasks(id text primary key,source_task_id text not null,revision integer not null,payload text not null); create table if not exists store_migrations(name text primary key);").map_err(|_|"任务保留表初始化失败")?;
    let tx=rusqlite::Transaction::new_unchecked(db,rusqlite::TransactionBehavior::Immediate).map_err(|_|"任务保留迁移失败")?;
    let done:bool=tx.query_row("select exists(select 1 from store_migrations where name='task_retention_v1')",[],|r|r.get(0)).map_err(|_|"任务迁移读取失败")?;
    if done{return Ok(());}
    let rows={let mut statement=tx.prepare("select id,payload from tasks").map_err(|_|"任务迁移读取失败")?;let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|_|"任务迁移读取失败")?;rows.collect::<std::result::Result<Vec<_>,_>>().map_err(|_|"任务迁移读取失败")?};
    for (id,raw) in rows {let mut task:Value=serde_json::from_str(&raw).map_err(|_|"任务数据损坏")?;task.as_object_mut().ok_or("任务数据损坏")?;task["history"]=json!([]);if task["status"]=="completed"{save_completion(&tx,&mut task)?;}tx.execute("update tasks set payload=?2 where id=?1",params![id,task.to_string()]).map_err(|_|"任务迁移保存失败")?;}
    // Hook payloads were an activity feed only. Keep their ids and timestamps for
    // replay deduplication; progress/plan payloads are business timing evidence.
    tx.execute("update events set payload='{}' where kind not in ('progress','plan')",[]).map_err(|_|"任务事件最小化失败")?;
    let events={let mut statement=tx.prepare("select id,kind,payload from events where kind in ('progress','plan')").map_err(|_|"任务事件读取失败")?;let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|_|"任务事件读取失败")?;rows.collect::<std::result::Result<Vec<_>,_>>().map_err(|_|"任务事件读取失败")?};
    for(id,kind,raw)in events{let event:Value=serde_json::from_str(&raw).map_err(|_|"计时记录格式无效")?;let value=if kind=="progress"{json!({"agent_id":event["agent_id"],"report":event["report"]})}else{json!({"agent_id":event["agent_id"],"plan":event["plan"]})};tx.execute("update events set payload=?2 where id=?1",params![id,value.to_string()]).map_err(|_|"任务事件最小化失败")?;}
    tx.execute("insert into store_migrations(name) values('task_retention_v1')",[]).map_err(|_|"任务迁移保存失败")?;
    tx.commit().map_err(|_|"任务迁移提交失败")?;Ok(())
}
pub struct Store {pub db:Connection,timings:TimingIndex}
impl Store {
    pub fn open(root:&Path,readonly:bool)->Result<Self>{
        if !readonly {fs::create_dir_all(root).map_err(|_|"本地目录不可写")?;}
        let db=Connection::open_with_flags(root.join("events.sqlite3"),if readonly{OpenFlags::SQLITE_OPEN_READ_ONLY}else{OpenFlags::SQLITE_OPEN_READ_WRITE|OpenFlags::SQLITE_OPEN_CREATE}).map_err(|_|"本地任务库不可用")?;
        db.busy_timeout(std::time::Duration::from_secs(if readonly{2}else{8})).map_err(|_|"任务库配置失败")?;
        if !readonly{db.execute_batch("pragma journal_mode=WAL; create table if not exists events(id text primary key,task_id text not null,kind text not null,at real not null,payload text not null); create table if not exists tasks(id text primary key,payload text not null); create index if not exists event_task_at on events(task_id,at);").map_err(|_|"任务库初始化失败")?;}
        if !readonly { migrate_retention(&db)?; }
        Ok(Self{db,timings:TimingIndex::default()})
    }
    pub fn ingest(&mut self,event:&Value)->Result<bool>{
        let kind=event["kind"].as_str().filter(|k|EVENTS.contains(k)||["progress","plan"].contains(k)).ok_or("事件类型无效")?;
        let session=event["session_id"].as_str().filter(|s|!s.is_empty()).ok_or("事件身份无效")?;let host=event["host_id"].as_str().filter(|s|!s.is_empty()).ok_or("事件身份无效")?;
        let at=event["recorded_at"].as_f64().filter(|n|*n>0.&&*n<now()+300.).ok_or("事件时间无效")?;
        let id=field(event,"event_id","",160)?;if id.is_empty(){return Err("事件标识缺失".into());}
        let mut safe=json!({"event_id":id,"kind":kind,"recorded_at":at,"session_id":session,"host_id":host});
        for key in ["cwd","turn_id","agent_id","tool_name","tool_use_id"]{safe[key]=json!(text(event.get(key).filter(|v|!v.is_null()).unwrap_or(&json!("")),if key=="cwd"{2000}else{200})?);}
        if kind=="progress"{safe["report"]=validate_report(&event["report"])?;}
        if kind=="plan"{safe["plan"]=validate_report(&json!({"revision":0,"status":"running","plan":event["plan"]}))?["plan"].clone();}
        let task_id=format!("{host}:{session}");
        let tx=self.db.transaction().map_err(|_|"任务库写入失败")?;
        let exists:bool=tx.query_row("select exists(select 1 from events where id=?1)",[&id],|r|r.get(0)).map_err(|_|"任务库读取失败")?;if exists{return Ok(false);}
        let raw:Option<String>=tx.query_row("select payload from tasks where id=?1",[&task_id],|r|r.get(0)).optional().map_err(|_|"任务库读取失败")?;
        let mut task=match raw{Some(raw)=>serde_json::from_str(&raw).map_err(|_|"任务数据损坏")?,None=>json!({"id":task_id,"session_id":session,"host_id":host,"cwd":safe["cwd"],"title":"","status":"unreported","runtime":"unknown","runtime_at":0,"updated_at":0,"revision":-1,"plan":[],"blockers":[],"evidence":[],"current_step":"","agents":{},"active_tools":{}})};
        // Keep only an empty legacy ABI slot: 0.9 writers require an array.
        // No activity is retained or exposed by the current snapshot API.
        task.as_object_mut().ok_or("任务数据损坏")?;task["history"]=json!([]);
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
        tx.execute("insert into events values(?1,?2,?3,?4,?5)",params![id,task_id,kind,at,stored.to_string()]).map_err(|_|"任务事件保存失败")?;
        if accepted_progress&&task["status"]=="completed" {save_completion(&tx,&mut task)?;}
        tx.execute("insert or replace into tasks values(?1,?2)",params![task_id,task.to_string()]).map_err(|_|"任务状态保存失败")?;tx.commit().map_err(|_|"任务事务保存失败")?;Ok(true)
    }
    pub fn tasks(&mut self)->Result<Vec<Value>>{
        self.timings.update(&self.db)?;
        let mut statement=self.db.prepare("select payload from tasks").map_err(|_|"任务库读取失败")?;
        let rows=statement.query_map([],|r|r.get::<_,String>(0)).map_err(|_|"任务库读取失败")?;
        let mut tasks=Vec::new();let mut current_completions=HashSet::new();for raw in rows{let mut task:Value=serde_json::from_str(&raw.map_err(|_|"任务库读取失败")?).map_err(|_|"任务数据损坏")?;task.as_object_mut().ok_or("任务数据损坏")?.remove("history");
            if task["revision"].as_i64().unwrap_or(-1)<0 {
                let Some(plan)=task["plan"].as_array().filter(|p|!p.is_empty())else{continue;};
                let title=plan[0]["title"].clone();
                if task["title"].as_str().unwrap_or("").trim().is_empty(){task["title"]=title;}
            }
            if task["status"]=="completed"{if let Some(id)=task["completion_archive_id"].as_str(){current_completions.insert(id.to_owned());}}
            self.timings.attach(&mut task);tasks.push(task);}
        // Old readonly databases intentionally do not run schema migration.
        let has_archive:bool=self.db.query_row("select exists(select 1 from sqlite_master where type='table' and name='completed_tasks')",[],|r|r.get(0)).map_err(|_|"完成记录读取失败")?;
        if has_archive{let mut statement=self.db.prepare("select id,payload from completed_tasks").map_err(|_|"完成记录读取失败")?;let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|_|"完成记录读取失败")?;for row in rows{let(id,raw)=row.map_err(|_|"完成记录读取失败")?;if current_completions.contains(&id){continue;}let mut archive:Value=serde_json::from_str(&raw).map_err(|_|"完成记录损坏")?;archive.as_object_mut().ok_or("完成记录损坏")?.remove("history");tasks.push(archive);}}
        tasks.sort_by(|a,b|b["updated_at"].as_f64().unwrap_or(0.).total_cmp(&a["updated_at"].as_f64().unwrap_or(0.)));Ok(tasks)
    }
    pub fn revision(&self,session:&str,host:&str)->i64{self.db.query_row("select payload from tasks where id=?1",[format!("{host}:{session}")],|r|r.get::<_,String>(0)).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok()).and_then(|v|v["revision"].as_i64()).unwrap_or(-1)}
}
