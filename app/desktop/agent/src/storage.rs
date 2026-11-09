//! Single-threaded database owner. Receipts, projections and notification outbox commit together.
use std::{path::PathBuf,sync::{Arc,mpsc},time::Duration};
use serde_json::{json,Value};
use rusqlite::{Connection,OptionalExtension,params};
use floatboard_client::{Profile,files::{atomic_json,read_json}};
use floatboard_protocol::{Request,Result,now,hash,canonical,secret_eq,report::validate_report,preferences::{defaults,merge_options}};
use floatboard_storage::store::Store;
use crate::notifications::Notifications;
#[cfg(test)]mod tests;

pub fn get(db:&Connection,key:&str)->Result<Option<Value>>{db.query_row("select payload from runtime_kv where key=?1",[key],|r|r.get::<_,String>(0)).optional().map_err(|e|floatboard_storage::storage_error::sqlite("agent.state.read","Agent状态读取失败",e))?.map(|v|serde_json::from_str(&v).map_err(|_|"Agent状态格式无效".into())).transpose()}
pub fn put(db:&Connection,key:&str,value:&Value)->Result<()>{db.execute("insert into runtime_kv(key,payload) values(?1,?2) on conflict(key) do update set payload=excluded.payload",params![key,value.to_string()]).map_err(|e|floatboard_storage::storage_error::sqlite("agent.state.write","Agent状态提交失败",e))?;Ok(())}
fn seq(db:&Connection)->Result<u64>{Ok(get(db,"commit_sequence")?.and_then(|v|v.as_u64()).unwrap_or(0))}
pub(crate) fn advance(db:&Connection)->Result<u64>{let n=seq(db)?.checked_add(1).ok_or("提交序号已耗尽")?;put(db,"commit_sequence",&json!(n))?;Ok(n)}
fn decode_session(raw:&str)->Result<Value>{
    use base64::Engine;
    let mut value:Value=serde_json::from_str(raw).map_err(|_|"会话数据无效")?;
    let protected=base64::engine::general_purpose::STANDARD.decode(value["protected_token"].as_str().ok_or("会话凭据缺失")?).map_err(|_|"会话凭据格式无效")?;
    let token=String::from_utf8(floatboard_client::files::crypt(&protected,false)?).map_err(|_|"会话凭据无效")?;
    value.as_object_mut().ok_or("会话数据无效")?.remove("protected_token");value["session_token"]=json!(token);Ok(value)
}

pub struct Database{pub store:Store,pub profile:Profile,pub database_id:String}
impl Database {
    pub fn initialize(profile:Profile)->Result<Self>{
        if profile.root.join("events.sqlite3").exists()||profile.root.join("database-id.json").exists(){return Err("目标数据库已存在，拒绝覆盖".into());}
        Self::open_inner(profile,true)
    }
    pub fn open(profile:Profile)->Result<Self>{Self::open_inner(profile,false)}
    fn open_inner(profile:Profile,create:bool)->Result<Self>{
        floatboard_client::windows::check_path(&profile.root)?;
        let path=profile.root.join("events.sqlite3");let marker=profile.root.join("database-id.json");
        if (!create||marker.exists())&&!path.is_file(){return Err("已有数据库文件缺失，拒绝创建空库 [stage=agent.missing_database]".into());}
        if !path.exists()&&(profile.root.join("events.sqlite3-wal").exists()||profile.root.join("events.sqlite3-shm").exists()){return Err("存在孤立WAL/SHM，拒绝创建空库".into());}
        let _preflight_guard=if !create{Some(floatboard_storage::file_identity::FileGuard::open(&path,false)?)}else{None};
        if !create{
            let db=Connection::open_with_flags(&path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_|"已有数据库不可读 [stage=agent.open]")?;
            let version:u32=db.query_row("pragma user_version",[],|r|r.get(0)).map_err(|_|"数据库版本不可读 [stage=agent.version]")?;
            if version>2{return Err("数据库版本高于本Agent，禁止降级 [stage=agent.version]".into());}
            if version!=2{return Err("数据库尚未离线迁移，拒绝在线改写schema [stage=agent.version]".into());}
            let integrity:String=db.query_row("pragma quick_check(1)",[],|r|r.get(0)).map_err(|e|floatboard_storage::storage_error::sqlite("agent.integrity","数据库完整性不可读",e))?;
            let tables:i64=db.query_row("select count(*) from sqlite_master where type='table' and name in ('events','tasks','completed_tasks','runtime_kv','report_receipts','command_receipts','runtime_sessions')",[],|r|r.get(0)).map_err(|_|"数据库结构不可读")?;
            if integrity!="ok"||tables!=7{return Err("数据库完整性或结构损坏，拒绝补建空表 [stage=agent.integrity]".into());}
            let identity=read_json(&marker).map_err(|_|"数据库身份标记缺失或不可读 [stage=agent.identity]")?;
            if identity["profile_id"]!=profile.profile_id||get(&db,"owner_profile_id")?!=Some(json!(profile.profile_id))||get(&db,"database_instance_id")?!=Some(identity["database_instance_id"].clone())||identity["database_instance_id"].as_str().is_none(){return Err("数据库与profile标记不匹配 [stage=agent.identity]".into());}
        }
        let store=Store::open(&profile.root,false)?;
        store.db.execute_batch("pragma synchronous=FULL; pragma foreign_keys=ON; create table if not exists runtime_kv(key text primary key,payload text not null); create table if not exists report_receipts(task_id text not null,revision integer not null,digest text not null,result text not null,primary key(task_id,revision)); create table if not exists command_receipts(id text primary key,digest text not null,result text not null); create table if not exists runtime_sessions(id text primary key,payload text not null); pragma user_version=2;").map_err(|e|floatboard_storage::storage_error::sqlite("agent.schema","Agent初始化失败",e))?;
        if get(&store.db,"owner_profile_id")?.is_some_and(|id|id!=profile.profile_id){return Err("数据库归属其他profile，拒绝打开".into());}put(&store.db,"owner_profile_id",&json!(profile.profile_id))?;
        let database_id=match get(&store.db,"database_instance_id")?{Some(v)=>v.as_str().ok_or("数据库身份无效")?.to_owned(),None=>{let id=uuid::Uuid::new_v4().to_string();put(&store.db,"database_instance_id",&json!(id))?;id}};
        if marker.exists(){if read_json(&marker)?["database_instance_id"]!=database_id||read_json(&marker)?["profile_id"]!=profile.profile_id{return Err("数据库与profile标记不匹配 [stage=agent.identity]".into());}}
        else{atomic_json(&marker,&json!({"database_instance_id":database_id,"profile_id":profile.profile_id}))?;}
        if create{put(&store.db,"write_gate",&json!("paused"))?;}
        Ok(Self{store,profile,database_id})
    }
    pub fn identity(&self)->Result<Value>{Ok(json!({"database_instance_id":self.database_id,"schema_version":2,"commit_sequence":seq(&self.store.db)?,"storage":{"status":"ok","message":""},"write_gate":get(&self.store.db,"write_gate")?.unwrap_or(json!("open"))}))}
    pub(crate) fn runtime_gap(&mut self,cutoff:f64)->Result<()>{
        let tx=self.store.db.transaction().map_err(|_|"运行中断事务不可用")?;
        let rows={let mut statement=tx.prepare("select id,payload from tasks").map_err(|_|"运行记录不可读")?;let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|_|"运行记录不可读")?;rows.collect::<std::result::Result<Vec<_>,_>>().map_err(|_|"运行记录不可读")?};
        let mut changed=false;
        for(id,raw)in rows{let mut task:Value=serde_json::from_str(&raw).map_err(|_|"任务记录损坏")?;if ["completed","cancelled"].contains(&task["status"].as_str().unwrap_or("")){continue;}changed=true;
            let evidence:Option<f64>=tx.query_row("select max(at) from events where task_id=?1 and kind in ('progress','plan')",[&id],|r|r.get(0)).map_err(|_|"计时依据不可读")?;
            // A missing heartbeat ends timing at the last business evidence, never at an invented timestamp.
            if let Some(at)=if cutoff>0.{Some(cutoff.max(evidence.unwrap_or(cutoff)))}else{evidence}{
                tx.execute("insert into events values(?1,?2,'runtime_gap',?3,'{}')",params![uuid::Uuid::new_v4().to_string(),id,at]).map_err(|_|"运行中断记录失败")?;
            }
            task["runtime"]=json!("unknown");task["active_tools"]=json!({});task["agents"]=json!({});tx.execute("update tasks set payload=?1 where id=?2",params![task.to_string(),id]).map_err(|_|"运行状态保存失败")?;
        }if changed{advance(&tx)?;}tx.commit().map_err(|_|"运行中断事务提交失败")?;Ok(())
    }
    fn session(&self,id:&str,cwd:&str)->Result<Value>{
        if id.is_empty()||id.len()>160{return Err("会话标识无效".into());}
        let saved:Option<String>=self.store.db.query_row("select payload from runtime_sessions where id=?1",[id],|r|r.get(0)).optional().map_err(|_|"会话读取失败")?;
        if let Some(saved)=saved{return decode_session(&saved);}
        let token=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
        use base64::Engine;
        let protected=base64::engine::general_purpose::STANDARD.encode(floatboard_client::files::crypt(token.as_bytes(),true)?);
        let value=json!({"session_id":id,"protected_token":protected,"host_id":std::env::var("COMPUTERNAME").unwrap_or_else(|_|"localhost".into()),"cwd":floatboard_protocol::report::text(&json!(cwd),2000)?,"created_at":now()});
        self.store.db.execute("insert into runtime_sessions values(?1,?2)",params![id,value.to_string()]).map_err(|_|"会话保存失败")?;decode_session(&value.to_string())
    }
    fn authenticate(&self,params:&Value)->Result<Value>{
        let id=params["session_id"].as_str().ok_or("缺少会话身份")?;
        let saved:Option<String>=self.store.db.query_row("select payload from runtime_sessions where id=?1",[id],|r|r.get(0)).optional().map_err(|_|"会话读取失败")?;
        let record=decode_session(&saved.ok_or("会话未登记，请先启用Hook")?)?;
        if !secret_eq(params["session_token"].as_str().unwrap_or(""),record["session_token"].as_str().ok_or("会话身份无效")?){return Err("会话身份不匹配".into());}Ok(record)
    }
    fn hook(&mut self,params:&Value)->Result<Value>{
        let kind=params["hook_event_name"].as_str().ok_or("缺少Hook事件")?;
        if !floatboard_storage::store::EVENTS.contains(&kind){return Err("Hook事件无效".into());}
        let id=params["session_id"].as_str().ok_or("缺少会话身份")?;let record=self.session(id,params["cwd"].as_str().unwrap_or(""))?;
        let mut event=json!({"version":2,"event_id":uuid::Uuid::new_v4().to_string(),"recorded_at":now(),"session_id":id,"host_id":record["host_id"],"cwd":record["cwd"],"kind":kind});
        for key in ["turn_id","agent_id","tool_name","tool_use_id"]{if let Some(v)=params.get(key){event[key]=v.clone();}}
        self.store.ingest_with(&event,|db,_|{advance(db)?;put(db,"last_hook_at",&json!(now()))?;Ok(())})?;
        if kind=="PostToolUse"&&params["tool_name"]=="update_plan"{if let Some(items)=params["tool_input"]["plan"].as_array(){event["event_id"]=json!(uuid::Uuid::new_v4().to_string());event["kind"]=json!("plan");event["plan"]=json!(items.iter().enumerate().map(|(i,v)|json!({"id":format!("step-{i}"),"title":v["step"],"status":if v["status"]=="in_progress"{"inProgress"}else{v["status"].as_str().unwrap_or("pending")}})).collect::<Vec<_>>());self.store.ingest_with(&event,|db,_|{advance(db)?;put(db,"last_hook_at",&json!(now()))?;Ok(())})?;}}
        if !["SessionStart","UserPromptSubmit"].contains(&kind){return Ok(json!({}));}
        let revision=self.store.revision(id,record["host_id"].as_str().unwrap_or(""))?;
        let name=self.profile.server_name();let context=format!("本会话已接入 Codex 任务侧窗（连接 {name}，绑定信息只可用于该连接）。多步骤任务开始、阶段变化、遇阻及完成时调用 {name}.report_progress 汇报完整快照。仅清单完成、有验收证据且无开放卡点时标completed。汇报失败不阻断用户工作。绑定信息仅用于工具，不展示：session_id={id}; session_token={}; last_revision={revision}。下一次revision须大于last_revision。不要为汇报创建子Agent。",record["session_token"].as_str().unwrap_or(""));
        Ok(json!({"hookSpecificOutput":{"hookEventName":kind,"additionalContext":context}}))
    }
    fn report(&mut self,params:&Value)->Result<Value>{
        let record=self.authenticate(params)?;let report=validate_report(params)?;let revision=report["revision"].as_i64().ok_or("修订无效")?;
        let task_id=format!("{}:{}",record["host_id"].as_str().unwrap_or(""),record["session_id"].as_str().unwrap_or(""));let digest=hash(canonical(&report).as_bytes());
        let prior:Option<(String,String)>=self.store.db.query_row("select digest,result from report_receipts where task_id=?1 and revision=?2",params![task_id,revision],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|"回执读取失败")?;
        if let Some((old,result))=prior{if old!=digest{return Err(format!("同一revision内容冲突；current_revision={}",self.store.revision(record["session_id"].as_str().unwrap(),record["host_id"].as_str().unwrap())?));}return serde_json::from_str(&result).map_err(|_|"提交回执无效".into());}
        let current=self.store.revision(record["session_id"].as_str().unwrap(),record["host_id"].as_str().unwrap())?;
        if revision<=current{return Err(format!("修订已过期；current_revision={current}"));}
        let event_id=uuid::Uuid::new_v4().to_string();let event=json!({"version":2,"event_id":event_id,"recorded_at":now(),"kind":"progress","session_id":record["session_id"],"host_id":record["host_id"],"cwd":record["cwd"],"report":report});let mut receipt=Value::Null;
        self.store.ingest_with(&event,|db,_|{
            let n=advance(db)?;crate::notifications::observe_in(db)?;put(db,"last_report_at",&json!(now()))?;receipt=json!({"accepted":true,"event_id":event_id,"revision":revision,"commit_sequence":n});
            db.execute("insert into report_receipts values(?1,?2,?3,?4)",params![task_id,revision,digest,receipt.to_string()]).map_err(|_|"提交回执保存失败")?;Ok(())
        })?;Ok(receipt)
    }
    pub fn call(&mut self,request:&Request,notifications:&Notifications)->Result<Value>{
        if get(&self.store.db,"write_gate")?==Some(json!("paused"))&&!["identity","snapshot","window_load","data_location","request_result","backup","diagnostics","resume","quiesce","preview","connect","disconnect","connect_request"].contains(&request.method.as_str()){return Err("Agent正在维护，业务写入未提交，请稍后重试".into());}
        match request.method.as_str(){
            "quiesce" if request.params["confirmed"]==true=>{put(&self.store.db,"write_gate",&json!("paused"))?;self.identity()},
            "resume" if request.params["confirmed"]==true=>{put(&self.store.db,"write_gate",&json!("open"))?;self.identity()},
            "identity"=>self.identity(),"hook_context"=>self.hook(&request.params),"report_progress"=>self.report(&request.params),
            "snapshot"=>{let tasks=self.store.tasks()?;let options=get(&self.store.db,"preferences")?.unwrap_or_else(defaults);let mut value=self.identity()?;value["tasks"]=json!(tasks);value["notices"]=json!([]);value["options"]=options;value["healthy"]=json!(true);value["collector"]=json!({"status":"ok","version":env!("CARGO_PKG_VERSION")});value["connection"]=crate::integration::connection(&self.profile).unwrap_or_else(|e|json!({"state":"error","error":e}));if value["connection"]["connected"]==true{let hook=get(&self.store.db,"last_hook_at")?;let report=get(&self.store.db,"last_report_at")?;if report.is_some(){value["connection"]["state"]=json!("sync_verified");}else if hook.is_some(){value["connection"]["state"]=json!("session_received");}value["connection"]["session_received_at"]=json!(hook);value["connection"]["sync_verified_at"]=json!(report);}value["data_location"]=json!({"current":self.profile.root,"path":self.profile.root,"managed":true});value["now"]=json!(now());Ok(value)},
            "window_load"=>Ok(get(&self.store.db,"window")?.unwrap_or(json!({}))),
            "preferences"|"window_save"=>self.settings(request),
            "request_result"=>{let id=request.params["request_id"].as_str().ok_or("缺少请求ID")?;let result:Option<String>=self.store.db.query_row("select result from command_receipts where id=?1",[id],|r|r.get(0)).optional().map_err(|_|"请求回执不可读")?;Ok(json!({"committed":result.is_some(),"result":result.map(|s|serde_json::from_str::<Value>(&s)).transpose().map_err(|_|"请求回执无效")?}))},
            "data_location"=>Ok(json!({"current":self.profile.root,"path":self.profile.root,"managed":true})),
            "preview"|"connect"|"disconnect"|"connect_request"=>crate::integration::call(&self.profile,&request.method,&request.params),
            "backup"=>self.backup(if request.params["kind"]=="upgrade"{"upgrade"}else{"manual"},request.params["destination"].as_str().map(PathBuf::from)),
            "diagnostics"=>{let mut v=self.identity()?;v["version"]=json!(env!("CARGO_PKG_VERSION"));v["profile_id"]=json!(self.profile.profile_id);Ok(v)},
            "notification_snapshot"=>notifications.call(&self.store.db,"notification_snapshot",&request.params),
            method if method.starts_with("notification_")=>{let tx=self.store.db.transaction().map_err(|_|"通知事务不可用")?;notifications.call(&tx,method,&request.params)?;advance(&tx)?;tx.commit().map_err(|_|"通知事务提交失败")?;notifications.after_commit(&self.store.db,method,&request.params)},
            _=>Err("未知Agent操作".into())
        }
    }
    fn settings(&mut self,request:&Request)->Result<Value>{
        let digest=hash(format!("{}:{}",request.method,canonical(&request.params)).as_bytes());
        let old:Option<(String,String)>=self.store.db.query_row("select digest,result from command_receipts where id=?1",[&request.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|"设置回执不可读")?;
        if let Some((d,result))=old{if d!=digest{return Err("设置请求ID内容冲突".into());}return serde_json::from_str(&result).map_err(|_|"设置回执无效".into());}
        let key=if request.method=="preferences"{"preferences"}else{"window"};let previous=get(&self.store.db,key)?.unwrap_or_else(||if key=="preferences"{defaults()}else{json!({})});
        let mut desired=if key=="preferences"{if self.profile.environment!="production"&&request.params["auto_start"]==true{return Err("隔离DEV不能设置开机启动".into());}merge_options(&previous,&request.params)?}else{floatboard_protocol::preferences::validate_window(&request.params)?;let mut value=previous.clone();for(k,v)in request.params.as_object().unwrap(){value[k]=v.clone();}value};
        let revision=previous["settings_revision"].as_u64().unwrap_or(0);
        if key=="preferences"&&request.params["settings_revision"].as_u64()!=Some(revision){return Err(format!("设置已变化；settings_revision={revision}"));}
        desired["settings_revision"]=json!(revision+1);
        let tx=self.store.db.transaction().map_err(|_|"设置事务不可用")?;put(&tx,key,&desired)?;advance(&tx)?;tx.execute("insert into command_receipts values(?1,?2,?3)",params![request.request_id,digest,desired.to_string()]).map_err(|_|"设置回执保存失败")?;tx.commit().map_err(|_|"设置提交失败")?;Ok(desired)
    }
    pub fn backup(&self,kind:&str,destination:Option<PathBuf>)->Result<Value>{
        let dir=destination.unwrap_or_else(||self.profile.root.join("backups").join(kind));std::fs::create_dir_all(&dir).map_err(|_|"备份目录不可写")?;
        let path=dir.join(format!("{}-{}.sqlite3",now() as u64,uuid::Uuid::new_v4()));
        self.store.db.backup("main",&path,None::<fn(rusqlite::backup::Progress)>).map_err(|_|"一致性备份失败")?;
        let backup=Connection::open_with_flags(&path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_|"备份不可读")?;
        let integrity:String=backup.query_row("pragma integrity_check",[],|r|r.get(0)).map_err(|_|"备份完整性检查失败")?;if integrity!="ok"{return Err("备份完整性不通过，未登记成功".into());}
        let receipt=json!({"path":path,"kind":kind,"at":now(),"database_instance_id":self.database_id,"profile_id":self.profile.profile_id,"commit_sequence":seq(&self.store.db)?,"schema_version":2,"version":env!("CARGO_PKG_VERSION"),"sha256":hash(&std::fs::read(&path).map_err(|_|"备份校验失败")?),"integrity":"ok"});atomic_json(&path.with_extension("json"),&receipt)?;Ok(receipt)
    }
    fn daily_backup(&self)->Result<()>{
        let last=get(&self.store.db,"daily_backup")?.unwrap_or(json!({"day":-1,"sequence":0}));let day=(now()/86400.).floor() as i64;let sequence=seq(&self.store.db)?;
        if last["day"].as_i64()==Some(day)||last["sequence"].as_u64()==Some(sequence){return Ok(());}
        self.backup("daily",None)?;put(&self.store.db,"daily_backup",&json!({"day":day,"sequence":sequence}))?;
        let dir=self.profile.root.join("backups/daily");let mut entries=Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|_|"备份清单不可读")?{let receipt=entry.map_err(|_|"备份清单不可读")?.path();if receipt.extension().is_none_or(|s|s!="json"){continue;}let value=read_json(&receipt)?;let target=receipt.with_extension("sqlite3");if value["kind"]=="daily"&&value["database_instance_id"]==self.database_id&&value["path"].as_str().is_some_and(|p|std::path::Path::new(p)==target){entries.push((value["at"].as_f64().ok_or("备份时间无效")?,receipt,target));}}
        entries.sort_by(|a,b|a.0.total_cmp(&b.0));let count=entries.len().saturating_sub(14);for(_,receipt,target)in entries.into_iter().take(count){std::fs::remove_file(target).map_err(|_|"旧每日备份清理失败")?;std::fs::remove_file(receipt).map_err(|_|"旧备份回执清理失败")?;}Ok(())
    }
}

type Job=(Request,mpsc::Sender<Result<Value>>);
type Changes=Arc<(std::sync::Mutex<Value>,std::sync::Condvar)>;
fn publish(changes:&Changes,db:Option<&Database>,failure:&Option<String>){
    let value=match db.map(Database::identity){Some(Ok(mut v))=>{if failure.is_some(){v["storage"]["status"]=json!("error");}v},_=>json!({"storage":{"status":"error"}})};
    if let Ok(mut previous)=changes.0.lock(){if *previous!=value{*previous=value;changes.1.notify_all();}}
}
pub struct Actor{sender:mpsc::SyncSender<Job>,thread:Option<std::thread::JoinHandle<()>>,stop:Arc<std::sync::atomic::AtomicBool>,changes:Changes}
impl Actor {
    pub fn start(profile:Profile)->Result<Self>{
        let notifications=Notifications::new(&profile.root).map(Arc::new);Self::start_with(profile,notifications)
    }
    fn start_with(profile:Profile,notifications:Result<Arc<Notifications>>)->Result<Self>{
        let (sender,receiver)=mpsc::sync_channel::<Job>(128);let stop=Arc::new(std::sync::atomic::AtomicBool::new(false));let stopping=stop.clone();let (ready_tx,ready)=mpsc::channel();
        let changes:Changes=Arc::new((std::sync::Mutex::new(Value::Null),std::sync::Condvar::new()));let updates=changes.clone();
        let thread=std::thread::Builder::new().name("agent-storage".into()).spawn(move||{
            let opened=Database::open(profile.clone());let mut failure=opened.as_ref().err().cloned();let mut db=opened.ok();if let Some(db)=&mut db{match get(&db.store.db,"heartbeat"){Ok(value)=>{let last=value.and_then(|v|v.as_f64()).unwrap_or(0.);if let Err(e)=db.runtime_gap(last){failure=Some(e);}},Err(e)=>failure=Some(e)}}
            let notify_error=notifications.as_ref().err().cloned();let notifications=notifications.ok();
            publish(&updates,db.as_ref(),&failure);let _=ready_tx.send(());let (done_tx,done)=mpsc::channel::<(String,crate::notifications::Outcome)>();let mut sending=false;let mut network:Option<std::thread::JoinHandle<()>>=None;let mut next_tick=std::time::Instant::now();let mut backup_error:Option<String>=None;let mut last_observed=now();
            while !stopping.load(std::sync::atomic::Ordering::Acquire){
                match receiver.recv_timeout(Duration::from_millis(100)){
                    Ok((request,reply))=>{
                        let mut result=if ["identity","diagnostics"].contains(&request.method.as_str()){match db.as_ref(){Some(db)=>db.identity(),None=>Ok(json!({"storage":{"status":"error","message":"任务存储不可用，未创建替代数据库","diagnostic":failure.as_ref().map(|s|floatboard_storage::storage_error::diagnostic(s))}}))}}
                        else if failure.is_some(){Err("任务存储不可用；数据未清空，请导出诊断".into())}
                        else{match (db.as_mut(),notifications.as_ref()){(Some(db),Some(n))=>db.call(&request,n),_=>Err(notify_error.clone().unwrap_or_else(||"Agent未就绪".into()))}};
                        if let Ok(value)=&mut result{if ["identity","diagnostics"].contains(&request.method.as_str()){value["backup_error"]=json!(backup_error);value["notification_error"]=json!(notify_error);if let Some(error)=&failure{value["storage"]=json!({"status":"error","message":"存储故障，数据未清空","diagnostic":floatboard_storage::storage_error::diagnostic(error)});}}}
                        if result.as_ref().is_err_and(|e|e.contains("sqlite=")||e.contains("stage=file_identity.")||e.contains("损坏")||e.contains("事务提交失败")){failure=result.as_ref().err().cloned();}
                        publish(&updates,db.as_ref(),&failure);let _=reply.send(result);
                    },Err(mpsc::RecvTimeoutError::Disconnected)=>break,Err(mpsc::RecvTimeoutError::Timeout)=>{}
                }
                if let Ok((id,outcome))=done.try_recv(){if let Some(db)=&db{if let Err(e)=Notifications::finish(&db.store.db,&id,outcome){failure=Some(e);}}sending=false;if let Some(thread)=network.take(){let _:std::thread::Result<()>=thread.join();}}
                if next_tick.elapsed()>=Duration::from_secs(1){let observed=now();if observed-last_observed>5.||observed<last_observed{if let Some(db)=&mut db{if let Err(e)=db.runtime_gap(last_observed){failure=Some(e);}}}last_observed=observed;next_tick=std::time::Instant::now();if failure.is_none(){if let (Some(db),Some(n))=(db.as_ref(),notifications.as_ref()){
                    if !sending{match get(&db.store.db,"write_gate"){Ok(Some(value)) if value=="open"=>{match n.claim(&db.store.db){Ok(Some((channel,record)))=>{sending=true;let n=n.clone();let tx=done_tx.clone();network=Some(std::thread::spawn(move||{let result=n.dispatch(channel,record);let _=tx.send(result);}));},Ok(None)=>{},Err(e)=>failure=Some(e)}},Err(e)=>failure=Some(e),_=>{}}}
                    if let Err(e)=put(&db.store.db,"heartbeat",&json!(last_observed)){failure=Some(e);}
                    backup_error=db.daily_backup().err();
                }}}publish(&updates,db.as_ref(),&failure);
            }
            // Accepted requests drain before the database is dropped.
            while let Ok((request,reply))=receiver.try_recv(){let result=match (db.as_mut(),notifications.as_ref()){(Some(db),Some(n)) if failure.is_none()=>db.call(&request,n),_=>Err("Agent存储不可用".into())};let _=reply.send(result);}
            if let Some(thread)=network{let _=thread.join();if let (Ok((id,outcome)),Some(db))=(done.try_recv(),db.as_ref()){let _=Notifications::finish(&db.store.db,&id,outcome);}}
            if let Some(n)=notifications{n.shutdown();}
        }).map_err(|_|"存储执行器启动失败")?;
        ready.recv_timeout(Duration::from_secs(15)).map_err(|_|"存储执行器初始化超时")?;Ok(Self{sender,thread:Some(thread),stop,changes})
    }
    pub fn dispatcher(&self)->impl Fn(Request)->Result<Value>+Send+Sync+'static{let sender=self.sender.clone();let changes=self.changes.clone();let stopping=self.stop.clone();move|request|{
        if stopping.load(std::sync::atomic::Ordering::Acquire){return Err("Agent正在停止，请重试".into());}
        if request.method=="watch"{
            let previous=changes.0.lock().map_err(|_|"订阅状态不可用")?;
            let (value,_)=changes.1.wait_timeout_while(previous,Duration::from_millis(1500),|v|v["database_instance_id"]==request.params["database_instance_id"]&&v["commit_sequence"]==request.params["commit_sequence"]&&v["storage"]["status"]=="ok"&&!stopping.load(std::sync::atomic::Ordering::Acquire)).map_err(|_|"订阅已中断")?;
            if stopping.load(std::sync::atomic::Ordering::Acquire){return Err("Agent正在停止，请重连".into());}
            return Ok(value.clone());
        }
        let (tx,rx)=mpsc::channel();sender.try_send((request,tx)).map_err(|_|"Agent请求队列繁忙或已停止，请重试")?;rx.recv_timeout(Duration::from_secs(4)).map_err(|_|"Agent提交结果尚未确认，请按原身份重试".to_string())?
    }}
}
impl Drop for Actor{fn drop(&mut self){self.stop.store(true,std::sync::atomic::Ordering::Release);self.changes.1.notify_all();if let Some(thread)=self.thread.take(){let _=thread.join();}}}
