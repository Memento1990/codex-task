//! Copy-first repair; original DB/WAL are never opened with SQLite or replaced.
use std::{fs,io::Read,path::{Path,PathBuf}};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use crate::{Result,migration,runtime::ProcessLock,store::{atomic_json,read_json,Store}};
const MAX_FILE:u64=256*1024*1024;
const MAX_STATE:u64=8*1024*1024;
fn source(root:&Path)->Result<PathBuf>{migration::check_path(root)?;let root=fs::canonicalize(root).map_err(|_|"修复数据目录不可用")?;let root=PathBuf::from(root.to_string_lossy().trim_start_matches(r"\\?\"));migration::check_path(&root)?;if !root.is_dir(){return Err("修复数据目录无效".into());}Ok(root)}
fn base(root:&Path)->Result<PathBuf>{let p=root.parent().ok_or("修复数据目录无效")?.join("control/repair-transactions");migration::check_path(&p)?;if p.starts_with(root){return Err("修复控制目录不得位于源目录内".into());}Ok(p)}
fn valid(id:&str)->bool{uuid::Uuid::parse_str(id).is_ok_and(|v|v.to_string()==id)}
fn digest(path:&Path,max:u64)->Result<String>{migration::check_path(path)?;let m=fs::metadata(path).map_err(|_|"修复文件不可读")?;if !m.is_file()||m.len()>max{return Err("修复文件不是普通文件或超过大小限制".into());}let mut f=fs::File::open(path).map_err(|_|"修复文件不可读")?;let(mut hash,mut count,mut buf)=(Sha256::new(),0u64,[0u8;65536]);loop{let n=f.read(&mut buf).map_err(|_|"修复文件读取失败")?;if n==0{break;}count+=n as u64;if count>max{return Err("修复文件超过大小限制".into());}hash.update(&buf[..n]);}Ok(format!("{:x}",hash.finalize()))}
fn bounded(path:&Path)->Result<Value>{migration::check_path(path)?;let m=fs::metadata(path).map_err(|_|"修复状态不可读")?;if !m.is_file()||m.len()>MAX_STATE{return Err("修复状态过大或不是普通文件".into());}read_json(path)}
fn save(root:&Path,j:&Value)->Result<()>{let id=j["id"].as_str().filter(|id|valid(id)).ok_or("修复事务标识无效")?;let p=base(root)?.join(id);migration::check_path(&p)?;fs::create_dir_all(&p).map_err(|_|"修复状态目录不可写")?;atomic_json(&p.join("journal.json"),j)?;atomic_json(&base(root)?.join("latest.json"),&json!({"version":1,"id":id}))}
fn target(j:&Value)->Result<PathBuf>{Ok(PathBuf::from(j["destination"].as_str().ok_or("修复目标缺失")?))}
fn load(root:&Path,id:&str)->Result<Value>{if !valid(id){return Err("修复事务标识无效".into());}let j=bounded(&base(root)?.join(id).join("journal.json"))?;let expected=root.parent().ok_or("修复目录无效")?.join(format!("data-repaired-{id}"));if j["version"]!=1||j["id"]!=id||j["source"]!=json!(root)||j["destination"]!=json!(expected){return Err("修复事务归属不匹配".into());}migration::check_path(&expected)?;Ok(j)}
fn seal(root:&Path)->Result<()>{let s=Store::open(root,false)?;s.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").map_err(|_|"恢复副本仍被占用，未切换数据")?;Ok(())}
fn verify(root:&Path)->Result<usize>{let mut s=Store::open(root,true)?;let check:String=s.db.query_row("PRAGMA integrity_check",[],|r|r.get(0)).map_err(|_|"恢复数据库检查失败")?;if check!="ok"{return Err("恢复数据库完整性未通过".into());}let n=s.tasks()?.len();if n==0{return Err("没有可恢复任务，不会启用空数据库".into());}Ok(n)}
/// Stages a replacement, never changes the active root.
pub fn prepare(root:&Path)->Result<Value>{let root=source(root)?;if migration::resolve_root(&root)?!=root{return Err("数据目录已经切换，请重启看板".into());}let _lock=ProcessLock::acquire(&base(&root)?.join("repair.lock"))?;let _migration=ProcessLock::acquire(&root.join(migration::LOCK))?;
 let report=crate::database_repair::call(&root,"recover_copy",&json!({"confirmed":true}))?;
 if report["status"]!="partial_recovery"||report["source_unchanged_during_copy"]!=true||report["recovery"]["destination_integrity_ok"]!=true||report["recovery"]["recovered_tasks"].as_u64().unwrap_or(0)==0{return Err("未找到通过验证的可恢复任务；原数据保持不变".into());}
 let recovery_id=report["recovery_id"].as_str().filter(|id|valid(id)).ok_or("恢复副本标识无效")?;let recovered=root.parent().unwrap().join("control/database-recovery").join(recovery_id).join("recovered-data");let count=verify(&recovered)?;seal(&recovered)?;if report["recovery"]["recovered_tasks"].as_u64()!=Some(count as u64){return Err("恢复任务数量不一致".into());}
 let id=uuid::Uuid::new_v4().to_string();let dest=root.parent().unwrap().join(format!("data-repaired-{id}"));migration::check_path(&dest)?;fs::create_dir(&dest).map_err(|_|"恢复目录创建失败")?;let hash=digest(&recovered.join("events.sqlite3"),MAX_FILE)?;fs::copy(recovered.join("events.sqlite3"),dest.join("events.sqlite3")).map_err(|_|"恢复数据库复制失败")?;if digest(&dest.join("events.sqlite3"),MAX_FILE)?!=hash||verify(&dest)?!=count{return Err("恢复目标验证失败".into());}seal(&dest)?;let hash=digest(&dest.join("events.sqlite3"),MAX_FILE)?;
 let j=json!({"version":1,"id":id,"source":root,"destination":dest,"phase":"prepared","created_at":crate::now(),"recovered_tasks":count,"recovered_task_rows":report["recovery"]["recovered_task_rows"],"database_sha256":hash,"source_files":report["source_files"],"recovery_id":recovery_id,"partial":true,"history_recovered":false,"timing_recovered":false,"baseline_reset":true,"original_preserved":true});save(&root,&j)?;Ok(j)}
/// A committed redirect is never rolled back: its target may contain new events.
pub fn status(root:&Path)->Result<Value>{let root=source(root)?;let p=base(&root)?.join("latest.json");if !p.exists(){return Ok(json!({"phase":"not_started"}));}let latest=bounded(&p)?;let mut j=load(&root,latest["id"].as_str().ok_or("修复状态标识无效")?)?;let current=migration::resolve_root(&root)?;if current==target(&j)?{j["phase"]=json!("committed");}else if current!=root{return Err("目录已由其他事务切换".into());}Ok(j)}
// Deny concurrent write/delete handles without opening the source with SQLite.
// Held through durable redirect; unknown direct writers fail closed.
fn source_guards(root:&Path)->Result<Vec<fs::File>>{
 use std::os::windows::fs::OpenOptionsExt;
 let mut guards=Vec::new();
 for name in ["events.sqlite3","events.sqlite3-wal","events.sqlite3-shm"]{
  let path=root.join(name);migration::check_path(&path)?;
  match fs::OpenOptions::new().read(true).share_mode(1).open(&path){
   Ok(file)=>guards.push(file),
   Err(e) if e.kind()==std::io::ErrorKind::NotFound && name!="events.sqlite3"=>{},
   Err(_)=>return Err("原数据库仍被其他写入程序占用，未切换数据".into()),
  }
 }
 Ok(guards)
}
fn unchanged(root:&Path,expected:&Value)->Result<()>{for name in ["events.sqlite3","events.sqlite3-wal","events.sqlite3-shm"]{let p=root.join(name);migration::check_path(&p)?;if expected[name].is_null(){if p.exists(){return Err("原数据已变化，请重新准备修复".into());}}else if expected[name]["sha256"].as_str()!=Some(digest(&p,MAX_FILE)?.as_str()){return Err("原数据已变化，请重新准备修复".into());}}Ok(())}
fn copy_entry(from:&Path,to:&Path,total:&mut u64,entries:&mut usize)->Result<()>{migration::check_path(from)?;migration::check_path(to)?;let m=fs::metadata(from).map_err(|_|"应用状态不可读")?;*entries+=1;if *entries>10000{return Err("应用状态文件过多".into());}if m.is_dir(){fs::create_dir(to).map_err(|_|"状态目录创建失败")?;for e in fs::read_dir(from).map_err(|_|"状态目录不可读")?{let e=e.map_err(|_|"状态文件不可读")?;copy_entry(&e.path(),&to.join(e.file_name()),total,entries)?;}}else if m.is_file(){*total=total.checked_add(m.len()).ok_or("状态过大")?;if m.len()>MAX_STATE||*total>64*1024*1024{return Err("应用状态超过大小限制".into());}let h=digest(from,MAX_STATE)?;fs::copy(from,to).map_err(|_|"状态复制失败")?;if digest(from,MAX_STATE)?!=h||digest(to,MAX_STATE)?!=h{return Err("应用状态复制期间变化".into());}}else{return Err("应用状态包含特殊文件".into());}Ok(())}
fn carry(root:&Path,dest:&Path)->Result<()>{let(mut total,mut entries)=(0,0);for name in ["preferences.json","window.json","sessions","integration.json"]{let p=root.join(name);migration::check_path(&p)?;if p.exists(){copy_entry(&p,&dest.join(name),&mut total,&mut entries)?;}}
 let p=root.join("weixin-production/account.dpapi");migration::check_path(&p)?;if p.exists(){let dir=dest.join("weixin-production");fs::create_dir(&dir).map_err(|_|"微信恢复目录创建失败")?;copy_entry(&p,&dir.join("account.dpapi"),&mut total,&mut entries)?;}
 let p=root.join("notifications.json");migration::check_path(&p)?;if p.exists(){let mut state=bounded(&p)?;if !state.is_object(){return Err("通知状态无效".into());}state["baseline_reset"]=json!(true);state["baseline"]=json!({});if let Some(records)=state["records"].as_array_mut(){for r in records{if ["sending","in_flight"].contains(&r["status"].as_str().unwrap_or("")) || (r["status"]=="pending" && (r["attempts"].as_u64().unwrap_or(0)>0 || r["lease_until"].as_f64().unwrap_or(0.)>0.)){ r["status"]=json!("unknown");r["lease_until"]=json!(0);r["error"]=json!("修复前发送结果未确认，不自动重试");}}}atomic_json(&dest.join("notifications.json"),&state)?;}Ok(())}
fn origins(root:&Path)->Result<Vec<PathBuf>>{let p=root.join("migration-origins.json");migration::check_path(&p)?;let mut roots=vec![];if p.exists(){let j=bounded(&p)?;for v in j["roots"].as_array().ok_or("旧目录记录无效")?{let old=source(Path::new(v.as_str().ok_or("旧目录记录无效")?))?;if migration::resolve_root(&old)?!=root{return Err("旧目录归属不一致".into());}if !roots.contains(&old){roots.push(old);}}}if !roots.contains(&root.to_path_buf()){roots.push(root.to_path_buf());}Ok(roots)}
/// Helper-only apply. Caller has normally exited GUI/notification service.
/// Runtime owns the live collector identities continuously across redirect and
/// activates the destination itself. Legacy collectors cannot authorize commit.
pub fn apply(root:&Path,id:&str)->Result<Value>{let root=source(root)?;let _transaction=ProcessLock::acquire(&base(&root)?.join("repair.lock"))?;let mut j=load(&root,id)?;let dest=target(&j)?;
 if migration::resolve_root(&root)?==dest{j["phase"]=json!("committed");return Ok(j);}
 if migration::resolve_root(&root)?!=root||j["phase"]!="prepared"{return Err("修复事务已变化，未切换数据".into());}
 let _notifications=ProcessLock::acquire(&root.join("notification-activity.lock")).map_err(|_|"通知正在处理，请稍后修复")?;let _integration=ProcessLock::acquire(&root.join("integration.lock"))?;
 crate::runtime::with_repair_pause(&root,|pause|{
  pause.verify()?;let _source_guards=source_guards(&root)?;let _destination=ProcessLock::acquire(&dest.join("collector.lock"))?;let _target_migration=ProcessLock::acquire(&dest.join(migration::LOCK))?;
  if j["database_sha256"].as_str()!=Some(digest(&dest.join("events.sqlite3"),MAX_FILE)?.as_str())||verify(&dest)? as u64!=j["recovered_tasks"].as_u64().ok_or("恢复任务数量缺失")?{return Err("恢复目标已变化，未切换数据".into());}
  // GUI/collector shutdown can legitimately checkpoint WAL and remove SHM.
  // Re-salvage under the proven pause rather than accepting stale snapshots or
  // rejecting every harmless checkpoint after preparation.
  let fresh=crate::database_repair::call(&root,"recover_copy",&json!({"confirmed":true}))?;
  if fresh["status"]!="partial_recovery"||fresh["source_unchanged_during_copy"]!=true||fresh["recovery"]["destination_integrity_ok"]!=true||fresh["recovery"]["recovered_tasks"].as_u64().unwrap_or(0)==0{return Err("暂停后的恢复副本未通过验证，未切换数据".into());}
  let rid=fresh["recovery_id"].as_str().filter(|id|valid(id)).ok_or("恢复标识无效")?;
  let recovered=root.parent().unwrap().join("control/database-recovery").join(rid).join("recovered-data");let count=verify(&recovered)?;seal(&recovered)?;
  if count as u64!=fresh["recovery"]["recovered_tasks"].as_u64().ok_or("恢复数量缺失")?{return Err("恢复数量不一致".into());}
  for name in ["events.sqlite3-wal","events.sqlite3-shm"]{if dest.join(name).exists(){return Err("恢复目标出现活动日志，未切换数据".into());}}
  let bytes=fs::read(recovered.join("events.sqlite3")).map_err(|_|"恢复副本不可读")?;if bytes.len() as u64>MAX_FILE{return Err("恢复副本过大".into());}
  crate::store::atomic_bytes(&dest.join("events.sqlite3"),&bytes)?;
  if verify(&dest)?!=count{return Err("最终恢复目标验证失败".into());}seal(&dest)?;
  j["source_files"]=fresh["source_files"].clone();j["recovery_id"]=json!(rid);j["recovered_tasks"]=json!(count);j["recovered_task_rows"]=fresh["recovery"]["recovered_task_rows"].clone();j["database_sha256"]=json!(digest(&dest.join("events.sqlite3"),MAX_FILE)?);
  let roots=origins(&root)?;
  // Runtime's pause must protect every legacy identity, not just this root.
  let ack=bounded(&root.join(migration::ACK))?;let held=ack["roots"].as_array().ok_or("旧目录保护缺失")?;for old in &roots{if !held.iter().any(|p|p.as_str().is_some_and(|p|Path::new(p)==old)){return Err("旧目录采集身份未受保护，未切换数据".into());}}
  // Mark first so a crash/partial state copy cannot be silently retried over a
  // possibly modified destination. A fresh preparation always gets a new UUID.
  j["phase"]=json!("applying");save(&root,&j)?;carry(&root,&dest)?;atomic_json(&dest.join("migration-origins.json"),&json!({"roots":roots}))?;
  unchanged(&root,&j["source_files"])?;pause.verify()?;
  j["phase"]=json!("commit_pending");save(&root,&j)?;
  atomic_json(&root.join(migration::REDIRECT),&json!({"version":1,"path":dest,"at":crate::now(),"repair_id":id}))?;
  // Redirect is the durable commit point. Do not turn journal write failure into
  // a reported pre-commit failure; status reconciles commit_pending by redirect.
  j["phase"]=json!("committed");j["committed_at"]=json!(crate::now());let _=save(&root,&j);Ok(j.clone())
 })}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn direct_writer_is_rejected_and_guard_denies_new_writes(){
  let root=std::env::temp_dir().join(format!("floatboard-repair-guard-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();let path=root.join("events.sqlite3");fs::write(&path,b"fixture").unwrap();
  let writer=fs::OpenOptions::new().write(true).open(&path).unwrap();assert!(source_guards(&root).is_err());drop(writer);
  let guards=source_guards(&root).unwrap();assert!(fs::OpenOptions::new().write(true).open(&path).is_err());assert!(fs::remove_file(&path).is_err());drop(guards);
  assert_eq!(fs::read(&path).unwrap(),b"fixture");fs::remove_dir_all(&root).unwrap();
 }
}
