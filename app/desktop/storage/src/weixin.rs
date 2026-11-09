//! Production Weixin transport. Account storage is independent of the isolated lab.
//! Construction/restoration is offline. Only explicit actions activate bounded network work.
use std::{fs, io::Read, path::PathBuf, sync::{Arc, Mutex, Condvar, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}, time::Duration};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::{RngCore, rngs::OsRng};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use crate::{Result, now};
#[cfg(test)] mod tests;
const BASE:&str="https://ilinkai.weixin.qq.com";
const MAX_RESPONSE:u64=1_048_576;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Account { generation:String, token:String, bot_id:String, login_user:String, base:String, recipient:String, context:String, cursor:String, last_received_at:f64, last_sent_at:f64, pause_until:f64 }
fn decode_account(bytes:&[u8])->Result<Option<Account>>{
 let clear=crypt(bytes,false)?;let account:Option<Account>=serde_json::from_slice(&clear).map_err(|_|"微信凭据无效")?;
 if let Some(a)=&account{validate_base(&a.base)?;if a.generation.is_empty()||a.token.is_empty()||a.login_user.is_empty()||a.bot_id.is_empty()||(!a.recipient.is_empty()&&(a.recipient!=a.login_user||a.context.is_empty())){return Err("微信凭据不完整".into());}}Ok(account)
}
pub fn validate_account_bytes(bytes:&[u8])->Result<()>{decode_account(bytes).map(|_|())}
#[derive(Clone)]
enum Work { Login, PollLogin, Updates }
struct State { account:Option<Account>, status:String, active:bool, busy:bool, restored:bool, epoch:u64, pending:Option<Work>, qr:String, qr_content:String, qr_base:String, qr_expires:f64, verify:String, bind_code:String, bind_expires:f64, last_test:String, error:String }
impl Default for State {fn default()->Self{Self{account:None,status:"disconnected".into(),active:false,busy:false,restored:false,epoch:0,pending:None,qr:String::new(),qr_content:String::new(),qr_base:BASE.into(),qr_expires:0.,verify:String::new(),bind_code:String::new(),bind_expires:0.,last_test:String::new(),error:String::new()}}}
trait Transport:Send+Sync { fn request(&self,base:&str,path:&str,token:Option<&str>,body:Option<Value>,query:&[(String,String)])->Result<Value>; }
struct Http(reqwest::blocking::Client);
impl Transport for Http {
 fn request(&self,base:&str,path:&str,token:Option<&str>,body:Option<Value>,query:&[(String,String)])->Result<Value>{
  let base=validate_base(base)?;let mut url=reqwest::Url::parse(&format!("{base}/{path}")).map_err(|_|"微信地址无效")?;
  {let mut q=url.query_pairs_mut();for(k,v)in query{q.append_pair(k,v);}}
  let post=body.is_some();let mut r=if post{self.0.post(url)}else{self.0.get(url)};
  r=r.header("iLink-App-Id","bot").header("iLink-App-ClientVersion",((2u32<<16)|(4<<8)|10).to_string());
  if let Some(body)=body{r=r.header("AuthorizationType","ilink_bot_token").header("X-WECHAT-UIN",STANDARD.encode(OsRng.next_u32().to_string())).header(reqwest::header::CONTENT_TYPE,"application/json").body(body.to_string());}
  if let Some(token)=token{r=r.bearer_auth(token);}
  let response=match r.send(){Ok(response)=>response,Err(e) if e.is_timeout()&&path=="ilink/bot/getupdates"=>return Ok(json!({"ret":0,"msgs":[]})),Err(e) if e.is_timeout()&&path=="ilink/bot/get_qrcode_status"=>return Ok(json!({"status":"wait"})),Err(e)=>return Err(if e.is_timeout(){"微信请求超时；请重试"}else{"微信网络请求失败"}.into())};
  if !response.status().is_success(){return Err("微信服务拒绝请求".into());}
  let mut bytes=Vec::new();response.take(MAX_RESPONSE+1).read_to_end(&mut bytes).map_err(|_|"微信响应读取失败")?;
  if bytes.len() as u64>MAX_RESPONSE{return Err("微信响应过大".into());}
  serde_json::from_slice(&bytes).map_err(|_|"微信响应格式无效".into())
 }
}
struct Inner { dir:PathBuf, state:Mutex<State>, wake:Condvar, stopped:AtomicBool, transport:Arc<dyn Transport>, send_gate:Mutex<()>, _owner:floatboard_client::windows::Instance }
pub struct Weixin { inner:Arc<Inner>, worker:Mutex<Option<JoinHandle<()>>> }
impl Weixin {
 pub fn new(directory:PathBuf)->Result<Self>{
  let client=reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(12)).build().map_err(|_|"微信网络组件初始化失败")?;
  Self::build(directory,Arc::new(Http(client)),true)
 }
 fn build(dir:PathBuf,transport:Arc<dyn Transport>,spawn:bool)->Result<Self>{
  fs::create_dir_all(&dir).map_err(|_|"微信目录不可写")?;
  let owner=floatboard_client::windows::Instance::acquire_named(&format!("Weixin.{}",floatboard_protocol::hash(dir.to_string_lossy().to_lowercase().as_bytes()))).map_err(|_|"微信目录已被其他实例占用")?;
  let mut state=State::default();let path=dir.join("account.dpapi");
  if path.exists(){if fs::metadata(&path).map_err(|_|"微信凭据不可读")?.len()>2_097_152{return Err("微信凭据过大".into());}let bytes=fs::read(&path).map_err(|_|"微信凭据不可读")?;if bytes.len()>2_097_152{return Err("微信凭据过大".into());}state.account=decode_account(&bytes)?;if state.account.is_some(){state.status="restored".into();state.restored=true;}}
  let inner=Arc::new(Inner{dir,state:Mutex::new(state),wake:Condvar::new(),stopped:AtomicBool::new(false),transport,send_gate:Mutex::new(()),_owner:owner});
  let worker=if spawn{let i=inner.clone();Some(thread::Builder::new().name("weixin-production".into()).spawn(move||worker(i)).map_err(|_|"微信线程无法启动")?)}else{None};
  Ok(Self{inner,worker:Mutex::new(worker)})
 }
   /// Stops future requests and serializes against an already dispatched send.
  pub fn halt(&self)->Result<()> {let _gate=self.inner.send_gate.lock().map_err(|_|"微信发送状态不可用")?;let mut s=self.inner.state.lock().map_err(|_|"微信状态不可用")?;s.epoch+=1;s.pending=None;s.active=false;s.busy=false;clear_login(&mut s);s.status=if s.account.is_some(){"restored"}else{"disconnected"}.into();Ok(())}
  pub fn send(&self,generation:&str,id:&str,text:&str)->SendOutcome {
   let Ok(_gate)=self.inner.send_gate.lock()else{return SendOutcome::Failed};
   if self.inner.stopped.load(Ordering::Acquire){return SendOutcome::Failed;}
   let (epoch,a)={let Ok(s)=self.inner.state.lock()else{return SendOutcome::Failed};let Some(a)=s.account.as_ref().filter(|a|a.generation==generation&&!a.recipient.is_empty()&&!a.context.is_empty()&&a.pause_until<=now()&&s.active)else{return SendOutcome::Failed};(s.epoch,a.clone())};
   let body=json!({"msg":{"from_user_id":"","to_user_id":a.recipient,"client_id":format!("floatboard-{id}"),"message_type":2,"message_state":2,"item_list":[{"type":1,"text_item":{"text":text}}],"context_token":a.context},"base_info":base_info()});
   let result=self.inner.transport.request(&a.base,"ilink/bot/sendmessage",Some(&a.token),Some(body),&[]);
   let outcome=match result {Ok(v)=>classify_send(&v),Err(_)=>SendOutcome::Unknown};
   if let Ok(mut s)=self.inner.state.lock(){if s.epoch==epoch {if outcome==SendOutcome::Accepted {if let Some(a)=&mut s.account{a.last_sent_at=now();}let _=persist(&self.inner,&s);}if outcome==SendOutcome::Failed{s.error="微信未受理，请检查会话并重新连接".into();s.active=false;}}}
   outcome
  }
pub fn shutdown(&self){self.inner.stopped.store(true,Ordering::Release);self.inner.wake.notify_all();if let Ok(mut w)=self.worker.lock(){if let Some(h)=w.take(){let _=h.join();}}}
 pub fn call(&self,action:&str,params:&Value)->Result<Value>{
   let _gate=if action!="snapshot"{Some(self.inner.send_gate.lock().map_err(|_|"微信发送状态不可用")?)}else{None};
  if self.inner.stopped.load(Ordering::Acquire){return Err("微信已停止".into());}
  let mut s=self.inner.state.lock().map_err(|_|"微信状态不可用")?;
  match action {
   "snapshot"=>{},
   "cancel_login"=>{s.epoch+=1;s.pending=None;s.active=false;s.busy=false;clear_login(&mut s);s.status=if s.account.is_some(){"restored"}else{"disconnected"}.into();},
   "disconnect"=>{s.epoch+=1;s.pending=None;s.active=false;s.busy=false;s.account=None;s.restored=false;s.bind_code.clear();s.bind_expires=0.;clear_login(&mut s);s.status="disconnected".into();s.error.clear();s.last_test.clear();persist(&self.inner,&s)?;},
   "start_login"=>{if s.busy{return Err("微信正在处理，请先取消或等待".into());}if s.account.is_some(){return Err("请先解除现有微信连接".into());}s.epoch+=1;clear_login(&mut s);s.error.clear();s.status="starting_login".into();s.busy=true;s.active=true;s.pending=Some(Work::Login);},
   "verify_code"=>{if s.status!="need_verifycode"{return Err("当前无需输入验证码".into());}let code=params["code"].as_str().unwrap_or("").trim();if code.is_empty()||code.len()>16||!code.bytes().all(|b|b.is_ascii_digit()){return Err("验证码应为不超过16位的数字".into());}s.verify=code.into();s.busy=true;s.pending=Some(Work::PollLogin);},
   "resume"|"start_binding"=>{
    if s.busy{return Err("微信正在处理，请稍后重试".into());}
    let a=s.account.as_ref().ok_or("请先扫码登录")?;if a.pause_until>now(){return Err("微信会话已暂停，请稍后重新登录".into());}
         if action=="start_binding"{let mut random=[0u8;16];OsRng.fill_bytes(&mut random);s.bind_code=format!("绑定 {}",random.iter().map(|b|format!("{b:02x}")).collect::<String>());s.bind_expires=now()+300.;s.status="binding".into();}else{let status=if a.recipient.is_empty(){"logged_in"}else{"bound"};s.restored=false;s.status=status.into();}s.pending=Some(Work::Updates);
s.active=true;s.error.clear();
   },
   _=>return Err("未知微信操作".into())
  }
  self.inner.wake.notify_all();Ok(snapshot(&s))
 }
}
impl Drop for Weixin {fn drop(&mut self){self.shutdown();}}
fn clear_login(s:&mut State){s.qr.clear();s.qr_content.clear();s.qr_expires=0.;s.qr_base=BASE.into();s.verify.clear();}
fn snapshot(s:&State)->Value{
 let a=s.account.as_ref();json!({"status":s.status,"busy":s.busy,"active":s.active,"logged_in":a.is_some(),"bound":a.is_some_and(|a|!a.recipient.is_empty()),"restored":s.restored,"qrcode":s.qr,"qrcode_img_content":s.qr_content,"qr_expires_at":s.qr_expires,"binding_code":if s.bind_expires>now(){s.bind_code.as_str()}else{""},"binding_expires_at":s.bind_expires,"recipient_masked":a.filter(|a|!a.recipient.is_empty()).map(|_|"已绑定扫码微信用户").unwrap_or(""),"last_received_at":a.map(|a|a.last_received_at).unwrap_or(0.),"last_sent_at":a.map(|a|a.last_sent_at).unwrap_or(0.),"pause_until":a.map(|a|a.pause_until).unwrap_or(0.),"last_test_status":s.last_test,"error":s.error,"account_version":a.map(|a|a.generation.as_str()).unwrap_or("")})
}
fn persist(i:&Inner,s:&State)->Result<()>{let bytes=serde_json::to_vec(&s.account).map_err(|_|"微信凭据编码失败")?;floatboard_client::files::atomic_bytes(&i.dir.join("account.dpapi"),&crypt(&bytes,true)?)}
fn validate_base(raw:&str)->Result<String>{
 if raw.len()>1024||raw.contains('\\')||raw.chars().any(|c|c.is_control()) {return Err("微信服务地址不安全".into());}
 let u=reqwest::Url::parse(raw).map_err(|_|"微信服务地址无效")?;
 let h=u.host_str().ok_or("微信服务地址无效")?;
 if u.scheme()!="https"||!u.username().is_empty()||u.password().is_some()||u.port().is_some_and(|p|p!=443)||u.query().is_some()||u.fragment().is_some()||u.path()!="/"&& !u.path().is_empty()||!(h=="ilinkai.weixin.qq.com"||h.ends_with(".weixin.qq.com")){return Err("微信服务地址不安全".into());}
 Ok(format!("https://{h}"))
}
fn required(v:&Value,key:&str,limit:usize)->Result<String>{let s=v[key].as_str().ok_or("微信响应缺少必要字段")?;if s.is_empty()||s.len()>limit||s.chars().any(|c|c.is_control()){return Err("微信响应字段无效".into());}Ok(s.into())}
fn base_info()->Value{json!({"channel_version":"2.4.10","bot_agent":concat!("CodexFloatboard/",env!("CARGO_PKG_VERSION"))})}
fn checked(v:&Value)->Result<()>{if v["ret"]==-14||v["errcode"]==-14{return Err("会话失效，已暂停一小时；请重新连接".into());}for key in ["ret","errcode"]{if let Some(c)=v.get(key){if c.as_i64()!=Some(0){return Err("微信服务未受理请求".into());}}}Ok(())}
fn worker(i:Arc<Inner>){while !i.stopped.load(Ordering::Acquire){step(&i);let Ok(s)=i.state.lock()else{break};drop(i.wake.wait_timeout(s,Duration::from_secs(1)));}}
fn step(i:&Inner){
 let Ok(_gate)=i.send_gate.lock()else{return};
 if i.stopped.load(Ordering::Acquire){return;}
 let (epoch,work,account,qr,verify,qr_base)={let Ok(mut s)=i.state.lock()else{return};
  if s.qr_expires>0.&&now()>s.qr_expires&&!s.qr.is_empty(){clear_login(&mut s);s.busy=false;s.active=false;s.pending=None;s.status="expired".into();}
  if s.bind_expires>0.&&now()>s.bind_expires&&!s.bind_code.is_empty(){s.bind_code.clear();s.bind_expires=0.;s.error="绑定码已过期，请重新生成".into();s.status=if s.account.as_ref().is_some_and(|a|!a.recipient.is_empty()){"bound"}else{"logged_in"}.into();}
  let work=s.pending.take().or_else(||if s.active{if !s.qr.is_empty()&&s.status!="need_verifycode"{Some(Work::PollLogin)}else if s.account.is_some(){Some(Work::Updates)}else{None}}else{None});
  let Some(work)=work else{return};(s.epoch,work,s.account.clone(),s.qr.clone(),s.verify.clone(),s.qr_base.clone())};
 let result=(||->Result<Value>{match &work{
  Work::Login=>i.transport.request(BASE,"ilink/bot/get_bot_qrcode",None,Some(json!({"local_token_list":[]})),&[("bot_type".into(),"3".into())]),
  Work::PollLogin=>{let mut q=vec![("qrcode".into(),qr)];if !verify.is_empty(){q.push(("verify_code".into(),verify));}i.transport.request(&qr_base,"ilink/bot/get_qrcode_status",None,None,&q)},
     Work::Updates=>{let a=account.as_ref().ok_or("微信账号不可用")?;i.transport.request(&a.base,"ilink/bot/getupdates",Some(&a.token),Some(json!({"get_updates_buf":a.cursor,"base_info":base_info()})),&[])},}} )();
 let Ok(mut s)=i.state.lock()else{return};if s.epoch!=epoch||i.stopped.load(Ordering::Acquire){return;}
 let result=result.and_then(|v|{checked(&v)?;apply(&mut s,&work,&v)?;if s.account.is_some(){persist(i,&s)?;}Ok(())});
 if let Err(error)=result {if error.starts_with("会话失效"){if let Some(a)=&mut s.account{a.pause_until=now()+3600.;}let _=persist(i,&s);s.status="paused".into();}else{s.status="error".into();}s.error=error;s.busy=false;s.active=false;s.pending=None;}
}
fn apply(s:&mut State,work:&Work,v:&Value)->Result<()>{match work{
 Work::Login=>{s.qr=required(v,"qrcode",8192)?;s.qr_content=required(v,"qrcode_img_content",8192)?;s.qr_expires=now()+480.;s.status="waiting_scan".into();s.busy=false;},
 Work::PollLogin=>{match v["status"].as_str().ok_or("微信登录响应无效")?{
  "wait"=>s.status="waiting_scan".into(),"scaned"=>{s.status="scanned".into();s.verify.clear();},"need_verifycode"=>{s.status="need_verifycode".into();s.verify.clear();},
  "expired"|"verify_code_blocked"=>{clear_login(s);s.status="expired".into();s.active=false;},
  "scaned_but_redirect"=>{let host=required(v,"redirect_host",512)?;s.qr_base=validate_base(&if host.contains("://"){host}else{format!("https://{host}")})?;},
  "confirmed"=>{let a=Account{generation:uuid::Uuid::new_v4().to_string(),token:required(v,"bot_token",16384)?,bot_id:required(v,"ilink_bot_id",1024)?,login_user:required(v,"ilink_user_id",1024)?,base:validate_base(v["baseurl"].as_str().unwrap_or(BASE))?,..Account::default()};s.account=Some(a);clear_login(s);s.active=false;s.restored=false;s.status="logged_in".into();},
  "binded_redirect"=>return Err("该机器人已绑定其他实例，请重新发起独立连接".into()),_=>return Err("微信登录状态不受支持".into())
 }s.busy=false;},
 Work::Updates=>{apply_updates(s,v)?;},
 }Ok(())}
fn apply_updates(s:&mut State,v:&Value)->Result<()>{
 let a=s.account.as_mut().ok_or("微信账号不可用")?;
 if let Some(cursor)=v.get("get_updates_buf"){let cursor=cursor.as_str().ok_or("微信游标格式无效")?;if cursor.len()>262144{return Err("微信游标过大".into());}a.cursor=cursor.into();}
 if let Some(msgs)=v.get("msgs"){let msgs=msgs.as_array().ok_or("微信消息格式无效")?;if msgs.len()>1000{return Err("微信消息过多".into());}for msg in msgs{
  if msg["from_user_id"].as_str()!=Some(a.login_user.as_str())||msg["to_user_id"].as_str()!=Some(a.bot_id.as_str())||msg["message_type"]!=1||msg["group_id"].as_str().is_some_and(|g|!g.is_empty()){continue;}
  let context=msg["context_token"].as_str().filter(|c|!c.is_empty()&&c.len()<=16384);let Some(context)=context else{continue};
  let matches=s.bind_expires>now()&&!s.bind_code.is_empty()&&msg["item_list"].as_array().is_some_and(|items|items.iter().any(|item|item["type"]==1&&item["text_item"]["text"].as_str().is_some_and(|t|t.trim()==s.bind_code)));
  if matches{a.recipient=a.login_user.clone();a.context=context.into();a.last_received_at=now();s.bind_code.clear();s.bind_expires=0.;s.status="bound".into();s.active=false;}
  else if a.recipient==a.login_user&&!a.recipient.is_empty(){a.context=context.into();a.last_received_at=now();}
 }}Ok(())
}
fn crypt(bytes:&[u8],encrypt:bool)->Result<Vec<u8>>{
 use windows_sys::Win32::{Security::Cryptography::{CRYPT_INTEGER_BLOB,CryptProtectData,CryptUnprotectData,CRYPTPROTECT_UI_FORBIDDEN},Foundation::LocalFree};
 let input=CRYPT_INTEGER_BLOB{cbData:bytes.len().try_into().map_err(|_|"微信凭据过长")?,pbData:bytes.as_ptr() as *mut u8};let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()};
 let ok=unsafe{if encrypt{CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}else{CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}};
 if ok==0{return Err("微信凭据无法由当前 Windows 用户保护或解密".into());}let result=unsafe{std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec()};unsafe{LocalFree(output.pbData as *mut _);};Ok(result)
}

#[derive(Debug,PartialEq)] pub enum SendOutcome { Accepted, Unknown, Failed }
fn classify_send(v:&Value)->SendOutcome {
 if ["ret","errcode"].iter().any(|k|v.get(k).is_some_and(|c|c.as_i64()!=Some(0))){return SendOutcome::Failed;}
 let message_id=v.get("message_id").is_some_and(|id|id.as_u64().is_some_and(|id|id>0)||id.as_str().is_some_and(|id|!id.is_empty()&&id.len()<=32&&id.bytes().all(|b|b.is_ascii_digit())&&id.bytes().any(|b|b!=b'0')));
 if v["ret"].as_i64()==Some(0)||message_id{SendOutcome::Accepted}else{SendOutcome::Unknown}
}
