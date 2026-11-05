//! Copy-only recovery. Never opens SQLite on the source and never applies recovery.
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::Path};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use crate::{Result, store::Store};

const FILES: [&str; 3] = ["events.sqlite3", "events.sqlite3-wal", "events.sqlite3-shm"];
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
const MAX_ROWS: usize = 10_000;
const MAX_PAYLOAD: usize = 1024 * 1024;
const MAX_ACCEPTED_BYTES: usize = 32 * 1024 * 1024;
const MAX_READ_BYTES: usize = 64 * 1024 * 1024;
const MAX_WORK: usize = MAX_ROWS * 3;
const SQL_SECONDS: u64 = 8;

// A connection is never reused until its interrupt thread has been cancelled and
// joined. The same deadline covers integrity, counts, scanning and all retries.
fn with_sql_deadline<T>(db: &Connection, action: impl FnOnce(std::time::Instant) -> T) -> T {
    with_interrupt_deadline(db.get_interrupt_handle(),std::time::Instant::now()+std::time::Duration::from_secs(SQL_SECONDS),action)
}
fn with_interrupt_deadline<T>(interrupt:rusqlite::InterruptHandle,deadline:std::time::Instant,action:impl FnOnce(std::time::Instant)->T)->T {
    use std::sync::{Arc, Condvar, Mutex};
    let stop=Arc::new((Mutex::new(false),Condvar::new()));
    struct Cancel(Arc<(Mutex<bool>,Condvar)>);
    impl Drop for Cancel {fn drop(&mut self){let (lock,wake)=&*self.0;*lock.lock().unwrap_or_else(|e|e.into_inner())=true;wake.notify_all();}}
    std::thread::scope(|scope| {
        let worker_stop=stop.clone();
        let worker=scope.spawn(move || {
            let (lock,wake)=&*worker_stop;
            let mut cancelled=lock.lock().unwrap_or_else(|e|e.into_inner());
            while !*cancelled {
                let remaining=deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero(){interrupt.interrupt();}
                // Keep interrupting until cancellation: SQLite clears its flag
                // after an interrupted statement, so later best-effort counts
                // must not silently obtain a fresh unbounded SQL window.
                let wait=if remaining.is_zero(){std::time::Duration::from_millis(10)}else{remaining};
                cancelled=wake.wait_timeout(cancelled,wait).unwrap_or_else(|e|e.into_inner()).0;
            }
        });
        let cancel=Cancel(stop);
        let result=action(deadline);
        drop(cancel);
        let _=worker.join();
        result
    })
}


// Also reject Windows junctions/reparse points, including ancestor directories.
fn plain(path: &Path) -> Result<()> {
    for p in path.ancestors() {
        if p.as_os_str().is_empty() { continue; }
        let m = fs::symlink_metadata(p).map_err(|_| "恢复路径不可访问")?;
        if m.file_type().is_symlink() { return Err("拒绝符号链接恢复路径".into()); }
        #[cfg(windows)] {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 { return Err("拒绝重解析点恢复路径".into()); }
        }
    }
    Ok(())
}
fn fingerprint(path: &Path) -> Result<Option<Value>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("源文件不可访问".into()),
        Ok(m) if !m.is_file() => return Err("源数据库必须是普通文件".into()),
        Ok(m) if m.len() > MAX_FILE => return Err("数据库文件超过安全恢复大小限制".into()),
        _ => ()
    }
    plain(path)?;
    let mut f = File::open(path).map_err(|_| "源文件不可读")?;
    let mut hash = Sha256::new(); let mut len = 0u64; let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|_| "源文件读取失败")?;
        if n == 0 { break; }
        len += n as u64;
        if len > MAX_FILE { return Err("数据库文件超过安全恢复大小限制".into()); }
        hash.update(&buf[..n]);
    }
    Ok(Some(json!({"bytes":len,"sha256":format!("{:x}",hash.finalize())})))
}
fn fingerprints(root: &Path) -> Result<Value> {
    let mut v = json!({}); let mut total = 0;
    for name in FILES {
        let f = fingerprint(&root.join(name))?;
        total += f.as_ref().and_then(|v|v["bytes"].as_u64()).unwrap_or(0);
        if total > MAX_TOTAL { return Err("数据库总大小超过安全恢复限制".into()); }
        v[name] = f.unwrap_or(Value::Null);
    }
    Ok(v)
}
fn copy_file(from: &Path, to: &Path) -> Result<()> {
    plain(from)?;
    let mut input = File::open(from).map_err(|_| "数据库副本读取失败")?;
    let mut output = OpenOptions::new().create_new(true).write(true).open(to).map_err(|_| "数据库副本创建失败")?;
    let copied = std::io::copy(&mut (&mut input).take(MAX_FILE + 1), &mut output).map_err(|_| "数据库复制失败")?;
    if copied > MAX_FILE { return Err("数据库复制超过大小限制".into()); }
    output.flush().and_then(|_|output.sync_all()).map_err(|_| "数据库副本保存失败".to_string())
}
fn readonly(root: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(root.join(FILES[0]), OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|_| "副本数据库无法打开")?;
    db.busy_timeout(std::time::Duration::from_secs(2)).map_err(|_| "副本配置失败")?;
    db.execute_batch("pragma query_only=on; pragma trusted_schema=off;").map_err(|_| "副本只读保护失败")?;
    Ok(db)
}
fn integrity(db: &Connection) -> bool {
    db.query_row("pragma integrity_check(1)", [], |r|r.get::<_,String>(0)).is_ok_and(|s|s=="ok")
}
// Copy the whole validated database, including its WAL. No Store write-open or
// retention migration runs here: episodes, event timing evidence and other
// tables must survive unchanged. All SQLite handles refer to disposable copies.
fn complete_copy(analysis:&Path,destination:&Path,deadline:std::time::Instant)->Result<Value>{
    let mut source=Store::open(analysis,true)?;
    source.db.execute_batch("pragma query_only=on; pragma trusted_schema=off;").map_err(|_|"完整副本保护失败")?;
    let (task_rows,events,archives,visible)=with_interrupt_deadline(source.db.get_interrupt_handle(),deadline,|deadline|->Result<_>{
        if std::time::Instant::now()>=deadline||!integrity(&source.db){return Err("完整副本完整性检查未通过".into());}
        let mut rows_count=0usize;let mut bytes=0usize;
        {let mut statement=source.db.prepare("select case when length(cast(id as blob))<=2000 then id else null end,case when length(cast(payload as blob))<=1048576 then payload else null end from tasks limit 10001").map_err(|_|"完整副本任务读取失败")?;
        let mut rows=statement.query([]).map_err(|_|"完整副本任务读取失败")?;
        while let Some(row)=rows.next().map_err(|_|"完整副本任务读取失败")?{
            rows_count+=1;if rows_count>MAX_ROWS||std::time::Instant::now()>=deadline{return Err("完整副本验证超过安全限制".into());}
            let id:String=row.get(0).map_err(|_|"完整副本任务结构无效")?;let raw:String=row.get(1).map_err(|_|"完整副本任务结构无效")?;
            bytes=bytes.saturating_add(id.len()).saturating_add(raw.len());
            if bytes>MAX_READ_BYTES||sanitized_task(&id,&raw).is_none(){return Err("完整副本任务结构无效或超过安全限制".into());}
        }}
        let events:i64=source.db.query_row("select count(*) from events",[],|r|r.get(0)).map_err(|_|"完整副本事件读取失败")?;
        let has_archive:bool=source.db.query_row("select exists(select 1 from sqlite_schema where type='table' and name='completed_tasks')",[],|r|r.get(0)).map_err(|_|"完整副本归档读取失败")?;
        let archives:i64=if has_archive{source.db.query_row("select count(*) from completed_tasks",[],|r|r.get(0)).map_err(|_|"完整副本归档读取失败")?}else{0};
        let visible=source.tasks()?.len();
        Ok((rows_count,events,archives,visible))
    })?;
    drop(source);
    fs::create_dir(destination).map_err(|_|"完整恢复目录创建失败")?;
    for name in [FILES[0],FILES[1]]{if analysis.join(name).exists(){copy_file(&analysis.join(name),&destination.join(name))?;}}
    let target=Connection::open_with_flags(destination.join(FILES[0]),OpenFlags::SQLITE_OPEN_READ_WRITE|OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|_|"完整恢复副本无法打开")?;
    target.busy_timeout(std::time::Duration::from_secs(2)).map_err(|_|"完整恢复副本配置失败")?;
    with_interrupt_deadline(target.get_interrupt_handle(),deadline,|deadline|->Result<()>{
        if std::time::Instant::now()>=deadline{return Err("完整恢复副本验证超时".into());}
        target.execute_batch("pragma trusted_schema=off;").map_err(|_|"完整恢复副本保护失败")?;
        let busy:i64=target.query_row("pragma wal_checkpoint(TRUNCATE)",[],|r|r.get(0)).map_err(|_|"完整恢复日志封存失败")?;
        if busy!=0{return Err("完整恢复日志仍被占用".into());}
        let mode:String=target.query_row("pragma journal_mode=DELETE",[],|r|r.get(0)).map_err(|_|"完整恢复日志封存失败")?;
        if !mode.eq_ignore_ascii_case("delete")||!integrity(&target){return Err("完整恢复副本完整性检查失败".into());}
        Ok(())
    })?;
    drop(target);
    let mut checked=Store::open(destination,true)?;
    let checked_count=with_interrupt_deadline(checked.db.get_interrupt_handle(),deadline,|_|checked.tasks())?.len();
    if checked_count!=visible{return Err("完整恢复副本任务数量不一致".into());}
    drop(checked);
    OpenOptions::new().write(true).open(destination.join(FILES[0])).and_then(|f|f.sync_all()).map_err(|_|"完整恢复副本保存失败")?;
    Ok(json!({"status":"full_recovery","mode":"full","partial":false,"source_task_count":task_rows,"recovered_task_rows":task_rows,"recovered_tasks":visible,"events_recovered":events,"archives_recovered":archives,"omitted_tasks":0,"omitted_events":0,"omitted_archives":0,"rejected_rows":0,"unreadable_rows":0,"scan_complete":true,"destination_integrity_ok":true,"history_recovered":true,"timing_recovered":true}))
}
fn sanitized_task(id: &str, raw: &str) -> Option<Value> {
    if id.is_empty() || id.len()>2000 || raw.len()>MAX_PAYLOAD { return None; }
    let mut v: Value = serde_json::from_str(raw).ok()?;
    let obj = v.as_object_mut()?;
    if obj.get("id")?.as_str()? != id || obj.get("revision")?.as_i64().is_none() { return None; }
    if !["unreported","running","waiting","blocked","completed","cancelled"].contains(&obj.get("status")?.as_str()?) { return None; }
    for key in ["plan","blockers","evidence"] { if !obj.get(key)?.is_array() { return None; } }
    for key in ["history","timing","completion_archive_id"] { obj.remove(key); }
    for key in ["plan","blockers"] { for item in obj.get_mut(key)?.as_array_mut()? { item.as_object_mut()?.remove("timing"); } }
    obj.insert("history".into(), json!([]));
    obj.insert("runtime".into(), json!("unknown"));
    obj.insert("active_tools".into(), json!({})); obj.insert("agents".into(), json!({}));
    obj.insert("recovery_provenance".into(),json!({"source":"readable_task_snapshot","history_recovered":false,"timing_recovered":false,"live_state_verified":false}));
    Some(v)
}
// Only the Store's known rowid table and TEXT primary-key index may drive the
// fallback. Never trust an attacker-controlled view/index with the same name.
fn known_task_index(db: &Connection,deadline:std::time::Instant) -> bool {
    if std::time::Instant::now()>=deadline{return false;}
    let table:rusqlite::Result<(String,String)>=db.query_row("select type,tbl_name from sqlite_schema where name='tasks'",[],|r|Ok((r.get(0)?,r.get(1)?)));
    if !matches!(table,Ok((ref kind,ref name)) if kind=="table" && name=="tasks"){return false;}
    let columns=(||->rusqlite::Result<Vec<(String,String,i64,i64,i64)>>{
        let mut s=db.prepare("pragma table_xinfo('tasks')")?;
        let rows=s.query_map([],|r|Ok((r.get(1)?,r.get(2)?,r.get(3)?,r.get(5)?,r.get(6)?)))?;
        rows.take(3).collect()
    })();
    if !matches!(columns,Ok(ref c) if c.len()==2 && c[0].0=="id" && c[0].1.eq_ignore_ascii_case("text") && c[0].3==1 && c[0].4==0 && c[1].0=="payload" && c[1].1.eq_ignore_ascii_case("text") && c[1].2==1 && c[1].3==0 && c[1].4==0){return false;}
    let valid=(||->rusqlite::Result<bool>{
        let mut s=db.prepare("pragma index_list('tasks')")?;let mut rows=s.query([])?;let mut found=false;
        let mut index_count=0;
        while let Some(r)=rows.next()? {index_count+=1;if index_count>64 || std::time::Instant::now()>=deadline{return Ok(false);}if r.get::<_,String>(1)?=="sqlite_autoindex_tasks_1"{found=r.get::<_,i64>(2)?==1 && r.get::<_,String>(3)?=="pk" && r.get::<_,i64>(4)?==0;}}
        if !found{return Ok(false);}
        let mut s=db.prepare("pragma index_xinfo('sqlite_autoindex_tasks_1')")?;
        let entries=s.query_map([],|r|Ok((r.get::<_,i64>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(entries.len()==2 && entries[0]==(0,Some("id".into()),0,"BINARY".into(),1) && entries[1].0== -1 && entries[1].1.is_none() && entries[1].4==0)
    })();
    valid.unwrap_or(false)
}
#[derive(Default)]
struct SalvageWork {full_rows:usize,index_rows:usize,lookups:usize,rejected:usize,unreadable:usize,duplicates:usize,read_bytes:usize,accepted_bytes:usize,budget_exhausted:bool}
impl SalvageWork {
    fn available(&mut self,deadline:std::time::Instant)->bool {
        if std::time::Instant::now()>=deadline || self.full_rows+self.index_rows+self.lookups>=MAX_WORK || self.read_bytes>=MAX_READ_BYTES {self.budget_exhausted=true;false}else{true}
    }
    fn bytes(&mut self,n:usize)->bool {if n>MAX_READ_BYTES.saturating_sub(self.read_bytes){self.budget_exhausted=true;false}else{self.read_bytes+=n;true}}
    fn accept(&mut self,id:String,raw:String,tasks:&mut Vec<(String,Value)>,seen:&mut std::collections::HashSet<String>)->bool {
        if seen.contains(&id){self.duplicates+=1;return true;}
        let Some(task)=sanitized_task(&id,&raw) else {self.rejected+=1;return true;};
        let accepted=task.to_string().len();
        if tasks.len()>=MAX_ROWS || accepted>MAX_ACCEPTED_BYTES.saturating_sub(self.accepted_bytes){self.budget_exhausted=true;return false;}
        self.accepted_bytes+=accepted;seen.insert(id.clone());tasks.push((id,task));true
    }
}
fn salvage(db: &Connection, destination: &Path, deadline:std::time::Instant) -> Result<Value> {
    let count=|sql:&str|->Option<i64>{if std::time::Instant::now()>=deadline{None}else{db.query_row(sql,[],|r|r.get::<_,i64>(0)).ok().filter(|n|*n>=0)}};
    let known=count("select count(*) from tasks");
    let omitted_events=count("select count(*) from events");
    let omitted_archives=count("select count(*) from completed_tasks");
    let mut tasks=Vec::new();let mut work=SalvageWork::default();let mut seen=std::collections::HashSet::new();let mut scan_complete=true;
    if let Ok(mut stmt)=db.prepare("select case when length(cast(id as blob)) <= 2000 then id else null end, case when length(cast(payload as blob)) <= 1048576 then payload else null end from tasks not indexed limit 10001") {
        match stmt.query([]) {
            Ok(mut rows)=>loop {
                if !work.available(deadline) || MAX_READ_BYTES.saturating_sub(work.read_bytes)<MAX_PAYLOAD+2000 {work.budget_exhausted=true;scan_complete=false;break;}
                match rows.next() {
                    Ok(Some(row))=>{
                        work.full_rows+=1;
                        if work.full_rows>MAX_ROWS{scan_complete=false;break;}
                        let id=row.get::<_,Option<String>>(0);let raw=row.get::<_,Option<String>>(1);
                        let bytes=id.as_ref().ok().and_then(|s|s.as_ref()).map_or(0,|s|s.len())+raw.as_ref().ok().and_then(|s|s.as_ref()).map_or(0,|s|s.len());
                        if !work.bytes(bytes){scan_complete=false;break;}
                        match (id,raw) {
                            (Ok(Some(id)),Ok(Some(raw)))=>{if !work.accept(id,raw,&mut tasks,&mut seen){scan_complete=false;break;}},
                            (Ok(_),Ok(_))=>{work.rejected+=1;}, // bounded/NULL values are invalid, not damaged pages
                            _=>{work.rejected+=1;work.unreadable+=1;scan_complete=false;}
                        }
                    },
                    Ok(None)=>break,
                    Err(_)=>{work.unreadable+=1;scan_complete=false;break;}
                }
            },
            Err(_)=>scan_complete=false
        }
    }else{scan_complete=false;}
    // Payload conversion can fail even when rows.next() succeeds (overflow-page
    // damage). Such failures above MUST trigger the independent index fallback.
    let mut indexed_fallback=false;let mut index_scan_complete=false;let mut lookup_complete=false;
    let mut schema_verified=false;
    if !scan_complete && work.available(deadline) {
        schema_verified=known_task_index(db,deadline);
        if schema_verified && work.available(deadline) {
            indexed_fallback=true;let mut keys=Vec::new();let mut key_seen=std::collections::HashSet::new();let mut index_errors=false;
            if let Ok(mut stmt)=db.prepare("select case when length(cast(id as blob)) <= 2000 then id else null end from tasks indexed by sqlite_autoindex_tasks_1 limit 10001") {
                if let Ok(mut rows)=stmt.query([]) {loop {
                    if !work.available(deadline) || MAX_READ_BYTES.saturating_sub(work.read_bytes)<2000{work.budget_exhausted=true;break;}
                    match rows.next() {
                        Ok(Some(row))=>{
                            work.index_rows+=1;
                            if work.index_rows>MAX_ROWS{break;}
                            match row.get::<_,String>(0) {
                                Ok(id)=>{if !work.bytes(id.len()){break;}if id.is_empty(){work.rejected+=1;}else if !key_seen.insert(id.clone()){work.duplicates+=1;}else{keys.push(id);}},
                                Err(_)=>{work.rejected+=1;work.unreadable+=1;index_errors=true;}
                            }
                        },
                        Ok(None)=>{index_scan_complete= !index_errors;break;},
                        Err(_)=>{work.unreadable+=1;break;}
                    }
                }}
            }
            lookup_complete=true;
            for id in keys {
                if !work.available(deadline){lookup_complete=false;break;}
                // Every enumerated key is charged, including already accepted
                // duplicates; failed/invalid lookups cannot evade the work cap.
                if seen.contains(&id){work.duplicates+=1;continue;}
                if MAX_READ_BYTES.saturating_sub(work.read_bytes)<MAX_PAYLOAD{work.budget_exhausted=true;lookup_complete=false;break;}
                work.lookups+=1;
                let raw=db.query_row("select case when length(cast(payload as blob)) <= 1048576 then payload else null end from tasks indexed by sqlite_autoindex_tasks_1 where id=?1",[&id],|r|r.get::<_,String>(0));
                match raw {
                    Ok(raw)=>{if !work.bytes(raw.len()) || !work.accept(id,raw,&mut tasks,&mut seen){lookup_complete=false;break;}},
                    Err(_)=>{work.rejected+=1;work.unreadable+=1;lookup_complete=false;}
                }
            }
        }
    }
    let mut report=json!({"indexed_fallback":indexed_fallback,"fallback_schema_verified":schema_verified,"index_scan_complete":index_scan_complete,"indexed_lookups_complete":lookup_complete,"unreadable_rows":work.unreadable,"duplicate_rows":work.duplicates,"rejected_rows":work.rejected,"full_scan_rows":work.full_rows,"indexed_rows":work.index_rows,"indexed_lookups":work.lookups,"row_work":work.full_rows+work.index_rows+work.lookups,"accepted_payload_bytes":work.accepted_bytes,"cumulative_read_bytes":work.read_bytes,"budget_exhausted":work.budget_exhausted,"sql_deadline_exceeded":std::time::Instant::now()>=deadline,"counts_are_best_effort":true,"row_counters_are_attempts":true,"full_scan_row_limit":MAX_ROWS,"indexed_row_limit":MAX_ROWS,"total_row_work_limit":MAX_WORK,"accepted_payload_byte_limit":MAX_ACCEPTED_BYTES,"cumulative_read_byte_limit":MAX_READ_BYTES,"sql_deadline_seconds":SQL_SECONDS,"source_task_count":known,"omitted_tasks":known.map(|n|(n-tasks.len() as i64).max(0)),"scan_complete":scan_complete,"events_recovered":0,"omitted_events":omitted_events,"omitted_source_archive_rows":omitted_archives,"historical_completion_episodes_recovered":0,"timing_recovered":false,"recovered_task_rows":tasks.len(),"recovered_tasks":0,"destination_integrity_ok":false});
    if tasks.is_empty(){report["status"]=json!("no_readable_tasks");return Ok(report);}
    fs::create_dir(destination).map_err(|_| "恢复目录创建失败")?;
    let target = Store::open(destination,false)?;
    target.db.execute("delete from store_migrations where name='task_retention_v1'",[]).map_err(|_| "恢复迁移准备失败")?;
    for (id,task) in &tasks {
        target.db.execute("insert into tasks(id,payload) values(?1,?2)", params![id,task.to_string()]).map_err(|_| "恢复任务保存失败")?;
    }
    drop(target);
    // Real Store migration reconstructs only the currently completed snapshot,
    // using deterministic archive:<sha256(task id)>:<revision>, with no events.
    let mut target = Store::open(destination,false)?;
    let completed: i64=target.db.query_row("select count(*) from completed_tasks",[],|r|r.get(0)).map_err(|_| "恢复完成记录检查失败")?;
    if !integrity(&target.db) { return Err("恢复副本未通过完整性或应用读取检查".into()); }
    // Count through the real application projection: unreported placeholders
    // remain preserved in tasks, but must not inflate prepare/apply/health counts.
    let visible=target.tasks().map_err(|_| "恢复副本未通过完整性或应用读取检查")?.len();
    report["recovered_tasks"]=json!(visible);
    target.db.execute_batch("pragma wal_checkpoint(TRUNCATE);").map_err(|_| "恢复副本收尾失败")?;
    report["status"]=json!("partial_recovery");report["destination_integrity_ok"]=json!(true);report["reconstructed_current_completions"]=json!(completed);Ok(report)
}

#[cfg(test)]
mod bounded_fallback_tests {
    use super::*;
    #[test]
    fn fallback_requires_known_store_primary_key_schema(){
        let db=Connection::open_in_memory().unwrap();let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
        db.execute_batch("create table tasks(id text primary key,payload text not null)").unwrap();assert!(known_task_index(&db,deadline));
        db.execute_batch("drop table tasks;create table tasks(id text,payload text not null);create unique index unrelated_pk on tasks(id)").unwrap();assert!(!known_task_index(&db,deadline));
        db.execute_batch("drop table tasks;create table tasks(id text primary key collate nocase,payload text not null)").unwrap();assert!(!known_task_index(&db,deadline));
    }
    #[test]
    fn read_and_accepted_budgets_charge_invalid_and_duplicate_work(){
        let mut work=SalvageWork::default();let mut tasks=Vec::new();let mut seen=std::collections::HashSet::new();
        let raw=json!({"id":"one","revision":1,"status":"running","plan":[],"blockers":[],"evidence":[]}).to_string();
        assert!(work.bytes(raw.len()));assert!(work.accept("one".into(),raw.clone(),&mut tasks,&mut seen));
        assert!(work.bytes(raw.len()));assert!(work.accept("one".into(),raw.clone(),&mut tasks,&mut seen));
        assert_eq!(work.duplicates,1);assert_eq!(tasks.len(),1);assert_eq!(work.read_bytes,raw.len()*2);
        assert!(work.bytes(3));assert!(work.accept("bad".into(),"bad".into(),&mut tasks,&mut seen));assert_eq!(work.rejected,1);
        work.read_bytes=MAX_READ_BYTES;assert!(!work.bytes(1));assert!(work.budget_exhausted);
        work.accepted_bytes=MAX_ACCEPTED_BYTES;assert!(!work.accept("two".into(),raw.replace("one","two"),&mut tasks,&mut seen));
    }
    #[test]
    fn cancelled_watchdog_is_joined_before_connection_reuse(){
        let db=Connection::open_in_memory().unwrap();
        assert_eq!(with_sql_deadline(&db,|_|db.query_row("select 1",[],|r|r.get::<_,i64>(0)).unwrap()),1);
        assert_eq!(db.query_row("select 2",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    }
}

/// `root` is the caller-owned source data root, never a UI-supplied path.
/// inspect/recover_copy create independent evidence; snapshot reads last report only.
/// No apply action exists. Unknown params are refused to prevent path injection.
pub fn call(root: &Path, action: &str, params: &Value) -> Result<Value> {
    if !["inspect","recover_copy","snapshot"].contains(&action) { return Err("不支持原地修复或应用恢复数据库".into()); }
    let obj=params.as_object().ok_or("恢复参数必须是对象")?;
    if obj.keys().any(|k| k!="confirmed") {return Err("恢复不接受自定义路径或额外参数".into());}
    if action=="recover_copy" && params["confirmed"]!=true {return Err("创建恢复副本需要明确确认".into());}
    plain(root)?;
    if !root.is_dir() {return Err("源数据库目录无效".into());}
    let root=fs::canonicalize(root).map_err(|_|"源数据库目录无法解析")?;
    let parent=root.parent().ok_or("源数据库目录无效")?;
    // Fixed sibling, rather than bridge::control_root (which may equal root).
    let base=parent.join("control");
    if base==root {return Err("恢复控制目录不得与源目录相同".into());}
    if base.exists(){plain(&base)?;} else {fs::create_dir(&base).map_err(|_|"恢复控制目录创建失败")?;}
    let base=base.join("database-recovery");
    if base.exists(){plain(&base)?;} else {fs::create_dir(&base).map_err(|_|"恢复目录创建失败")?;}
    if action=="snapshot" {
        let report=base.join("latest.json");
        if !report.exists(){return Ok(json!({"status":"not_started","apply_supported":false}));}
        plain(&report)?;
        if fs::metadata(&report).map_err(|_|"恢复报告不可读")?.len()>65536{return Err("恢复报告过大".into());}
        return crate::store::read_json(&report);
    }
    let before=fingerprints(&root)?;
    if before[FILES[0]].is_null(){return Err("未找到源数据库；不会创建或重建源数据库".into());}
    let id=uuid::Uuid::new_v4().to_string(); let work=base.join(&id);
    fs::create_dir(&work).map_err(|_|"独立恢复目录创建失败")?;
    let evidence=work.join("source-copy");fs::create_dir(&evidence).map_err(|_|"证据目录创建失败")?;
    for name in FILES {if !before[name].is_null(){copy_file(&root.join(name),&evidence.join(name))?;}}
    let copied=fingerprints(&evidence)?; let after=fingerprints(&root)?;
    let stable=before==after && before==copied;
    let mut report=json!({"version":1,"operation":action,"recovery_id":id,"status":"unstable_source","source_unchanged_during_copy":stable,"coordinated_snapshot":false,"source_consistency":"uncoordinated_copy_not_a_transactional_snapshot","source_files":before,"copied_files":copied,"source_files_after":after,"apply_supported":false,"partial":true,"paths":{"evidence":format!("control/database-recovery/{id}/source-copy"),"report":format!("control/database-recovery/{id}/report.json")},"warning":"仅复制，不修改原数据库；即使哈希稳定也不是协调快照。恢复不包含事件历史或计时，不支持直接应用。"});
    if stable {
        // SQLite may maintain the copied SHM even on read-only connection. Keep
        // exact evidence bytes separate from disposable analysis files.
        let analysis=work.join("analysis");fs::create_dir(&analysis).map_err(|_|"分析目录创建失败")?;
        for name in FILES {if !before[name].is_null(){copy_file(&evidence.join(name),&analysis.join(name))?;}}
        match readonly(&analysis) {
            Ok(db)=>with_sql_deadline(&db,|deadline| {
                report["source_copy_integrity_ok"]=json!(integrity(&db));
                report["status"]=json!("inspected_copy");
                if action=="recover_copy" {
                    // Full preservation has priority. A failing full-copy must
                    // never leave a half-built target for task-only salvage.
                    let full=complete_copy(&analysis,&work.join("complete-data"),deadline);
                    let (result,path)=match full{
                        Ok(result)=>(Ok(result),"complete-data"),
                        Err(error)=>{report["full_recovery_error"]=json!(error);(salvage(&db,&work.join("recovered-data"),deadline),"recovered-data")}
                    };
                    match result {
                        Ok(mut result)=>{if result["status"]!="full_recovery"{result["mode"]=json!("partial");result["history_recovered"]=json!(false);}report["status"]=result["status"].clone();report["partial"]=json!(result["status"]!="full_recovery");if result["destination_integrity_ok"]==true {report["paths"]["recovered_database"]=json!(format!("control/database-recovery/{id}/{path}/events.sqlite3"));}report["recovery"]=result;},
                        Err(_)=>{report["status"]=json!("recovery_failed");report["error"]=json!("恢复副本未通过检查，不能使用；原文件未改动");}
                    }
                }
                report["sql_deadline_exceeded"]=json!(std::time::Instant::now()>=deadline);
            }),
            Err(_)=>{report["status"]=json!("unreadable_copy");report["source_copy_integrity_ok"]=json!(false);}
        }
    }
    report["warning"]=json!(if report["status"]=="full_recovery"{"完整副本已验证，事件、耗时和历史归档均保留；尚未切换数据。字节稳定不代表已协调暂停在线写入。"}else if report["status"]=="partial_recovery"{"仅能部分抢救任务快照，事件历史和耗时不能恢复；此副本不可自动启用，原数据和日志已保留。"}else{"副本未通过完整验证，不可启用；原数据和日志已保留。"});
    crate::store::atomic_json(&work.join("report.json"),&report)?;
    crate::store::atomic_json(&base.join("latest.json"),&report)?;
    Ok(report)
}
