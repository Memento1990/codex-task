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
fn salvage(db: &Connection, destination: &Path) -> Result<Value> {
    let known: Option<i64> = db.query_row("select count(*) from tasks",[],|r|r.get(0)).ok();
    let omitted_events: Option<i64> = db.query_row("select count(*) from events",[],|r|r.get(0)).ok();
    let omitted_archives: Option<i64> = db.query_row("select count(*) from completed_tasks",[],|r|r.get(0)).ok();
    let mut tasks = Vec::new(); let mut rejected = 0usize; let mut scan_complete = true; let mut recovered_bytes=0usize;
    if let Ok(mut stmt) = db.prepare("select case when length(cast(id as blob)) <= 2000 then id else null end, case when length(cast(payload as blob)) <= 1048576 then payload else null end from tasks limit 10001") {
        match stmt.query([]) {
            Ok(mut rows) => loop {
                match rows.next() {
                    Ok(Some(row)) => {
                        if tasks.len()+rejected >= MAX_ROWS { scan_complete=false; break; }
                        let id: rusqlite::Result<String> = row.get(0); let raw: rusqlite::Result<String> = row.get(1);
                        if let (Ok(id),Ok(raw)) = (id,raw) {
                            if recovered_bytes.saturating_add(raw.len()) > 32*1024*1024 { scan_complete=false; break; }
                            if let Some(task)=sanitized_task(&id,&raw) { recovered_bytes+=raw.len(); tasks.push((id,task)); } else { rejected+=1; }
                        } else { rejected+=1; }
                    },
                    Ok(None) => break,
                    Err(_) => {scan_complete=false;break;}
                }
            },
            Err(_) => scan_complete=false,
        }
    } else { scan_complete=false; }
    if tasks.is_empty() {
        return Ok(json!({"status":"no_readable_tasks","recovered_tasks":0,"rejected_rows":rejected,"source_task_count":known,"omitted_tasks":known,"scan_complete":scan_complete,"events_recovered":0,"omitted_events":omitted_events,"omitted_source_archive_rows":omitted_archives,"timing_recovered":false,"destination_integrity_ok":false}));
    }
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
    if !integrity(&target.db) || target.tasks().is_err() { return Err("恢复副本未通过完整性或应用读取检查".into()); }
    target.db.execute_batch("pragma wal_checkpoint(TRUNCATE);").map_err(|_| "恢复副本收尾失败")?;
    Ok(json!({"status":"partial_recovery","recovered_tasks":tasks.len(),"reconstructed_current_completions":completed,"rejected_rows":rejected,"source_task_count":known,"counts_are_best_effort":true,"omitted_tasks":known.map(|n|(n-tasks.len() as i64).max(0)),"scan_complete":scan_complete,"destination_integrity_ok":true,"events_recovered":0,"omitted_events":omitted_events,"omitted_source_archive_rows":omitted_archives,"historical_completion_episodes_recovered":0,"timing_recovered":false}))
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
            Ok(db)=>{
                report["source_copy_integrity_ok"]=json!(integrity(&db));
                report["status"]=json!("inspected_copy");
                if action=="recover_copy" {
                    match salvage(&db,&work.join("recovered-data")) {
                        Ok(result)=>{report["status"]=result["status"].clone();if result["destination_integrity_ok"]==true {report["paths"]["recovered_database"]=json!(format!("control/database-recovery/{id}/recovered-data/events.sqlite3"));}report["recovery"]=result;},
                        Err(_)=>{report["status"]=json!("recovery_failed");report["error"]=json!("恢复副本未通过检查，不能使用；原文件未改动");}
                    }
                }
            },
            Err(_)=>{report["status"]=json!("unreadable_copy");report["source_copy_integrity_ok"]=json!(false);}
        }
    }
    crate::store::atomic_json(&work.join("report.json"),&report)?;
    crate::store::atomic_json(&base.join("latest.json"),&report)?;
    Ok(report)
}
