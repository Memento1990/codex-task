//! Durable, independently polled external notifications. No Engine lock is acquired here.
//! Disk contains only DPAPI CurrentUser ciphertext for channel credentials. Provider
//! response text and reqwest errors are deliberately never logged or returned.
use std::{collections::BTreeMap, fs::{self, File, OpenOptions}, path::{Path, PathBuf}, sync::{Arc, Mutex, RwLock, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}, time::Duration};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use crate::{Result, now};

#[cfg(test)]
mod tests;

const EXPIRY: f64 = 1800.;
const RETENTION: f64 = 7. * 86400.;
const LOCK_ERROR: &str = "通知数据正忙，请稍后重试";
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Channel { id:String, enabled:bool, webhook:String, keyword:String, signing_secret:String }
impl Default for Channel { fn default()->Self { Self { id:String::new(),enabled:false,webhook:String::new(),keyword:String::new(),signing_secret:String::new() } } }
#[derive(Clone, Serialize, Deserialize)]
struct Record { #[serde(default)] account_version:String, id:String, created_at:f64, channel:String, event:String, title:String, status:String, attempts:u32, error:String, next_at:f64, lease_until:f64 }
#[derive(Clone, Default, Serialize, Deserialize)]
struct Baseline { status:String, blockers:BTreeMap<String,bool>, revision:i64 }
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct State { enabled:bool, baseline_reset:bool, subscriptions:BTreeMap<String,bool>, channels:Vec<Channel>, records:Vec<Record>, baseline:BTreeMap<String,Baseline>, revision:u64 }
impl Default for State { fn default()->Self { Self { enabled:false,baseline_reset:true,subscriptions:BTreeMap::from([("blocker_opened".into(),true),("task_completed".into(),true),("blocker_resolved".into(),false)]),channels:["wechat","wecom","feishu"].into_iter().map(|id|Channel{id:id.into(),..Channel::default()}).collect(),records:vec![],baseline:BTreeMap::new(),revision:0 } } }
struct Inner { root:PathBuf, lifecycle:RwLock<()>, stopped:AtomicBool, baseline_needed:AtomicBool, error:Mutex<Option<String>>, weixin:Mutex<Option<Arc<crate::weixin::Weixin>>>, dispatch:Mutex<()> }
pub struct NotificationService { inner:Arc<Inner>, worker:Mutex<Option<JoinHandle<()>>> }
impl NotificationService {
    pub fn new(root:PathBuf)->Self {
        let inner=Arc::new(Inner{root,lifecycle:RwLock::new(()),stopped:AtomicBool::new(false),baseline_needed:AtomicBool::new(true),error:Mutex::new(None),weixin:Mutex::new(None),dispatch:Mutex::new(())});
        let worker_inner=Arc::clone(&inner);
        let handle=thread::Builder::new().name("notification-outbox".into()).spawn(move||worker(worker_inner)).ok();
        if handle.is_none(){if let Ok(mut error)=inner.error.lock(){*error=Some("通知后台线程无法启动".into());}}
        Self { inner,worker:Mutex::new(handle) }
    }
    /// Drain every foreground operation and worker cycle before migration. Requests
    /// are bounded, but draining can include multiple sequential network waits.
    /// Taking the account out releases its process lock even if stopped service
    /// Arc clones remain alive. Never join the worker while holding lifecycle.
    pub fn shutdown(&self) {
        self.inner.stopped.store(true,Ordering::Release);
        {
            let _lifecycle=self.inner.lifecycle.write().unwrap_or_else(|e|e.into_inner());
            let account=self.inner.weixin.lock().unwrap_or_else(|e|e.into_inner()).take();
            if let Some(account)=account {account.shutdown();}
        }
        if let Ok(mut handle)=self.worker.lock() { if let Some(handle)=handle.take() { let _=handle.join(); } }
    }
    pub fn observe(&self,tasks:&[Value])->Result<()> {
        ensure_running(&self.inner)?;
        let _lifecycle=self.inner.lifecycle.read().map_err(|_|"通知生命周期不可用")?;
        ensure_running(&self.inner)?;
        let _activity=crate::runtime::ProcessLock::acquire(&self.inner.root.join("notification-activity.lock"))?;
        ensure_running(&self.inner)?;
        check_root(&self.inner.root)?;
        observe(&self.inner,tasks)
    }
    pub fn call(&self,method:&str,params:Value)->Result<Value> {
        ensure_running(&self.inner)?;
        let _lifecycle=self.inner.lifecycle.read().map_err(|_|"通知生命周期不可用")?;
        ensure_running(&self.inner)?;
        check_root(&self.inner.root)?;
        if method=="notification_snapshot" {let _=weixin(&self.inner);return notification_snapshot(&self.inner);}
        // Cross-process configuration changes serialize with any outbox dispatch.
        let _activity=crate::runtime::ProcessLock::acquire(&self.inner.root.join("notification-activity.lock")).map_err(|_|"通知正在处理，请稍后重试")?;
        // Snapshot remains responsive while a bounded provider request is in flight.
        let _dispatch=self.inner.dispatch.lock().map_err(|_|"通知发送状态不可用")?;
        ensure_running(&self.inner)?;
        if method=="notification_weixin_action" {
            let action=params["action"].as_str().ok_or("缺少微信操作")?;
            let w=weixin(&self.inner)?;
            if action=="resume" {let allowed=transaction(&self.inner.root,|s|Ok(s.enabled&&s.channels.iter().any(|c|c.id=="wechat"&&c.enabled)))?;if !allowed{return Err("请先开启外部通知与微信渠道".into());}}
            if action=="disconnect" {transaction(&self.inner.root,|s|{invalidate_weixin(s);Ok(())})?;}
            w.call(action,&params)?;
            return notification_snapshot(&self.inner);
        }
        if method=="notification_channel_delete"&&params["id"]=="wechat" {transaction(&self.inner.root,|s|{invalidate_weixin(s);Ok(())})?;weixin(&self.inner)?.call("disconnect",&json!({}))?;}
        let wx=weixin_snapshot(&self.inner);
        transaction(&self.inner.root,|state| {

            match method {
                "notification_snapshot"=>{},
                "notification_save"=>{
                    let enabled=params["enabled"].as_bool().ok_or("通知开关格式无效")?;
                    if state.enabled!=enabled { state.baseline_reset=true;self.inner.baseline_needed.store(true,Ordering::Release); }
                    state.enabled=enabled;
                    if let Some(subs)=params.get("subscriptions") {
                        for event in ["blocker_opened","task_completed","blocker_resolved"] { if let Some(value)=subs.get(event) { state.subscriptions.insert(event.into(),value.as_bool().ok_or("订阅格式无效")?); } }
                    }
                    if !enabled { for r in &mut state.records { if r.status=="pending" { r.status="skipped".into();r.error="外部通知已关闭".into(); } } }
                },
                "notification_channel_save"=>{
                    let id=channel_id(&params)?;
                    let activating=params["enabled"]==true&&!state.channels.iter().any(|c|c.id==id&&c.enabled);
                    if id=="wechat" {
                        let enabled=params["enabled"].as_bool().ok_or("渠道开关格式无效")?;
                        if enabled&&wx["bound"]!=true{return Err("请先扫码并绑定微信收件人".into());}
                        state.channels.iter_mut().find(|c|c.id==id).unwrap().enabled=enabled;
                        if !enabled {invalidate_weixin_records(state,"微信渠道已关闭");}
                    }else{
                    let c=state.channels.iter_mut().find(|c|c.id==id).ok_or("渠道不存在")?;
                    c.enabled=params["enabled"].as_bool().ok_or("渠道开关格式无效")?;
                    c.keyword=bounded(params.get("keyword").and_then(Value::as_str).unwrap_or(""),100);
                    if let Some(url)=params.get("webhook").and_then(Value::as_str).filter(|s|!s.trim().is_empty()) { validate_url(id,url.trim())?;c.webhook=protect(url.trim())?; }
                    if params["clear_signing_secret"]==true { c.signing_secret.clear(); }
                    else if let Some(secret)=params.get("signing_secret").and_then(Value::as_str).filter(|s|!s.is_empty()) { if secret.len()>4096{return Err("签名密钥过长".into());} if id!="feishu" {return Err("此渠道不支持签名密钥".into());} c.signing_secret=protect(secret)?; }
                    if c.enabled&&c.webhook.is_empty(){return Err("请先配置 Webhook".into());}
                    }
                    // Shared baseline: activation suppresses the next observation for
                    // ALL channels. This conservative compromise prevents historical
                    // events between polls leaking into a newly enabled channel.
                    if activating {state.baseline_reset=true;}
                },
                "notification_channel_delete"=>{
                    let id=channel_id(&params)?;
                    let c=state.channels.iter_mut().find(|c|c.id==id).ok_or("渠道不存在")?;
                    *c=Channel{id:id.into(),..Channel::default()};
                    for r in &mut state.records { if r.channel==id&&r.status=="pending" {r.status="skipped".into();r.error="渠道已删除".into();} }
                },
                "notification_test"=>{
                    let id=channel_id(&params)?;
                    let c=state.channels.iter().find(|c|c.id==id).ok_or("渠道不存在")?;
                    if !state.enabled||!c.enabled{return Err("请先开启外部通知与渠道".into());}
                    if c.id=="wechat" {if wx["bound"]!=true{return Err("请先绑定微信收件人".into());}}else if c.webhook.is_empty(){return Err("请先配置 Webhook".into());}
                    enqueue(state,id,"test","任务侧窗 · 测试通知",None);
                },
                "notification_retry"=>{
                    let id=params["id"].as_str().ok_or("缺少记录标识")?;
                    let old=state.records.iter().find(|r|r.id==id).cloned().ok_or("通知记录不存在")?;
                    if old.status=="pending" {return Err("通知正在队列中".into());}
                    let c=state.channels.iter().find(|c|c.id==old.channel).ok_or("渠道不存在")?;
                    if c.id=="wechat" {if old.status=="unknown"{return Err("微信发送结果未知，可能已送达；为避免重复，请先在手机确认，不支持重试此条".into());}if wx["bound"]!=true{return Err("请先绑定微信收件人".into());}}else if c.webhook.is_empty(){return Err("渠道尚未配置".into());}
                    if !state.enabled||!c.enabled{return Err("请先开启外部通知与渠道".into());}
                    // A manual retry is a new outbox entry; preserve the original audit trail.
                    enqueue(state,&old.channel,&old.event,&old.title,None);
                },
                "notification_clear"=>{state.records.retain(|r|r.status=="pending");},
                _=>return Err("未知通知方法".into())
            }
            prune(state,now());
            let mut result=snapshot(state);
            if let Ok(error)=self.inner.error.lock(){result["service_error"]=json!(*error);}
            Ok(result)
        })?;
        if (method=="notification_save"&&params["enabled"]==false)||(method=="notification_channel_save"&&params["id"]=="wechat"&&params["enabled"]==false){if let Some(w)=self.inner.weixin.lock().map_err(|_|"微信状态不可用")?.as_ref(){w.halt()?;}}
        sync_weixin(&self.inner)?;
        notification_snapshot(&self.inner)
    }
}
impl Drop for NotificationService { fn drop(&mut self){self.shutdown();} }
fn channel_id(params:&Value)->Result<&str>{params["id"].as_str().filter(|s|["wechat","wecom","feishu"].contains(s)).ok_or_else(||"渠道标识无效".into())}
fn bounded(s:&str,n:usize)->String { s.chars().filter(|c|!c.is_control()||*c=='\n').take(n).collect() }
fn snapshot(s:&State)->Value {
    json!({"enabled":s.enabled,"subscriptions":s.subscriptions,"channels":s.channels.iter().map(|c|json!({"id":c.id,"name":match c.id.as_str(){"wechat"=>"微信","wecom"=>"企业微信",_=>"飞书"},"enabled":c.enabled,"configured":!c.webhook.is_empty(),"status":if c.id=="wechat"{"unavailable"}else if c.webhook.is_empty(){"unconfigured"}else if !c.enabled{"disabled"}else{"ready"},"keyword":c.keyword,"has_signing_secret":!c.signing_secret.is_empty()})).collect::<Vec<_>>(),"records":s.records.iter().rev().map(|r|json!({"id":r.id,"created_at":r.created_at,"channel":r.channel,"event":r.event,"title":r.title,"status":r.status,"attempts":r.attempts,"error":r.error})).collect::<Vec<_>>(),"revision":s.revision})
}
fn transaction<T>(root:&Path,op:impl FnOnce(&mut State)->Result<T>)->Result<T> {
    fs::create_dir_all(root).map_err(|_|"通知目录不可写")?;
    let _lock=lock(root)?;
    let path=root.join("notifications.json");
    let mut state:State=if path.exists(){serde_json::from_slice(&fs::read(&path).map_err(|_|"通知数据不可读")?).map_err(|_|"通知数据损坏，已停止发送")?}else{State::default()};
    let result=op(&mut state)?;state.revision=state.revision.saturating_add(1);
    crate::store::atomic_json(&path,&serde_json::to_value(&state).map_err(|_|"通知数据编码失败")?)?;
    Ok(result)
}
fn lock(root:&Path)->Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    for _ in 0..10 {
        match OpenOptions::new().create(true).truncate(false).read(true).write(true).share_mode(0).open(root.join("notifications.lock")) {
            Ok(f)=>return Ok(f),Err(e) if e.raw_os_error()==Some(32)=>thread::sleep(Duration::from_millis(10)),Err(_)=>return Err("通知锁不可用".into())
        }
    } Err(LOCK_ERROR.into())
}
fn enqueue(s:&mut State,channel:&str,event:&str,title:&str,id:Option<String>) {
    let id=id.unwrap_or_else(||uuid::Uuid::new_v4().to_string());
    if s.records.iter().any(|r|r.id==id){return;}
    s.records.push(Record{account_version:String::new(),id,created_at:now(),channel:channel.into(),event:event.into(),title:bounded(title,400),status:"pending".into(),attempts:0,error:String::new(),next_at:0.,lease_until:0.});
}
fn prune(s:&mut State,at:f64) {
    for r in &mut s.records {if r.status=="pending"&&at-r.created_at>=EXPIRY&&r.lease_until<=at {r.status="expired".into();r.error="通知已超过30分钟有效期".into();}}
    s.records.retain(|r|at-r.created_at<RETENTION);
    if s.records.len()>500 {s.records.drain(..s.records.len()-500);}
}
fn observe(inner:&Inner,tasks:&[Value])->Result<()> {
    if inner.stopped.load(Ordering::Acquire){return Ok(());}
    transaction(&inner.root,|state|{
        let baseline=inner.baseline_needed.load(Ordering::Acquire)||state.baseline_reset;
        let mut current=BTreeMap::new();
        for task in tasks.iter().filter(|t|t["archived"]!=true&&t["status"]!="unreported") {
            let Some(id)=task["id"].as_str().filter(|s|!s.is_empty())else{continue};
            let mut next=Baseline {status:task["status"].as_str().unwrap_or("").into(),revision:task["revision"].as_i64().unwrap_or(0),blockers:BTreeMap::new()};
            if let Some(blockers)=task["blockers"].as_array(){for b in blockers {if let(Some(id),Some(open))=(b["id"].as_str(),b["open"].as_bool()){next.blockers.insert(id.into(),open);}}}
            if !baseline&&state.enabled {
                let old=state.baseline.get(id).cloned();
                let mut events=Vec::new();
                if next.status=="completed"&&old.as_ref().is_none_or(|o|o.status!="completed") {events.push(("task_completed","task".to_string(),"任务已完成".to_string()));}
                for (bid,open) in &next.blockers {
                    let previous=old.as_ref().and_then(|o|o.blockers.get(bid));
                    let event=if *open&&previous!=Some(&true){Some("blocker_opened")}else if !*open&&previous==Some(&true){Some("blocker_resolved")}else{None};
                    if let Some(event)=event {let label=task["blockers"].as_array().and_then(|bs|bs.iter().find(|b|b["id"]==bid.as_str())).and_then(|b|b["title"].as_str()).unwrap_or("卡点");events.push((event,bid.clone(),format!("{} · {label}",if *open{"卡点待处理"}else{"卡点已解除"})));}
                }
                let channels:Vec<String>=state.channels.iter().filter(|c|c.enabled&&(c.id=="wechat"||!c.webhook.is_empty())).map(|c|c.id.clone()).collect();
                for (event,stable,label) in events {if state.subscriptions.get(event)!=Some(&true){continue;}
                    let title=format!("{label} · {}",task["title"].as_str().unwrap_or("任务"));
                    for channel in &channels {let key=crate::store::hash(format!("{id}\0{event}\0{stable}\0{}\0{channel}",next.revision).as_bytes());enqueue(state,channel,event,&title,Some(key));}
                }
            }
            current.insert(id.into(),next);
        }
        state.baseline=current;state.baseline_reset=false;prune(state,now());Ok(())
    })?;
    inner.baseline_needed.store(false,Ordering::Release);Ok(())
}
fn check_root(root:&Path)->Result<()> {
    let resolved=crate::migration::resolve_root(root)?;
    let normalize=|p:&Path|p.to_string_lossy().trim_start_matches(r"\\?\").trim_end_matches(['\\','/']).to_lowercase();
    if normalize(&resolved)!=normalize(root){return Err("数据目录已迁移，请重新初始化通知服务".into());}Ok(())
}
fn ensure_running(inner:&Inner)->Result<()> {if inner.stopped.load(Ordering::Acquire){Err("通知服务已停止".into())}else{Ok(())}}

// Observation is optional: corrupt task storage must not disable explicit tests,
// durable outbox delivery, or restoration of an independently stored account.
// Caller owns lifecycle then activity; dispatch always follows those locks.
fn worker_cycle(inner:&Inner,transport:impl FnOnce(&Channel,&Record)->Outcome)->Result<()> {
    ensure_running(inner)?;
    let sync_result={let _gate=inner.dispatch.lock().map_err(|_|"通知发送状态不可用")?;ensure_running(inner)?;sync_weixin(inner)};
    let observation=(||{let mut store=crate::store::Store::open(&inner.root,true)?;let tasks=store.tasks()?;observe(inner,&tasks)})();
    ensure_running(inner)?;
    let delivery=deliver_one(inner,transport);
    sync_result.and(observation).and(delivery)
}
fn worker(inner:Arc<Inner>) {
    let client=reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(8)).connect_timeout(Duration::from_secs(4)).build().ok();
    if client.is_none(){if let Ok(mut error)=inner.error.lock(){*error=Some("通知网络客户端初始化失败".into());}return;}
    while !inner.stopped.load(Ordering::Acquire) {
        {
            let Ok(_lifecycle)=inner.lifecycle.read()else{break};
            if inner.stopped.load(Ordering::Acquire){break;}
            if let Ok(_activity)=crate::runtime::ProcessLock::acquire(&inner.root.join("notification-activity.lock")) {
                if inner.stopped.load(Ordering::Acquire)||check_root(&inner.root).is_err(){break;}
                let result=worker_cycle(&inner,|channel,record|send(client.as_ref().unwrap(),channel,record));
                if let Ok(mut error)=inner.error.lock(){*error=result.err();}
            }
        }
        for _ in 0..10 {if inner.stopped.load(Ordering::Acquire){break;}thread::sleep(Duration::from_millis(100));}
    }
}
#[derive(Debug,PartialEq)]
enum Outcome { Unknown, Accepted, Temporary(&'static str), Permanent(&'static str) }
fn deliver_one(inner:&Inner,transport:impl FnOnce(&Channel,&Record)->Outcome)->Result<()> {
    let Ok(_dispatch)=inner.dispatch.try_lock()else{return Ok(())};
    ensure_running(inner)?;
    let wx=weixin_snapshot(inner);
    let work=transaction(&inner.root,|state|{
        let at=now();prune(state,at);
        let Some(index)=state.records.iter().position(|r|r.status=="pending"&&r.next_at<=at&&r.lease_until<=at)else{return Ok(None)};
        let channel=state.channels.iter().find(|c|c.id==state.records[index].channel).cloned();
        let record=&mut state.records[index];
        // Only the process owning the account may dispatch Weixin. A second
        // instance must not discard its owner's durable queue.
        if channel.as_ref().is_some_and(|c|c.id=="wechat")&&wx["bound"]!=true{return Ok(None);}
        let Some(channel)=channel.filter(|c|(c.id=="wechat"&&wx["bound"]==true||c.id!="wechat"&&!c.webhook.is_empty())&&state.enabled&&c.enabled)else{record.status="skipped".into();record.error="通知已关闭或渠道不可用".into();return Ok(None)};
        if channel.id=="wechat" {
            if record.lease_until>0.&&record.attempts>0 {record.status="unknown".into();record.error="上次发送中断，可能已送达；不会自动重发".into();return Ok(None);}
            record.account_version=wx["account_version"].as_str().unwrap_or("").into();
        }
        // Persist claim BEFORE external side effect. An OS sharing lock serializes
        // every claim across processes; lease allows recovery after process crash.
        if record.attempts>=3 {record.status="failed".into();record.error="自动发送次数已达上限".into();return Ok(None);}
        record.attempts+=1;record.lease_until=at+30.;Ok(Some((channel,record.clone())))
    })?;
    let Some((channel,record))=work else{return Ok(())};
    if inner.stopped.load(Ordering::Acquire){return Ok(());}
    let outcome=if channel.id=="wechat" {
        match weixin(inner)?.send(&record.account_version,&record.id,&record.title) {
            crate::weixin::SendOutcome::Accepted=>Outcome::Accepted,
            crate::weixin::SendOutcome::Unknown=>Outcome::Unknown,
            crate::weixin::SendOutcome::Failed=>Outcome::Permanent("微信未受理或会话不可用，请重新连接")
        }
    }else{transport(&channel,&record)};
    transaction(&inner.root,|state|{
        if let Some(r)=state.records.iter_mut().find(|r|r.id==record.id&&r.status=="pending"&&r.attempts==record.attempts) {
            r.lease_until=0.;
            match outcome {Outcome::Unknown=>{r.status="unknown".into();r.error="发送结果未知，可能已送达；不会自动重发".into();},Outcome::Accepted=>{r.status="accepted".into();r.error.clear();},Outcome::Permanent(error)=>{r.status="failed".into();r.error=error.into();},Outcome::Temporary(error)=>{r.error=error.into();if r.attempts>=3{r.status="failed".into();}else{r.next_at=now()+if r.attempts==1{5.}else{30.};}}}
        }Ok(())
    })
}
fn validate_url(id:&str,input:&str)->Result<reqwest::Url> {
    let invalid=||"Webhook 地址无效；仅允许官方 HTTPS 机器人地址".to_string();
    let url=reqwest::Url::parse(input).map_err(|_|invalid())?;
    // Explicit :443 is acceptable; all non-default ports, userinfo, fragments,
    // encoded/ambiguous paths and extra query parameters are rejected.
    if url.scheme()!="https"||!url.username().is_empty()||url.password().is_some()||url.port().is_some()||url.fragment().is_some()||input.contains('@')||input.contains('\\')||input.contains('%')||input.contains("/../")||input.contains("/./")||input.chars().any(char::is_control) {return Err(invalid());}
    match id {
        "wecom"=>{let pairs:Vec<_>=url.query_pairs().collect();if url.host_str()!=Some("qyapi.weixin.qq.com")||url.path()!="/cgi-bin/webhook/send"||pairs.len()!=1||pairs[0].0!="key"||pairs[0].1.is_empty()||!pairs[0].1.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_'){return Err(invalid());}},
        "feishu"=>{let token=url.path().strip_prefix("/open-apis/bot/v2/hook/").unwrap_or("");if url.host_str()!=Some("open.feishu.cn")||url.query().is_some()||token.is_empty()||!token.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_'){return Err(invalid());}},
        _=>return Err("微信未接入协议".into())
    }
    Ok(url)
}
fn send(client:&reqwest::blocking::Client,c:&Channel,r:&Record)->Outcome {
    let Ok(webhook)=unprotect(&c.webhook)else{return Outcome::Permanent("本地凭据无法解密，请重新配置")};
    let Ok(url)=validate_url(&c.id,&webhook)else{return Outcome::Permanent("Webhook 地址无效，请重新配置")};
    let payload=match payload(c,r,&(now() as u64).to_string()){Ok(value)=>value,Err(_)=>return Outcome::Permanent("本地签名密钥无法解密，请重新配置")};
    let response=match client.post(url).header(reqwest::header::CONTENT_TYPE,"application/json").body(payload.to_string()).send(){Ok(r)=>r,Err(e)=>return if e.is_timeout()||e.is_connect(){Outcome::Temporary("网络暂不可用或请求超时")}else{Outcome::Permanent("发送请求失败")}};
    let status=response.status().as_u16();
    if status==429||status>=500{return classify_response(&c.id,status,b"");}
    if !(200..300).contains(&status){return classify_response(&c.id,status,b"");}
    // Bound provider response; never echo it, as it may contain request secrets.
    use std::io::Read;
    let mut bytes=Vec::new();if response.take(65537).read_to_end(&mut bytes).is_err()||bytes.len()>65536{return Outcome::Permanent("平台响应无效");}
    classify_response(&c.id,status,&bytes)
}
fn payload(c:&Channel,r:&Record,timestamp:&str)->Result<Value> {
    let text=if c.keyword.is_empty(){r.title.clone()}else{format!("{}\n{}",c.keyword,r.title)};
    let mut payload=if c.id=="wecom"{json!({"msgtype":"text","text":{"content":text}})}else{json!({"msg_type":"text","content":{"text":text}})};
    if c.id=="feishu"&&!c.signing_secret.is_empty(){let secret=unprotect(&c.signing_secret)?;payload["sign"]=json!(feishu_signature(timestamp,&secret));payload["timestamp"]=json!(timestamp);}
    Ok(payload)
}
fn classify_response(channel:&str,status:u16,bytes:&[u8])->Outcome {
    if status==429||status>=500{return Outcome::Temporary("平台繁忙，请稍后重试");}
    if !(200..300).contains(&status){return Outcome::Permanent("平台拒绝请求，请检查机器人配置");}
    let Ok(value)=serde_json::from_slice::<Value>(bytes)else{return Outcome::Permanent("平台响应无效")};
    let code=if channel=="wecom"{value["errcode"].as_i64()}else{value.get("code").or_else(||value.get("StatusCode")).and_then(Value::as_i64)};
    match code {Some(0)=>Outcome::Accepted,Some(45009|45011|99991400|99991401)=>Outcome::Temporary("平台频率限制"),_=>Outcome::Permanent("平台未受理，请检查机器人配置与签名")}
}
fn feishu_signature(timestamp:&str,secret:&str)->String {
    // Feishu signs an empty message with timestamp + newline + secret as HMAC key.
    let key=format!("{timestamp}\n{secret}");let bytes=key.as_bytes();let mut block=[0u8;64];
    if bytes.len()>64 {block[..32].copy_from_slice(&Sha256::digest(bytes));}else{block[..bytes.len()].copy_from_slice(bytes);}
    let mut inner=Sha256::new();inner.update(block.map(|b|b^0x36));let digest=inner.finalize();
    let mut outer=Sha256::new();outer.update(block.map(|b|b^0x5c));outer.update(digest);STANDARD.encode(outer.finalize())
}
fn protect(value:&str)->Result<String>{crypt(value.as_bytes(),true).map(|v|STANDARD.encode(v))}
fn unprotect(value:&str)->Result<String>{let bytes=STANDARD.decode(value).map_err(|_|"本地凭据格式无效")?;String::from_utf8(crypt(&bytes,false)?).map_err(|_|"本地凭据格式无效".into())}
fn crypt(bytes:&[u8],encrypt:bool)->Result<Vec<u8>> {
    use windows_sys::Win32::{Security::Cryptography::{CRYPT_INTEGER_BLOB,CryptProtectData,CryptUnprotectData,CRYPTPROTECT_UI_FORBIDDEN},Foundation::LocalFree};
    let input=CRYPT_INTEGER_BLOB{cbData:bytes.len().try_into().map_err(|_|"凭据过长")?,pbData:bytes.as_ptr() as *mut u8};let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()};
    let ok=unsafe{if encrypt{CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}else{CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}};
    if ok==0{return Err("Windows 当前用户凭据保护失败".into());}
    let result=unsafe{std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec()};unsafe{LocalFree(output.pbData as *mut _);};Ok(result)
}

fn invalidate_weixin_records(s:&mut State,reason:&str){for r in &mut s.records{if r.channel=="wechat"&&r.status=="pending"{r.status=if r.lease_until>0.{"unknown"}else{"skipped"}.into();r.error=reason.into();}}}
fn invalidate_weixin(s:&mut State){if let Some(c)=s.channels.iter_mut().find(|c|c.id=="wechat"){c.enabled=false;}invalidate_weixin_records(s,"微信账号已解除，旧队列不再发送");}
fn weixin(inner:&Inner)->Result<Arc<crate::weixin::Weixin>>{let mut slot=inner.weixin.lock().map_err(|_|"微信状态不可用")?;ensure_running(inner)?;if slot.is_none(){*slot=Some(Arc::new(crate::weixin::Weixin::new(inner.root.join("weixin-production"))?));}Ok(slot.as_ref().unwrap().clone())}
fn weixin_snapshot(inner:&Inner)->Value{inner.weixin.lock().ok().and_then(|s|s.as_ref().and_then(|w|w.call("snapshot",&json!({})).ok())).unwrap_or_else(||json!({"status":"disconnected","busy":false,"active":false,"logged_in":false,"bound":false,"restored":false,"error":""}))}
fn notification_snapshot(inner:&Inner)->Result<Value>{let wx=weixin_snapshot(inner);transaction(&inner.root,|s|{let mut result=snapshot(s);result["weixin"]=wx.clone();if let Some(c)=result["channels"].as_array_mut().and_then(|cs|cs.iter_mut().find(|c|c["id"]=="wechat")){c["configured"]=wx["bound"].clone();c["status"]=json!(if wx["bound"]!=true{"unconfigured"}else if c["enabled"]!=true{"disabled"}else{"ready"});}if let Ok(e)=inner.error.lock(){result["service_error"]=json!(*e);}Ok(result)})}
fn sync_weixin(inner:&Inner)->Result<()>{
 let enabled=transaction(&inner.root,|s|Ok(s.enabled&&s.channels.iter().any(|c|c.id=="wechat"&&c.enabled)))?;
 if enabled {let w=weixin(inner)?;let s=w.call("snapshot",&json!({}))?;if s["bound"]==true&&(s["status"]=="restored"||s["status"]=="bound")&&s["active"]!=true&&s["error"].as_str().unwrap_or("").is_empty(){w.call("resume",&json!({}))?;}}
 else if let Some(w)=inner.weixin.lock().map_err(|_|"微信状态不可用")?.as_ref(){let s=w.call("snapshot",&json!({}))?;if s["bound"]==true&&s["active"]==true{w.halt()?;}}
 Ok(())
}
