//! Notification state lives in the Agent transaction, network effects never do.
use std::{collections::BTreeMap,sync::Arc,time::Duration};
use serde::{Serialize,Deserialize};
use serde_json::{json,Value};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use sha2::{Digest,Sha256};
use rusqlite::Connection;
use floatboard_protocol::{Result,now};
use floatboard_client::files::crypt;
const EXPIRY:f64=1800.;
const RETENTION:f64=7.*86400.;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Channel { id:String, enabled:bool, webhook:String, keyword:String, signing_secret:String }
impl Default for Channel { fn default()->Self { Self { id:String::new(),enabled:false,webhook:String::new(),keyword:String::new(),signing_secret:String::new() } } }
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Record { #[serde(default)] account_version:String, id:String, created_at:f64, channel:String, event:String, title:String, status:String, attempts:u32, error:String, next_at:f64, lease_until:f64 }
#[derive(Clone, Default, Serialize, Deserialize)]
struct Baseline { status:String, blockers:BTreeMap<String,bool>, revision:i64 }
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct State { enabled:bool, baseline_reset:bool, subscriptions:BTreeMap<String,bool>, channels:Vec<Channel>, records:Vec<Record>, baseline:BTreeMap<String,Baseline>, revision:u64 }
impl Default for State { fn default()->Self { Self { enabled:false,baseline_reset:true,subscriptions:BTreeMap::from([("blocker_opened".into(),true),("task_completed".into(),true),("blocker_resolved".into(),false)]),channels:["wechat","wecom","feishu"].into_iter().map(|id|Channel{id:id.into(),..Channel::default()}).collect(),records:vec![],baseline:BTreeMap::new(),revision:0 } } }

/// Offline import keeps provider ciphertext and send outcomes, never replays history.
pub(crate) fn import_state(value:&Value)->Result<Value>{
    let mut state:State=serde_json::from_value(value.clone()).map_err(|_|"通知来源格式无效")?;
    for channel in &state.channels{for protected in [&channel.webhook,&channel.signing_secret]{if !protected.is_empty(){unprotect(protected)?;}}}
    for record in &mut state.records{if record.status=="pending"&&record.lease_until>0.{record.status="unknown".into();record.lease_until=0.;record.error="迁移前发送结果未确认".into();}}
    state.baseline_reset=true;state.baseline.clear();serde_json::to_value(state).map_err(|_|"通知来源无效".into())
}

#[derive(Debug,PartialEq)]
pub(crate) enum Outcome{Unknown,Accepted,Temporary(&'static str),Permanent(&'static str)}
pub struct Notifications{weixin:Arc<floatboard_storage::weixin::Weixin>,#[cfg(test)]test_url:Option<url::Url>}
impl Notifications {
    pub fn new(root:&std::path::Path)->Result<Self>{Ok(Self{weixin:Arc::new(floatboard_storage::weixin::Weixin::new(root.join("weixin-production"))?),#[cfg(test)]test_url:None})}
    #[cfg(test)]pub(crate) fn for_test(root:&std::path::Path,url:url::Url)->Result<Self>{let mut value=Self::new(root)?;value.test_url=Some(url);Ok(value)}
    fn snapshot(&self,state:&State)->Result<Value>{
        let wx=self.weixin.call("snapshot",&json!({}))?;
        Ok(json!({"enabled":state.enabled,"subscriptions":state.subscriptions,"revision":state.revision,"weixin":wx,"channels":state.channels.iter().map(|c|json!({"id":c.id,"name":match c.id.as_str(){"wechat"=>"微信","wecom"=>"企业微信",_=>"飞书"},"enabled":c.enabled,"configured":if c.id=="wechat"{wx["bound"]==true}else{!c.webhook.is_empty()},"status":if c.id=="wechat"&&wx["bound"]!=true||c.id!="wechat"&&c.webhook.is_empty(){"unconfigured"}else if !c.enabled{"disabled"}else{"ready"},"keyword":c.keyword,"has_signing_secret":!c.signing_secret.is_empty()})).collect::<Vec<_>>(),"records":state.records.iter().rev().map(|r|json!({"id":r.id,"created_at":r.created_at,"channel":r.channel,"event":r.event,"title":r.title,"status":r.status,"attempts":r.attempts,"error":r.error})).collect::<Vec<_>>()}))
    }
    pub fn call(&self,db:&Connection,method:&str,params:&Value)->Result<Value>{
        let mut state=load(db)?;
        let channel=params["id"].as_str().unwrap_or("");
        match method {
            "notification_snapshot"=>return self.snapshot(&state),
            "notification_save"=>{state.enabled=params["enabled"].as_bool().ok_or("通知开关无效")?;state.baseline_reset=true;if let Some(subs)=params["subscriptions"].as_object(){for (k,v) in subs{if !state.subscriptions.contains_key(k){return Err("通知订阅无效".into());}state.subscriptions.insert(k.clone(),v.as_bool().ok_or("通知订阅无效")?);}}},
            "notification_channel_save"=>{
                let c=state.channels.iter_mut().find(|c|c.id==channel).ok_or("渠道不存在")?;c.enabled=params["enabled"].as_bool().ok_or("通知渠道开关无效")?;
                if channel=="wechat" {if c.enabled&&self.weixin.call("snapshot",&json!({}))?["bound"]!=true{return Err("请先绑定微信收件人".into());}}
                else{c.keyword=bounded(params["keyword"].as_str().unwrap_or(""),100);
                    if let Some(url)=params["webhook"].as_str().filter(|s|!s.trim().is_empty()){validate_url(channel,url.trim())?;c.webhook=protect(url.trim())?;}
                    if params["clear_signing_secret"]==true{c.signing_secret.clear();}else if let Some(secret)=params["signing_secret"].as_str().filter(|s|!s.is_empty()){if channel!="feishu"||secret.len()>4096{return Err("签名密钥无效".into());}c.signing_secret=protect(secret)?;}
                    if c.enabled&&c.webhook.is_empty(){return Err("请先配置Webhook".into());}}
                state.baseline_reset=true;
            },
            "notification_channel_delete"=>{let c=state.channels.iter_mut().find(|c|c.id==channel).ok_or("渠道不存在")?;*c=Channel{id:channel.into(),..Default::default()};for r in &mut state.records{if r.channel==channel&&r.status=="pending"{r.status="skipped".into();}}},
            "notification_test"=>{let c=state.channels.iter().find(|c|c.id==channel).ok_or("渠道不存在")?;if !state.enabled||!c.enabled{return Err("请先开启外部通知与渠道".into());}if channel=="wechat"&&self.weixin.call("snapshot",&json!({}))?["bound"]!=true{return Err("请先绑定微信收件人".into());}enqueue(&mut state,channel,"test","任务侧窗 · 测试通知",None);},
            "notification_retry"=>{let old=state.records.iter().find(|r|r.id==channel).cloned().ok_or("通知记录不存在")?;if old.status=="unknown"||old.status=="pending"{return Err("通知发送中或结果未知，不支持重发此条".into());}if !state.enabled||!state.channels.iter().any(|c|c.id==old.channel&&c.enabled){return Err("请先开启通知渠道".into());}enqueue(&mut state,&old.channel,&old.event,&old.title,None);},
            "notification_clear"=>state.records.retain(|r|r.status=="pending"),
            "notification_weixin_action"=>{let action=params["action"].as_str().ok_or("缺少微信操作")?;if action=="disconnect"{if let Some(c)=state.channels.iter_mut().find(|c|c.id=="wechat"){c.enabled=false;}}},
            _=>return Err("未知通知方法".into())
        }
        for r in &mut state.records{if r.status=="pending"&&(!state.enabled||!state.channels.iter().any(|c|c.id==r.channel&&c.enabled)){r.status=if r.lease_until>0.{"unknown"}else{"skipped"}.into();}}
        state.revision+=1;prune(&mut state,now());save(db,&state)?;
        if state.baseline_reset{observe_in(db)?;state=load(db)?;}
        self.snapshot(&state)
    }
    pub fn after_commit(&self,db:&Connection,method:&str,params:&Value)->Result<Value>{
        if method=="notification_weixin_action"{self.weixin.call(params["action"].as_str().ok_or("缺少微信操作")?,params)?;}
        let state=load(db)?;
        if !state.enabled||!state.channels.iter().any(|c|c.id=="wechat"&&c.enabled){self.weixin.halt()?;}
        self.snapshot(&state)
    }
    pub(crate) fn claim(&self,db:&Connection)->Result<Option<(Channel,Record)>>{
        let tx=db.unchecked_transaction().map_err(|_|"通知认领事务不可用")?;
        let mut state=load(&tx)?;let before=serde_json::to_value(&state).map_err(|_|"通知状态无效")?;prune(&mut state,now());
        let wx=self.weixin.call("snapshot",&json!({}))?;
        if state.enabled&&state.channels.iter().any(|c|c.id=="wechat"&&c.enabled)&&wx["bound"]==true&&wx["active"]!=true&&wx["error"].as_str().unwrap_or("").is_empty(){let _=self.weixin.call("resume",&json!({}));}
        let work=if let Some(r)=state.records.iter_mut().find(|r|r.status=="pending"&&r.next_at<=now()&&r.lease_until<=now()){
            if r.lease_until>0.&&r.attempts>0{r.status="unknown".into();r.error="上次发送中断，可能已送达；不会自动重发".into();None}
            else if !state.enabled{r.status="skipped".into();None}
            else if let Some(c)=state.channels.iter().find(|c|c.id==r.channel&&c.enabled){if c.id=="wechat"&&wx["bound"]!=true{None}else{r.attempts+=1;r.lease_until=now()+30.;r.account_version=wx["account_version"].as_str().unwrap_or("").into();Some((c.clone(),r.clone()))}}else{r.status="skipped".into();None}
        }else{None};if json!(state)!=before{save(&tx,&state)?;crate::storage::advance(&tx)?;}tx.commit().map_err(|_|"通知认领事务提交失败")?;Ok(work)
    }
    pub(crate) fn dispatch(&self,channel:Channel,record:Record)->(String,Outcome){
        #[cfg(test)]if let Some(url)=&self.test_url{let client=reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().unwrap();let outcome=send_to(&client,&channel,&record,url.clone());return (record.id,outcome);}
        let outcome=if channel.id=="wechat"{match self.weixin.send(&record.account_version,&record.id,&record.title){floatboard_storage::weixin::SendOutcome::Accepted=>Outcome::Accepted,floatboard_storage::weixin::SendOutcome::Unknown=>Outcome::Unknown,floatboard_storage::weixin::SendOutcome::Failed=>Outcome::Permanent("微信未受理或会话不可用")}}
        else{match reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(8)).connect_timeout(Duration::from_secs(4)).build(){Ok(client)=>send(&client,&channel,&record),Err(_)=>Outcome::Permanent("通知客户端不可用")}};
        (record.id,outcome)
    }
    pub(crate) fn finish(db:&Connection,id:&str,outcome:Outcome)->Result<()>{
        let tx=db.unchecked_transaction().map_err(|_|"通知结果事务不可用")?;
        let mut state=load(&tx)?;if let Some(r)=state.records.iter_mut().find(|r|r.id==id&&r.status=="pending"){
            r.lease_until=0.;match outcome{Outcome::Accepted=>{r.status="accepted".into();r.error.clear();},Outcome::Unknown=>{r.status="unknown".into();r.error="发送结果未知，可能已送达；不会自动重发".into();},Outcome::Permanent(e)=>{r.status="failed".into();r.error=e.into();},Outcome::Temporary(e)=>{r.error=e.into();if r.attempts>=3{r.status="failed".into();}else{r.next_at=now()+if r.attempts==1{5.}else{30.};}}}}
        save(&tx,&state)?;crate::storage::advance(&tx)?;tx.commit().map_err(|_|"通知结果事务提交失败".into())
    }
    pub fn shutdown(&self){self.weixin.shutdown();}
}
fn bounded(s:&str,n:usize)->String{s.chars().filter(|c|!c.is_control()||*c=='\n').take(n).collect()}
fn load(db:&Connection)->Result<State>{let raw=crate::storage::get(db,"notifications")?.unwrap_or_else(||json!(State::default()));serde_json::from_value(raw).map_err(|_|"通知数据损坏".into())}
fn save(db:&Connection,state:&State)->Result<()>{crate::storage::put(db,"notifications",&json!(state))}
pub fn observe_in(db:&Connection)->Result<()>{
    let mut value=load(db)?;let state=&mut value;
    let tasks={let mut statement=db.prepare("select payload from tasks").map_err(|_|"通知任务读取失败")?;let rows=statement.query_map([],|r|r.get::<_,String>(0)).map_err(|_|"通知任务读取失败")?;rows.map(|r|r.map_err(|_|"通知任务读取失败".to_string()).and_then(|s|serde_json::from_str::<Value>(&s).map_err(|_|"通知任务格式无效".into()))).collect::<Result<Vec<_>>>()?};
        let baseline=state.baseline_reset;
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
                    for channel in &channels {let key=floatboard_protocol::hash(format!("{id}\0{event}\0{stable}\0{}\0{channel}",next.revision).as_bytes());enqueue(state,channel,event,&title,Some(key));}
                }
            }
            current.insert(id.into(),next);
        }
        state.baseline=current;state.baseline_reset=false;prune(state,now());
    save(db,&value)
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
pub(crate) fn send(client:&reqwest::blocking::Client,c:&Channel,r:&Record)->Outcome {
    let Ok(webhook)=unprotect(&c.webhook)else{return Outcome::Permanent("本地凭据无法解密，请重新配置")};
    let Ok(url)=validate_url(&c.id,&webhook)else{return Outcome::Permanent("Webhook 地址无效，请重新配置")};
    send_to(client,c,r,url)
}
fn send_to(client:&reqwest::blocking::Client,c:&Channel,r:&Record,url:url::Url)->Outcome {
    let payload=match payload(c,r,&(now() as u64).to_string()){Ok(value)=>value,Err(_)=>return Outcome::Permanent("本地签名密钥无法解密，请重新配置")};
    let response=match client.post(url).header(reqwest::header::CONTENT_TYPE,"application/json").body(payload.to_string()).send(){Ok(r)=>r,Err(e)=>return if e.is_connect(){Outcome::Temporary("网络暂不可用")}else{Outcome::Unknown}};
    let status=response.status().as_u16();
    if status==429||status>=500{return classify_response(&c.id,status,b"");}
    if !(200..300).contains(&status){return classify_response(&c.id,status,b"");}
    // Bound provider response; never echo it, as it may contain request secrets.
    use std::io::Read;
    let mut bytes=Vec::new();if response.take(65537).read_to_end(&mut bytes).is_err()||bytes.len()>65536{return Outcome::Unknown;}
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
    let Ok(value)=serde_json::from_slice::<Value>(bytes)else{return Outcome::Unknown};
    let code=if channel=="wecom"{value["errcode"].as_i64()}else{value.get("code").or_else(||value.get("StatusCode")).and_then(Value::as_i64)};
    match code {Some(0)=>Outcome::Accepted,Some(45009|45011|99991400|99991401)=>Outcome::Temporary("平台频率限制"),Some(_)=>Outcome::Permanent("平台未受理，请检查机器人配置与签名"),None=>Outcome::Unknown}
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
fn enqueue(s:&mut State,channel:&str,event:&str,title:&str,id:Option<String>) {
    let id=id.unwrap_or_else(||uuid::Uuid::new_v4().to_string());
    if s.records.iter().any(|r|r.id==id){return;}
    s.records.push(Record{account_version:String::new(),id,created_at:now(),channel:channel.into(),event:event.into(),title:bounded(title,400),status:"pending".into(),attempts:0,error:String::new(),next_at:0.,lease_until:0.});
}
fn prune(s:&mut State,at:f64) {
    for r in &mut s.records {if r.status=="pending"&&at-r.created_at>=EXPIRY&&r.lease_until<=at {if r.lease_until>0.&&r.attempts>0{r.status="unknown".into();r.error="上次发送中断，可能已送达；不会自动重发".into();}else{r.status="expired".into();r.error="通知已超过30分钟有效期".into();}}}
    s.records.retain(|r|at-r.created_at<RETENTION);
    if s.records.len()>500 {s.records.drain(..s.records.len()-500);}
}

#[cfg(test)]mod tests{
use super::*;use std::{io::{Read,Write},net::TcpListener};
fn local_response(response:&'static [u8])->Outcome{
 let listener=TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
 let server=std::thread::spawn(move||{let(mut socket,_)=listener.accept().unwrap();let mut bytes=[0;4096];assert!(socket.read(&mut bytes).unwrap()>0);socket.write_all(response).unwrap();});
 let c=Channel{id:"wecom".into(),..Default::default()};let mut state=State::default();enqueue(&mut state,"wecom","test","synthetic",None);
 let result=send_to(&reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().unwrap(),&c,&state.records[0],format!("http://{address}/test").parse().unwrap());server.join().unwrap();result
}
#[test]fn local_service_acceptance_and_ambiguous_results_are_distinct(){
 assert_eq!(local_response(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"errcode\":0}"),Outcome::Accepted);
 assert_eq!(local_response(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx"),Outcome::Unknown);
 assert_eq!(local_response(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\nConnection: close\r\n\r\n{"),Outcome::Unknown);
 assert!(matches!(local_response(b"HTTP/1.1 429 Limited\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),Outcome::Temporary(_)));
}
#[test]fn interrupted_send_is_unknown_even_after_expiry(){let mut state=State::default();enqueue(&mut state,"wecom","test","synthetic",None);state.records[0].created_at=now()-3600.;state.records[0].attempts=1;state.records[0].lease_until=now()-1.;prune(&mut state,now());assert_eq!(state.records[0].status,"unknown");}
#[test]fn production_urls_never_allow_the_local_test_transport(){assert!(validate_url("wecom","http://127.0.0.1/test").is_err());assert!(validate_url("wecom","https://localhost/test").is_err());}
}
