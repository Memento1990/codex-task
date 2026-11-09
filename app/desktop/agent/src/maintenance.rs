//! Explicit offline maintenance. Inputs are copied and hashed; SQLite never opens originals.
use std::{collections::{BTreeMap,BTreeSet},fs,path::{Path,PathBuf}};
use serde::{Serialize,Deserialize};
use serde_json::{json,Value};
use rusqlite::{Connection,OpenFlags,params};
use floatboard_client::{Profile,windows::Instance,files::{read_json,atomic_json}};
use floatboard_protocol::{Result,hash,canonical,now};
use crate::storage::Database;
use base64::Engine;

#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source{pub label:String,pub role:String,pub database:PathBuf,#[serde(default)]pub main_only:bool}
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateSource{pub label:String,pub kind:String,pub path:PathBuf}
#[derive(Clone,Serialize,Deserialize)]
struct Record{table:String,id:String,revision:i64,digest:String,payload:Value,columns:Value,sources:Vec<String>}
fn hashes(source:&Source)->Result<Value>{
    let mut result=serde_json::Map::new();
    for suffix in ["","-wal","-shm"]{let path=PathBuf::from(format!("{}{suffix}",source.database.display()));if path.exists(){let bytes=fs::read(&path).map_err(|_|"来源文件不可读")?;result.insert(suffix.into(),json!({"sha256":hash(&bytes),"bytes":bytes.len()}));}}
    if !result.contains_key(""){return Err("来源主库缺失".into());}Ok(Value::Object(result))
}
fn integrity(db:&Connection)->Result<()>{let mut stmt=db.prepare("pragma integrity_check").map_err(|_|"完整性检查不可用")?;let rows=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(|_|"完整性检查失败")?;let results=rows.collect::<std::result::Result<Vec<_>,_>>().map_err(|_|"完整性结果不可读")?;if results!=["ok"]{return Err(format!("完整性不通过：{}",results.join(";")));}Ok(())}
fn read_records(db:&Connection,label:&str)->Result<Vec<Record>>{
    let mut result=Vec::new();
    for table in ["events","tasks","completed_tasks"]{
        let exists:i64=db.query_row("select count(*) from sqlite_master where type='table' and name=?1",[table],|r|r.get(0)).map_err(|_|"来源schema不可读")?;
        if exists==0{continue;}
        let query=match table{"events"=>"select id,payload,task_id,kind,at from events","completed_tasks"=>"select id,payload,source_task_id,revision,0 from completed_tasks",_=>"select id,payload,'',0,0 from tasks"};let mut stmt=db.prepare(query).map_err(|_|"来源表不可读")?;
        let rows=stmt.query_map([],|r|{let columns=match table{"events"=>json!({"task_id":r.get::<_,String>(2)?,"kind":r.get::<_,String>(3)?,"at":r.get::<_,f64>(4)?}),"completed_tasks"=>json!({"source_task_id":r.get::<_,String>(2)?,"revision":r.get::<_,i64>(3)?}),_=>json!({})};Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,columns))}).map_err(|_|"来源记录不可读")?;
        for row in rows{let (id,raw,columns)=row.map_err(|_|"来源记录读取失败")?;let payload:Value=serde_json::from_str(&raw).map_err(|_|"来源记录JSON损坏")?;let revision=if table=="events"{payload["report"]["revision"].as_i64()}else{payload["revision"].as_i64()}.unwrap_or(-1);result.push(Record{table:table.into(),id,revision,digest:hash(canonical(&json!({"payload":payload,"columns":columns})).as_bytes()),payload,columns,sources:vec![label.into()]});}
    }Ok(result)
}
fn record_key(record:&Record)->String{format!("{}:{}:{}",record.table,record.id,if record.table=="events"{-1}else{record.revision})}
fn business(record:&Record)->Option<(String,String)>{if record.table!="events"||record.columns["kind"]!="progress"{return None;}let p=&record.payload;Some((format!("business:{}:{}",record.columns["task_id"].as_str().unwrap_or(""),record.revision),hash(canonical(&p["report"]).as_bytes())))}
fn coverage(records:&[Record])->Value{
    let mut revisions:BTreeMap<String,BTreeSet<i64>>=BTreeMap::new();
    for r in records.iter().filter(|r|r.table=="events"&&r.columns["kind"]=="progress"&&r.revision>=0){revisions.entry(r.columns["task_id"].as_str().unwrap_or("").into()).or_default().insert(r.revision);}
    let mut gaps=Vec::new();for(id,values)in &revisions{let mut previous=None;for revision in values{if let Some(old)=previous{if *revision>old+1{gaps.push(json!({"task_id":id,"after_revision":old,"before_revision":revision}));}}previous=Some(*revision);}}
    json!({"revision_gaps":gaps,"minimized_event_payloads":records.iter().filter(|r|r.table=="events"&&r.payload.as_object().is_some_and(|o|o.is_empty())).count(),"task_snapshots_without_progress_events":records.iter().filter(|r|r.table=="tasks"&&!revisions.contains_key(&r.id)).map(|r|r.id.clone()).collect::<Vec<_>>(),"timing_policy":"仅采用保留事件与完成档案中的证据；未保留的修订、运行起止及耗时保持未知或partial。"})
}

fn runtime_neutral(payload:&Value)->Value{let mut value=payload.clone();if let Some(object)=value.as_object_mut(){for key in ["runtime","runtime_at","updated_at","agents","active_tools"]{object.remove(key);}}value}
fn neutral_record(records:&[Record],key:&str)->Result<Record>{
    let versions=records.iter().filter(|r|record_key(r)==key).collect::<Vec<_>>();let first=*versions.first().ok_or("冲突记录不存在")?;
    if first.table!="tasks"||versions.iter().any(|r|runtime_neutral(&r.payload)!=runtime_neutral(&first.payload)){return Err("仅运行态时间差异可以使用unknown_runtime决议".into());}
    let mut merged=first.clone();merged.payload=runtime_neutral(&first.payload);merged.payload["runtime"]=json!("unknown");merged.payload["runtime_at"]=Value::Null;merged.payload["updated_at"]=Value::Null;merged.payload["agents"]=json!({});merged.payload["active_tools"]=json!({});merged.digest=hash(canonical(&merged.payload).as_bytes());merged.sources=versions.iter().flat_map(|r|r.sources.clone()).collect();Ok(merged)
}

pub fn preview(sources:&[Source],states:&[StateSource],output:&Path)->Result<Value>{
    if output.exists(){return Err("预览目录已存在，拒绝覆盖".into());}
    let labels=sources.iter().map(|s|&s.label).chain(states.iter().map(|s|&s.label)).collect::<BTreeSet<_>>();if sources.is_empty()||labels.len()!=sources.len()+states.len()||sources.iter().any(|s|s.label.is_empty()||s.role.is_empty())||states.iter().any(|s|s.label.is_empty()){return Err("来源名称和角色必须唯一且非空".into());}
    fs::create_dir_all(output).map_err(|_|"预览目录不可写")?;
    let mut manifest=Vec::new();let mut variants:BTreeMap<(String,String),Record>=BTreeMap::new();
    for (index,source) in sources.iter().enumerate(){
        let before=hashes(source)?;let stage=output.join(format!("source-{index}"));fs::create_dir(&stage).map_err(|_|"来源副本目录不可写")?;
        for suffix in ["","-wal","-shm"]{if source.main_only&&!suffix.is_empty(){continue;}let input=PathBuf::from(format!("{}{suffix}",source.database.display()));if input.exists(){fs::copy(input,stage.join(format!("events.sqlite3{suffix}"))).map_err(|_|"来源复制失败")?;}}
        if hashes(source)?!=before{return Err("来源在复制期间发生变化，请取得稳定副本".into());}
        let inspected=(||{let db=Connection::open_with_flags(stage.join("events.sqlite3"),OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_|"来源副本打不开")?;integrity(&db)?;let records=read_records(&db,&source.label)?;let counts=records.iter().fold(BTreeMap::<String,usize>::new(),|mut a,r|{*a.entry(r.table.clone()).or_default()+=1;a});db.backup("main",stage.join("consistent.sqlite3"),None::<fn(rusqlite::backup::Progress)>).map_err(|_|"来源一致副本生成失败")?;Ok::<_,String>((records,counts))})();
        match inspected{Ok((records,counts))=>{for record in records{let key=(record_key(&record),record.digest.clone());if let Some(old)=variants.get_mut(&key){old.sources.push(source.label.clone());}else{variants.insert(key,record);}}manifest.push(json!({"source":source,"files":before,"status":"ok","counts":counts,"mode":if source.main_only{"main-only; WAL未采用"}else{"main+WAL副本"}}));},Err(error)=>manifest.push(json!({"source":source,"files":before,"status":"unreadable","error":error}))}
        if hashes(source)?!=before{return Err("检查期间来源发生变化".into());}
    }
    let mut state_manifest=Vec::new();
    for (index,source) in states.iter().enumerate(){
        let bytes=fs::read(&source.path).map_err(|_|"设置来源不可读")?;
        let payload=if source.kind=="weixin_account"{
            floatboard_storage::weixin::validate_account_bytes(&bytes)?;
            json!(base64::engine::general_purpose::STANDARD.encode(&bytes))
        }else{
            let value:Value=serde_json::from_slice(&bytes).map_err(|_|"设置来源格式无效")?;
            match source.kind.as_str(){"preferences"=>{floatboard_protocol::preferences::validate_options(&value)?;},"window"=>{floatboard_protocol::preferences::validate_window(&value)?;},"notifications"=>{crate::notifications::import_state(&value)?;},_=>return Err("未知设置来源类型".into())}value
        };
        fs::write(output.join(format!("state-source-{index}.bin")),&bytes).map_err(|_|"设置来源副本不可写")?;
        if fs::read(&source.path).map_err(|_|"设置来源不可读")?!=bytes{return Err("设置来源在复制期间发生变化".into());}
        let record=Record{table:source.kind.clone(),id:"current".into(),revision:0,digest:hash(canonical(&payload).as_bytes()),payload,columns:json!({}),sources:vec![source.label.clone()]};
        let key=(record_key(&record),record.digest.clone());if let Some(old)=variants.get_mut(&key){old.sources.push(source.label.clone());}else{variants.insert(key,record);}
        state_manifest.push(json!({"source":source,"sha256":hash(&bytes),"bytes":bytes.len()}));
    }
    let records=variants.into_values().collect::<Vec<_>>();let mut groups:BTreeMap<String,BTreeSet<String>>=BTreeMap::new();
    for record in &records{groups.entry(record_key(record)).or_default().insert(record.digest.clone());if let Some((key,digest))=business(record){groups.entry(key).or_default().insert(digest);}}
    let conflicts=groups.into_iter().filter(|(_,v)|v.len()>1).map(|(key,versions)|json!({"key":key,"versions":versions})).collect::<Vec<_>>();
    let unreadable=manifest.iter().filter(|m|m["status"]!="ok").count();
    let preview=json!({"version":1,"created_at":now(),"sources":manifest,"state_sources":state_manifest,"records":records,"conflicts":conflicts,"unreadable_sources":unreadable,"activation_allowed":false,"notes":["原始任务记录、完成档案和事件分别保留；未依赖事件重放替代快照。","main-only与WAL副本分别登记，不能认定为协调快照。","缺失的事件、修订与计时不会补造。"]});
    atomic_json(&output.join("preview.json"),&preview)?;
    let receipt=json!({"preview_sha256":hash(&fs::read(output.join("preview.json")).map_err(|_|"预览不可读")?),"conflicts":conflicts.len(),"unreadable_sources":unreadable,"counts":records.iter().fold(BTreeMap::<String,usize>::new(),|mut a,r|{*a.entry(r.table.clone()).or_default()+=1;a}),"sources":preview["sources"],"state_sources":preview["state_sources"],"coverage":coverage(&records)});atomic_json(&output.join("summary.json"),&receipt)?;Ok(receipt)
}

pub fn apply(preview_path:&Path,resolutions:&Value,profile:Profile)->Result<Value>{
    let _owner=Instance::acquire(&profile.profile_id)?;
    let bytes=fs::read(preview_path).map_err(|_|"迁移预览不可读")?;
    if resolutions["preview_sha256"]!=hash(&bytes){return Err("决议与预览不匹配".into());}
    let preview:Value=serde_json::from_slice(&bytes).map_err(|_|"预览无效")?;
    for item in preview["sources"].as_array().ok_or("来源清单缺失")?{let source:Source=serde_json::from_value(item["source"].clone()).map_err(|_|"来源格式无效")?;if hashes(&source)?!=item["files"]{return Err("来源已变化，必须重新预览".into());}if item["status"]!="ok"&&resolutions["acknowledged_unreadable"].as_array().is_none_or(|a|!a.contains(&json!(source.label))){return Err("存在未确认的不可读来源".into());}}
    for item in preview["state_sources"].as_array().into_iter().flatten(){let path=item["source"]["path"].as_str().ok_or("设置来源路径缺失")?;if hash(&fs::read(path).map_err(|_|"设置来源不可读")?)!=item["sha256"]{return Err("设置来源已变化，必须重新预览".into());}}
    let choices=resolutions["choices"].as_object().ok_or("缺少逐项冲突决议")?;let records:Vec<Record>=serde_json::from_value(preview["records"].clone()).map_err(|_|"迁移记录无效")?;
    for conflict in preview["conflicts"].as_array().ok_or("冲突清单缺失")?{let key=conflict["key"].as_str().ok_or("冲突键无效")?;let chosen=choices.get(key).ok_or_else(||format!("冲突未解决：{key}"))?;if chosen=="unknown_runtime"{neutral_record(&records,key)?;}else if conflict["versions"].as_array().is_none_or(|a|!a.contains(chosen)){return Err("冲突选择无效".into());}}
    let mut selected:BTreeMap<(String,String),Record>=BTreeMap::new();
    for record in &records{
        if choices.get(&record_key(record)).is_some_and(|v|v!=&record.digest){continue;}
        if business(record).is_some_and(|(key,digest)|choices.get(&key).is_some_and(|v|v!=&digest)){continue;}
        let key=(record.table.clone(),record.id.clone());if selected.get(&key).is_none_or(|old|old.revision<record.revision){selected.insert(key,record.clone());}
    }
    for (key,choice) in choices{if choice=="unknown_runtime"{let merged=neutral_record(&records,key)?;selected.insert((merged.table.clone(),merged.id.clone()),merged);}}
    let mut target=Database::initialize(profile)?;
    let tx=target.store.db.transaction().map_err(|_|"迁移事务不可用")?;
    tx.execute_batch("create table migration_provenance(table_name text not null,record_id text not null,revision integer not null,digest text not null,payload text not null,columns_json text not null,sources text not null,selected integer not null);create table migration_manifest(payload text not null);").map_err(|_|"迁移来源表创建失败")?;
    for r in &records{let chosen=selected.get(&(r.table.clone(),r.id.clone())).is_some_and(|v|v.digest==r.digest);tx.execute("insert into migration_provenance values(?1,?2,?3,?4,?5,?6,?7,?8)",params![r.table,r.id,r.revision,r.digest,r.payload.to_string(),r.columns.to_string(),json!(r.sources).to_string(),chosen]).map_err(|_|"迁移来源保存失败")?;}
    let mut rows=selected.values().collect::<Vec<_>>();rows.sort_by(|a,b|a.columns["at"].as_f64().unwrap_or(0.).total_cmp(&b.columns["at"].as_f64().unwrap_or(0.)).then(a.revision.cmp(&b.revision)).then(a.id.cmp(&b.id)));
    for r in rows{let p=&r.payload;match r.table.as_str(){
        "events"=>tx.execute("insert into events values(?1,?2,?3,?4,?5)",params![r.id,r.columns["task_id"].as_str().ok_or("事件task_id缺失")?,r.columns["kind"].as_str().ok_or("事件kind缺失")?,r.columns["at"].as_f64().ok_or("事件时间缺失")?,p.to_string()]),
        "tasks"=>tx.execute("insert into tasks values(?1,?2)",params![r.id,p.to_string()]),
        "completed_tasks"=>tx.execute("insert into completed_tasks values(?1,?2,?3,?4)",params![r.id,r.columns["source_task_id"].as_str().ok_or("完成档案来源缺失")?,r.columns["revision"].as_i64().ok_or("完成档案修订缺失")?,p.to_string()]),
        "preferences"=>{let mut value=floatboard_protocol::preferences::validate_options(p)?;if p["appearance_revision"]!=2{value["theme"]=json!("dark");}value["settings_revision"]=json!(0);crate::storage::put(&tx,"preferences",&value)?;Ok(0)},
        "window"=>{crate::storage::put(&tx,"window",p)?;Ok(0)},
        "notifications"=>{crate::storage::put(&tx,"notifications",&crate::notifications::import_state(p)?)?;Ok(0)},
        "weixin_account"=>Ok(0),_=>return Err("迁移表无效".into())}.map_err(|e|format!("迁移记录提交失败：{e}"))?;}
    tx.execute("insert into migration_manifest values(?1)",[json!({"preview_sha256":hash(&bytes),"resolutions":resolutions,"sources":preview["sources"],"state_sources":preview["state_sources"]}).to_string()]).map_err(|_|"迁移清单保存失败")?;tx.commit().map_err(|_|"迁移提交失败")?;
    if let Some(account)=selected.get(&("weixin_account".into(),"current".into())){let bytes=base64::engine::general_purpose::STANDARD.decode(account.payload.as_str().ok_or("微信来源凭据无效")?).map_err(|_|"微信来源凭据编码无效")?;floatboard_client::files::atomic_bytes(&target.profile.root.join("weixin-production/account.dpapi"),&bytes)?;}
    crate::storage::put(&target.store.db,"write_gate",&json!("paused"))?;
    integrity(&target.store.db)?;let tasks=target.store.tasks()?;let counts=selected.values().fold(BTreeMap::<String,usize>::new(),|mut a,r|{*a.entry(r.table.clone()).or_default()+=1;a});
    let receipt=json!({"integrity":"ok","database_instance_id":target.database_id,"raw_counts":counts,"visible_tasks":tasks.len(),"retained_versions":records.len(),"notifications_generated":0,"activation_allowed":false,"resolutions_sha256":hash(canonical(resolutions).as_bytes()),"preview_sha256":hash(&bytes),"source_files":preview["sources"],"state_sources":preview["state_sources"],"coverage":coverage(&records)});atomic_json(&target.profile.root.join("migration-validation.json"),&receipt)?;Ok(receipt)
}

pub fn initialize(profile:Profile)->Result<Value>{let _owner=Instance::acquire(&profile.profile_id)?;let db=Database::initialize(profile)?;crate::storage::put(&db.store.db,"write_gate",&json!("open"))?;db.identity()}
pub fn check(profile:Profile)->Result<Value>{let _owner=Instance::acquire(&profile.profile_id)?;let mut db=Database::open(profile)?;integrity(&db.store.db)?;let mut result=db.identity()?;result["visible_tasks"]=json!(db.store.tasks()?.len());result["integrity"]=json!("ok");Ok(result)}
pub fn restore(source:&Path,profile:Profile)->Result<Value>{
    let _owner=Instance::acquire(&profile.profile_id)?;let target=profile.root.join("events.sqlite3");if target.exists()||profile.root.join("database-id.json").exists(){return Err("恢复仅允许新目标，禁止覆盖现有数据".into());}
    let receipt=read_json(&source.with_extension("json"))?;let before=fs::read(source).map_err(|_|"备份不可读")?;if receipt["sha256"]!=hash(&before){return Err("备份校验不通过".into());}
    fs::create_dir_all(&profile.root).map_err(|_|"恢复目标不可写")?;fs::write(&target,&before).map_err(|_|"恢复写入失败")?;
    let raw=Connection::open(&target).map_err(|_|"恢复副本不可读")?;integrity(&raw)?;
    let database_id=uuid::Uuid::new_v4().to_string();crate::storage::put(&raw,"restored_from_database",&receipt["database_instance_id"])?;crate::storage::put(&raw,"database_instance_id",&json!(database_id))?;crate::storage::put(&raw,"owner_profile_id",&json!(profile.profile_id))?;crate::storage::put(&raw,"write_gate",&json!("paused"))?;drop(raw);
    atomic_json(&profile.root.join("database-id.json"),&json!({"profile_id":profile.profile_id,"database_instance_id":database_id}))?;
    let db=Database::open(profile.clone())?;integrity(&db.store.db)?;let mut result=db.identity()?;result["integrity"]=json!("ok");Ok(result)
}

#[cfg(test)]mod tests{
use super::*;
#[test]fn settings_sources_preserve_preferences_window_credentials_and_require_conflict_choices(){
 let(root,profile)=fixture();drop(Database::initialize(profile.clone()).unwrap());
 let preferences=root.join("preferences.json");let other=root.join("other-preferences.json");let window=root.join("window.json");let notifications=root.join("notifications.json");let account=root.join("account.dpapi");
 atomic_json(&preferences,&json!({"theme":"light","appearance_revision":2,"notification_seconds":45,"auto_start":true})).unwrap();atomic_json(&other,&json!({"theme":"dark","appearance_revision":2})).unwrap();atomic_json(&window,&json!({"width":1200,"height":800,"x":12,"y":24,"pinned":false})).unwrap();
 let protected=base64::engine::general_purpose::STANDARD.encode(floatboard_client::files::crypt(b"https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=synthetic",true).unwrap());
 atomic_json(&notifications,&json!({"enabled":true,"channels":[{"id":"wecom","enabled":true,"webhook":protected}],"subscriptions":{"task_completed":true},"records":[{"id":"inflight","created_at":now(),"channel":"wecom","event":"test","title":"synthetic","status":"pending","attempts":1,"error":"","next_at":now(),"lease_until":now()+30.}]})).unwrap();
 let encrypted=floatboard_client::files::crypt(br#"{"generation":"synthetic-generation","token":"synthetic-account","bot_id":"synthetic-bot","login_user":"synthetic-user","base":"https://ilinkai.weixin.qq.com"}"#,true).unwrap();fs::write(&account,&encrypted).unwrap();
 let sources=[Source{label:"database".into(),role:"baseline".into(),database:profile.root.join("events.sqlite3"),main_only:false}];
 let states=[("prefs","preferences",preferences.clone()),("other","preferences",other),("window","window",window),("notifications","notifications",notifications),("weixin","weixin_account",account)].into_iter().map(|(label,kind,path)|StateSource{label:label.into(),kind:kind.into(),path}).collect::<Vec<_>>();
 let summary=preview(&sources,&states,&root.join("preview")).unwrap();assert_eq!(summary["conflicts"],1);
 let bytes=read_json(&root.join("preview/preview.json")).unwrap();let choice=bytes["records"].as_array().unwrap().iter().find(|r|r["table"]=="preferences"&&r["payload"]["theme"]=="light").unwrap()["digest"].clone();
 let mut target=profile.clone();target.root=root.join("target");target.profile_id=uuid::Uuid::new_v4().to_string();
 assert!(apply(&root.join("preview/preview.json"),&json!({"preview_sha256":summary["preview_sha256"],"choices":{}}),target.clone()).unwrap_err().contains("冲突未解决"));
 let receipt=apply(&root.join("preview/preview.json"),&json!({"preview_sha256":summary["preview_sha256"],"choices":{"preferences:current:0":choice}}),target.clone()).unwrap();assert_eq!(receipt["notifications_generated"],0);
 let db=Database::open(target.clone()).unwrap();let prefs=crate::storage::get(&db.store.db,"preferences").unwrap().unwrap();assert_eq!(prefs["theme"],"light");assert_eq!(prefs["notification_seconds"],45);assert_eq!(prefs["auto_start"],true);assert_eq!(crate::storage::get(&db.store.db,"window").unwrap().unwrap()["width"],1200);
 let state=crate::storage::get(&db.store.db,"notifications").unwrap().unwrap();assert_eq!(state["channels"][0]["webhook"],protected);assert_eq!(state["records"][0]["status"],"unknown");assert_eq!(state["baseline_reset"],true);assert_eq!(fs::read(target.root.join("weixin-production/account.dpapi")).unwrap(),encrypted);drop(db);
 fs::write(preferences,b"changed").unwrap();let mut next=profile;next.root=root.join("next");assert!(apply(&root.join("preview/preview.json"),&json!({"preview_sha256":summary["preview_sha256"],"choices":{"preferences:current:0":choice}}),next.clone()).unwrap_err().contains("设置来源已变化"));assert!(!next.root.exists());fs::remove_dir_all(root).unwrap();
}
fn fixture()->(PathBuf,Profile){let root=std::env::temp_dir().join(format!("floatboard-maintenance-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();let p=Profile{profile_id:uuid::Uuid::new_v4().to_string(),environment:"development".into(),owner_sid:floatboard_client::windows::sid().unwrap(),root:root.join("source"),port:60003,agent_exe:root.join("FloatboardAgent.exe"),gui_exe:root.join("CodexFloatboard.exe"),codex_home:root.join("codex")};(root,p)}
#[test]fn minimized_events_and_snapshots_migrate_with_exact_counts(){
 let(root,profile)=fixture();let db=Database::initialize(profile.clone()).unwrap();
 db.store.db.execute("insert into events values('minimal','host:session','SessionStart',10,'{}')",[]).unwrap();
 let task=json!({"id":"host:session","revision":2,"title":"preserved","status":"running","plan":[],"blockers":[],"evidence":[]});db.store.db.execute("insert into tasks values('host:session',?1)",[task.to_string()]).unwrap();drop(db);
 let source=Source{label:"history".into(),role:"baseline".into(),database:profile.root.join("events.sqlite3"),main_only:false};let before=hashes(&source).unwrap();let preview_dir=root.join("preview");let summary=preview(&[source.clone()],&[],&preview_dir).unwrap();
 let mut target=profile.clone();target.profile_id=uuid::Uuid::new_v4().to_string();target.root=root.join("target");let receipt=apply(&preview_dir.join("preview.json"),&json!({"preview_sha256":summary["preview_sha256"],"choices":{}}),target.clone()).unwrap();assert_eq!(receipt["raw_counts"]["events"],1);assert_eq!(receipt["raw_counts"]["tasks"],1);assert_eq!(receipt["notifications_generated"],0);assert_eq!(hashes(&source).unwrap(),before);
 let db=Database::open(target).unwrap();let raw:String=db.store.db.query_row("select payload from tasks",[],|r|r.get(0)).unwrap();assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(),task);drop(db);fs::remove_dir_all(root).unwrap();
}
#[test]fn source_changes_and_unresolved_conflicts_prevent_target_creation(){
 let(root,profile)=fixture();let db=Database::initialize(profile.clone()).unwrap();db.store.db.execute("insert into tasks values('t','{\"revision\":1,\"title\":\"a\"}')",[]).unwrap();drop(db);
 let second=root.join("second.sqlite3");fs::copy(profile.root.join("events.sqlite3"),&second).unwrap();let db=Connection::open(&second).unwrap();db.execute("update tasks set payload='{\"revision\":1,\"title\":\"b\"}'",[]).unwrap();drop(db);
 let sources=[Source{label:"a".into(),role:"baseline".into(),database:profile.root.join("events.sqlite3"),main_only:false},Source{label:"b".into(),role:"increment".into(),database:second.clone(),main_only:false}];let summary=preview(&sources,&[],&root.join("preview")).unwrap();assert_eq!(summary["conflicts"],1);
 let mut target=profile.clone();target.root=root.join("target");target.profile_id=uuid::Uuid::new_v4().to_string();let resolution=json!({"preview_sha256":summary["preview_sha256"],"choices":{}});assert!(apply(&root.join("preview/preview.json"),&resolution,target.clone()).unwrap_err().contains("冲突未解决"));assert!(!target.root.exists());fs::write(second,b"changed").unwrap();assert!(apply(&root.join("preview/preview.json"),&resolution,target).unwrap_err().contains("来源已变化"));fs::remove_dir_all(root).unwrap();
}
}
