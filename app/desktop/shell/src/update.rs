//! Signed, stable-channel updates. Network work never holds the state mutex.
use std::{fs,io::{Read,Write},path::PathBuf,sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::Duration};
use base64::{Engine as _,engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature,VerifyingKey};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use crate::Result;
use floatboard_client::files::{atomic_json,read_json};
const REPO:&str="Memento1990/codex-task";
const API:&str="https://api.github.com/repos/Memento1990/codex-task/releases/latest";
const MAX_ARCHIVE:u64=512*1024*1024;
const MAX_MANIFEST:u64=64*1024;

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest{pub schema:u32,pub version:String,pub platform:String,pub minimum_version:String,pub updater_protocol:u32,pub database_schema_min:u32,pub database_schema_max:u32,pub agent_protocol:u32,pub url:String,pub size:u64,pub sha256:String}
#[derive(Clone,Debug)]
pub struct VerifiedUpdate{pub manifest:Manifest,pub archive:PathBuf,pub manifest_bytes:Vec<u8>,pub signature_bytes:Vec<u8>,pub manifest_path:PathBuf,pub signature_path:PathBuf}
fn key_from_base64(key:&str)->Result<VerifyingKey>{let bytes=STANDARD.decode(key.trim()).map_err(|_|"更新公钥编码无效")?;let bytes:[u8;32]=bytes.try_into().map_err(|_|"更新公钥长度无效")?;VerifyingKey::from_bytes(&bytes).map_err(|_|"更新公钥无效".into())}
pub fn verify_manifest(bytes:&[u8],signature:&[u8])->Result<Manifest>{verify_manifest_with_key(bytes,std::str::from_utf8(signature).map_err(|_|"更新签名编码无效")?,option_env!("FLOATBOARD_UPDATE_PUBLIC_KEY").ok_or("发行版缺少更新公钥")?,env!("CARGO_PKG_VERSION"))}
pub fn verify_manifest_with_key(bytes:&[u8],signature:&str,key:&str,current:&str)->Result<Manifest>{
 let manifest=verify_signed_manifest(bytes,signature,key)?;
 let current=semver::Version::parse(current).map_err(|_|"当前版本无效")?;let version=semver::Version::parse(&manifest.version).map_err(|_|"更新版本无效")?;let minimum=semver::Version::parse(&manifest.minimum_version).map_err(|_|"最低版本无效")?;
 if version<=current||minimum>current{return Err("更新版本不适用或需要先手动升级".into());}Ok(manifest)
}
pub fn verify_signed_manifest(bytes:&[u8],signature:&str,key:&str)->Result<Manifest>{
 if bytes.len() as u64>MAX_MANIFEST{return Err("更新清单过大".into());}
 let key=key_from_base64(key)?;let signature=STANDARD.decode(signature.trim()).map_err(|_|"更新签名编码无效")?;let signature=Signature::from_slice(&signature).map_err(|_|"更新签名长度无效")?;
 key.verify_strict(bytes,&signature).map_err(|_|"更新清单签名校验失败，未下载")?;
 let manifest:Manifest=serde_json::from_slice(bytes).map_err(|_|"更新清单格式无效")?;
 let version=semver::Version::parse(&manifest.version).map_err(|_|"更新版本无效")?;let minimum=semver::Version::parse(&manifest.minimum_version).map_err(|_|"最低版本无效")?;
 if manifest.schema!=2||manifest.platform!="windows-x86_64"||manifest.updater_protocol!=2||manifest.agent_protocol!=2||manifest.database_schema_min>2||manifest.database_schema_max<2||manifest.database_schema_min>manifest.database_schema_max{return Err("更新协议或数据兼容性不受支持".into());}
 if !version.pre.is_empty()||!version.build.is_empty()||!minimum.build.is_empty()||!minimum.pre.is_empty()||minimum>version{return Err("更新版本不适用或需要先手动升级".into());}
 if manifest.size==0||manifest.size>MAX_ARCHIVE||manifest.sha256.len()!=64||!manifest.sha256.bytes().all(|b|b.is_ascii_hexdigit()){return Err("更新文件大小或校验无效".into());}
 if !release_url(&manifest.url){return Err("更新下载地址不在官方仓库".into());}
 let url=reqwest::Url::parse(&manifest.url).map_err(|_|"更新地址无效")?;
 let prefix=format!("/{REPO}/releases/download/v{}/",manifest.version);
 if !url.path().starts_with(&prefix){return Err("下载文件与签名版本不匹配".into());}
 Ok(manifest)
}
fn safe_url(url:&reqwest::Url)->bool{url.scheme()=="https"&&url.username().is_empty()&&url.password().is_none()&&url.port_or_known_default()==Some(443)&&url.fragment().is_none()}
pub fn release_url(value:&str)->bool{reqwest::Url::parse(value).is_ok_and(|url|safe_url(&url)&&url.host_str()==Some("github.com")&&url.query().is_none()&&url.path().starts_with(&format!("/{REPO}/releases/download/"))&&!url.path().contains("%"))}
fn redirect_allowed(url:&reqwest::Url)->bool{safe_url(url)&&(release_url(url.as_str())||url.host_str()==Some("release-assets.githubusercontent.com")||url.host_str()==Some("objects.githubusercontent.com"))}
fn client()->Result<reqwest::blocking::Client>{reqwest::blocking::Client::builder().user_agent(concat!("CodexFloatboard/",env!("CARGO_PKG_VERSION"))).connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(180)).redirect(reqwest::redirect::Policy::custom(|attempt|{if attempt.previous().len()>=5||!redirect_allowed(attempt.url()){attempt.error("更新重定向地址不受信任")}else{attempt.follow()}})).build().map_err(|_|"更新网络服务不可用".into())}
fn bounded(mut response:reqwest::blocking::Response,limit:u64)->Result<Vec<u8>>{if !response.status().is_success(){return Err(format!("更新服务器返回 {}",response.status()));}if response.content_length().is_some_and(|size|size>limit){return Err("更新响应过大".into());}let mut bytes=Vec::new();response.by_ref().take(limit+1).read_to_end(&mut bytes).map_err(|_|"更新响应读取失败")?;if bytes.len() as u64>limit{return Err("更新响应过大".into());}Ok(bytes)}
#[derive(Clone,Serialize,Deserialize)]
#[serde(default,deny_unknown_fields)]
pub struct Preferences{pub auto_check:bool,pub auto_download:bool}
impl Default for Preferences{fn default()->Self{Self{auto_check:true,auto_download:true}}}
struct State{state:String,latest_version:Option<String>,release_notes:String,release_url:String,downloaded_bytes:u64,total_bytes:u64,error:Option<String>,last_checked:Option<f64>,preferences:Preferences,verified:Option<VerifiedUpdate>,manifest:Option<Manifest>,manifest_bytes:Vec<u8>,signature_bytes:Vec<u8>}
pub struct UpdateService{root:PathBuf,key:Option<String>,state:Mutex<State>,busy:AtomicBool,cancelled:AtomicBool}
struct Busy<'a>(&'a AtomicBool);impl Drop for Busy<'_>{fn drop(&mut self){self.0.store(false,Ordering::SeqCst);}}
impl UpdateService{
 pub fn new(root:PathBuf)->Arc<Self>{let root=crate::paths::control_root(&root).join("updates");let configured=option_env!("FLOATBOARD_UPDATE_PUBLIC_KEY");let key_error=configured.and_then(|key|key_from_base64(key).err());let key=if cfg!(all(target_os="windows",target_arch="x86_64")){configured.filter(|key|key_from_base64(key).is_ok()).map(str::to_owned)}else{None};let preferences=read_json(&root.join("preferences.json")).ok().and_then(|value|serde_json::from_value(value).ok()).unwrap_or_default();let service=Arc::new(Self{root:root.clone(),key:key.clone(),state:Mutex::new(State{state:if key.is_some(){"idle"}else{"unsupported"}.into(),latest_version:None,release_notes:String::new(),release_url:String::new(),downloaded_bytes:0,total_bytes:0,error:key_error,last_checked:read_json(&root.join("status.json")).ok().and_then(|value|value["last_checked"].as_f64()),preferences,verified:None,manifest:None,manifest_bytes:Vec::new(),signature_bytes:Vec::new()}),busy:AtomicBool::new(false),cancelled:AtomicBool::new(false)});if service.key.is_some(){let _=service.restore_ready();}service}
 fn lock(&self)->std::sync::MutexGuard<'_,State>{self.state.lock().unwrap_or_else(|error|error.into_inner())}
 pub fn snapshot(&self)->Value{let state=self.lock();json!({"current_version":env!("CARGO_PKG_VERSION"),"supported":self.key.is_some(),"state":state.state,"latest_version":state.latest_version,"release_notes":state.release_notes,"release_url":state.release_url,"downloaded_bytes":state.downloaded_bytes,"total_bytes":state.total_bytes,"error":state.error,"last_checked":state.last_checked,"preferences":state.preferences,"installation_result":crate::update_install::last_result()})}
 fn enter(&self,state:&str)->Result<Busy<'_>>{if self.lock().state=="installing"{return Err("更新正在安装".into());}if self.key.is_none(){return Err("此发行版未配置更新签名公钥，请从官方发布页手动更新".into());}if self.busy.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_err(){return Err("另一个更新操作正在进行".into());}let guard=Busy(&self.busy);self.cancelled.store(false,Ordering::SeqCst);{let mut value=self.lock();if value.state=="installing"{return Err("更新正在安装".into());}value.state=state.into();value.error=None;if state=="downloading"{value.downloaded_bytes=0;}}Ok(guard)}
 fn finish(&self,result:Result<()>)->Result<Value>{if let Err(error)=result{let ready=self.verified_stage().is_ok();let mut state=self.lock();state.state=if ready{"ready"}else if self.cancelled.load(Ordering::SeqCst){"idle"}else{"error"}.into();state.error=if self.cancelled.load(Ordering::SeqCst){None}else{Some(error.clone())};return Err(error);}Ok(self.snapshot())}
 fn retry_pause(&self,attempt:usize)->Result<()>{for _ in 0..(10*(attempt+1)){self.cancelled()?;std::thread::sleep(Duration::from_millis(50));}Ok(())}
 fn send(&self,request:reqwest::blocking::RequestBuilder)->Result<reqwest::blocking::Response>{
  for attempt in 0..3 {self.cancelled()?;let cloned=request.try_clone().ok_or("更新请求无法重试")?;match cloned.send(){Ok(response) if response.status().is_server_error()||response.status()==reqwest::StatusCode::TOO_MANY_REQUESTS=>{if attempt==2{return Ok(response);}},Ok(response)=>return Ok(response),Err(_)if attempt==2=>return Err("更新网络请求失败，请检查网络或系统代理".into()),Err(_)=>{}}self.retry_pause(attempt)?;}
  Err("更新请求失败".into())
 }
 fn cancelled(&self)->Result<()>{if self.cancelled.load(Ordering::SeqCst){Err("更新已取消".into())}else{Ok(())}}
 pub fn cancel(&self)->Result<Value>{if self.lock().state=="installing"{return Err("更新已进入安装阶段，不能取消".into());}self.cancelled.store(true,Ordering::SeqCst);Ok(self.snapshot())}
 pub fn preferences(&self,value:&Value)->Result<Value>{crate::paths::check_path(&self.root)?;let preferences:Preferences=serde_json::from_value(value.clone()).map_err(|_|"更新设置无效")?;crate::paths::check_path(&self.root.join("preferences.json"))?;atomic_json(&self.root.join("preferences.json"),&json!(preferences))?;self.lock().preferences=preferences;Ok(self.snapshot())}
 pub fn check(&self)->Result<Value>{let guard=self.enter("checking")?;let result=self.check_inner();let download=result.is_ok()&&{let state=self.lock();state.preferences.auto_download&&state.state=="available"&&state.manifest.is_some()};let snapshot=self.finish(result);drop(guard);if download{self.download()}else{snapshot}}
 fn check_inner(&self)->Result<()>{
  crate::paths::check_path(&self.root)?;let client=client()?;self.cancelled()?;
  let cache_path=self.root.join("release-cache.json");crate::paths::check_path(&cache_path)?;crate::paths::check_path(&self.root.join("status.json"))?;crate::paths::check_path(&self.root.join("verified-manifest.json"))?;let cache=read_json(&cache_path).ok();
  let mut request=client.get(API).header("Accept","application/vnd.github+json");
  if let Some(etag)=cache.as_ref().and_then(|cache|cache["etag"].as_str()){request=request.header("If-None-Match",etag);}
  let response=self.send(request)?;
  if response.status()==reqwest::StatusCode::FORBIDDEN||response.status()==reqwest::StatusCode::TOO_MANY_REQUESTS{
   return self.check_fallback(&client).map_err(|error|format!("GitHub 检查受限且签名更新清单不可用，可稍后重试或访问官方发布页。{error}"));
  }
  let etag=response.headers().get(reqwest::header::ETAG).and_then(|value|value.to_str().ok()).map(str::to_owned);
  let release=if response.status()==reqwest::StatusCode::NOT_MODIFIED{cache.as_ref().map(|value|value["release"].clone()).filter(|value|value.is_object()).ok_or("更新缓存不可用")?}else{serde_json::from_slice::<Value>(&bounded(response,1024*1024)?).map_err(|_|"发布信息无效")?};
  if release["draft"]!=false||release["prerelease"]!=false{return Err("发布信息不是稳定版本".into());}
  let tag=release["tag_name"].as_str().ok_or("发布版本缺失")?;let latest=semver::Version::parse(tag.trim_start_matches('v')).map_err(|_|"发布版本无效")?;let current=semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_|"当前版本无效")?;
  if !latest.pre.is_empty(){return Err("不支持预发布版本".into());}
  {let mut state=self.lock();state.last_checked=Some(crate::now());state.latest_version=Some(latest.to_string());state.release_notes=release["body"].as_str().unwrap_or("").chars().take(20000).collect();state.release_url=format!("https://github.com/{REPO}/releases/tag/v{latest}");}
  atomic_json(&self.root.join("status.json"),&json!({"last_checked":crate::now()}))?;
  if latest<=current{let mut state=self.lock();state.state="up_to_date".into();state.manifest=None;state.verified=None;return Ok(());}
  let assets=release["assets"].as_array().ok_or("发布文件缺失")?;
  let asset=|name:&str|->Result<String>{let matches=assets.iter().filter(|asset|asset["name"]==name).collect::<Vec<_>>();if matches.len()!=1{return Err("更新清单文件缺失或重复".into());}let url=matches[0]["browser_download_url"].as_str().filter(|url|release_url(url)).ok_or("更新清单地址无效")?;Ok(url.into())};
  let bytes=bounded(self.send(client.get(asset("update-manifest.json")?))?,MAX_MANIFEST)?;self.cancelled()?;
  let signature=bounded(self.send(client.get(asset("update-manifest.json.sig")?))?,1024)?;
  let signature=std::str::from_utf8(&signature).map_err(|_|"更新签名无效")?;
  let manifest=verify_manifest_with_key(&bytes,signature,self.key.as_deref().ok_or("缺少更新公钥")?,env!("CARGO_PKG_VERSION"))?;
  if manifest.version!=latest.to_string(){return Err("签名更新版本与发布版本不匹配".into());}
  self.cancelled()?;atomic_json(&cache_path,&json!({"etag":etag.or_else(||cache.as_ref().and_then(|value|value["etag"].as_str()).map(str::to_owned)),"release":release}))?;
  atomic_json(&self.root.join("verified-manifest.json"),&json!({"manifest_base64":STANDARD.encode(&bytes),"signature":signature}))?;
  let ready=self.cached_stage(&manifest,&bytes,signature.as_bytes()).ok();
  let mut state=self.lock();state.total_bytes=manifest.size;state.manifest_bytes=bytes;state.signature_bytes=signature.as_bytes().to_vec();state.manifest=Some(manifest);if let Some(ready)=ready{state.downloaded_bytes=ready.manifest.size;state.verified=Some(ready);state.state="ready".into();}else{state.verified=None;state.state="available".into();}Ok(())
 }
 fn check_fallback(&self,client:&reqwest::blocking::Client)->Result<()>{
  let base=format!("https://github.com/{REPO}/releases/latest/download/");
  let bytes=bounded(self.send(client.get(format!("{base}update-manifest.json")))?,MAX_MANIFEST)?;self.cancelled()?;
  let signature=bounded(self.send(client.get(format!("{base}update-manifest.json.sig")))?,1024)?;
  self.accept_fallback(&bytes,&signature)
 }
 fn accept_fallback(&self,bytes:&[u8],signature:&[u8])->Result<()>{
  let signature_text=std::str::from_utf8(signature).map_err(|_|"更新签名无效")?;
  let key=self.key.as_deref().ok_or("缺少更新公钥")?;let manifest=verify_signed_manifest(bytes,signature_text,key)?;
  let version=semver::Version::parse(&manifest.version).map_err(|_|"签名版本无效")?;let current=semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|_|"当前版本无效")?;
  if version>current{verify_manifest_with_key(bytes,signature_text,key,env!("CARGO_PKG_VERSION"))?;}
  self.cancelled()?;let checked=crate::now();crate::paths::check_path(&self.root.join("status.json"))?;atomic_json(&self.root.join("status.json"),&json!({"last_checked":checked}))?;
  if version<=current{let mut state=self.lock();state.last_checked=Some(checked);state.latest_version=Some(manifest.version.clone());state.release_url=format!("https://github.com/{REPO}/releases/tag/v{}",manifest.version);state.release_notes.clear();state.manifest=None;state.verified=None;state.state="up_to_date".into();return Ok(());}
  let ready=self.cached_stage(&manifest,bytes,signature).ok();let mut state=self.lock();state.last_checked=Some(checked);state.latest_version=Some(manifest.version.clone());state.release_url=format!("https://github.com/{REPO}/releases/tag/v{}",manifest.version);state.release_notes="GitHub API 受限；已通过官方发布文件的签名清单检查更新。".into();state.total_bytes=manifest.size;state.manifest=Some(manifest);state.manifest_bytes=bytes.to_vec();state.signature_bytes=signature.to_vec();state.verified=ready;state.state=if state.verified.is_some(){"ready"}else{"available"}.into();Ok(())
 }
 pub fn download(&self)->Result<Value>{let _guard=self.enter("downloading")?;self.finish(self.download_inner())}
 fn download_inner(&self)->Result<()>{
  let manifest=self.lock().manifest.clone().ok_or("请先检查可用更新")?;
  let (bytes,signature)={let state=self.lock();(state.manifest_bytes.clone(),state.signature_bytes.clone())};
  if let Ok(stage)=self.cached_stage(&manifest,&bytes,&signature){let mut state=self.lock();state.downloaded_bytes=manifest.size;state.verified=Some(stage);state.state="ready".into();return Ok(());}
  crate::paths::check_path(&self.root)?;fs::create_dir_all(&self.root).map_err(|_|"更新缓存目录不可写")?;
  let partial=self.root.join(format!("download-{}.part",uuid::Uuid::new_v4().simple()));
  let result=(||{let mut response=self.send(client()?.get(&manifest.url))?;if !response.status().is_success(){return Err("更新文件下载失败".into());}if response.content_length().is_some_and(|length|length!=manifest.size){return Err("更新下载大小与签名不匹配".into());}
   let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&partial).map_err(|_|"更新缓存文件不可写")?;let mut digest=Sha256::new();let mut total=0;let mut buffer=[0u8;65536];
   loop{self.cancelled()?;let count=response.read(&mut buffer).map_err(|_|"更新下载中断")?;if count==0{break;}total+=count as u64;if total>manifest.size{return Err("更新下载超过签名大小".into());}file.write_all(&buffer[..count]).map_err(|_|"更新缓存保存失败")?;digest.update(&buffer[..count]);self.lock().downloaded_bytes=total;}
   if total!=manifest.size||format!("{:x}",digest.finalize())!=manifest.sha256.to_lowercase(){return Err("更新文件校验失败，未安装".into());}file.sync_all().map_err(|_|"更新文件落盘失败")?;drop(file);self.cancelled()?;
   let archive=self.archive_path(&manifest);crate::paths::check_path(&archive)?;
   if archive.exists(){fs::remove_file(&archive).map_err(|_|"旧更新缓存无法替换")?;}fs::rename(&partial,&archive).map_err(|_|"更新文件提交失败")?;
   let (manifest_bytes,signature_bytes)={let state=self.lock();(state.manifest_bytes.clone(),state.signature_bytes.clone())};
   let manifest_path=archive.with_extension("manifest.json");let signature_path=archive.with_extension("manifest.sig");
   crate::paths::check_path(&manifest_path)?;crate::paths::check_path(&signature_path)?;
   floatboard_client::files::atomic_bytes(&manifest_path,&manifest_bytes)?;floatboard_client::files::atomic_bytes(&signature_path,&signature_bytes)?;
   crate::paths::check_path(&self.root.join("ready-manifest.json"))?;crate::paths::check_path(&self.root.join("ready-manifest.sig"))?;
   floatboard_client::files::atomic_bytes(&self.root.join("ready-manifest.json"),&manifest_bytes)?;floatboard_client::files::atomic_bytes(&self.root.join("ready-manifest.sig"),&signature_bytes)?;
   self.prune_cache(&archive)?;
   let mut state=self.lock();state.verified=Some(VerifiedUpdate{manifest,archive,manifest_bytes,signature_bytes,manifest_path,signature_path});state.state="ready".into();Ok(())})();let _=fs::remove_file(&partial);result
 }
 fn prune_cache(&self,keep:&std::path::Path)->Result<()>{
  let keep=keep.file_stem().ok_or("缓存路径无效")?.to_string_lossy();
  for entry in fs::read_dir(&self.root).map_err(|_|"更新缓存目录不可读")?{let entry=entry.map_err(|_|"更新缓存不可读")?;let name=entry.file_name();let name=name.to_string_lossy();
   let stem=name.strip_suffix(".zip").or_else(||name.strip_suffix(".manifest.json")).or_else(||name.strip_suffix(".manifest.sig"));
   let Some(stem)=stem else{continue;};if stem==keep{continue;}
   let Some((version,hash))=stem.rsplit_once('-')else{continue;};
   if semver::Version::parse(version).is_ok()&&hash.len()==64&&hash.bytes().all(|b|b.is_ascii_hexdigit()){
    let path=entry.path();if path.parent()!=Some(self.root.as_path()){return Err("缓存清理路径无效".into());}crate::paths::check_path(&path)?;if entry.file_type().map_err(|_|"缓存属性不可读")?.is_file(){fs::remove_file(path).map_err(|_|"旧更新缓存清理失败")?;}
   }
  }Ok(())
 }
 fn archive_path(&self,manifest:&Manifest)->PathBuf{self.root.join(format!("{}-{}.zip",manifest.version,manifest.sha256.to_lowercase()))}
 fn cached_stage(&self,manifest:&Manifest,bytes:&[u8],signature:&[u8])->Result<VerifiedUpdate>{
  let verified=verify_manifest_with_key(bytes,std::str::from_utf8(signature).map_err(|_|"更新签名编码无效")?,self.key.as_deref().ok_or("缺少更新公钥")?,env!("CARGO_PKG_VERSION"))?;
  if &verified!=manifest{return Err("更新缓存清单不匹配".into());}
  let archive=self.archive_path(manifest);crate::paths::check_path(&archive)?;
  if fs::metadata(&archive).map_err(|_|"无已下载缓存")?.len()!=manifest.size{return Err("更新缓存大小无效".into());}
  let mut file=fs::File::open(&archive).map_err(|_|"更新缓存不可读")?;let mut digest=Sha256::new();let mut buffer=[0;65536];loop{let count=file.read(&mut buffer).map_err(|_|"更新缓存不可读")?;if count==0{break;}digest.update(&buffer[..count]);}
  if format!("{:x}",digest.finalize())!=manifest.sha256.to_lowercase(){return Err("更新缓存校验失败".into());}
  let manifest_path=archive.with_extension("manifest.json");let signature_path=archive.with_extension("manifest.sig");
  crate::paths::check_path(&manifest_path)?;crate::paths::check_path(&signature_path)?;
  floatboard_client::files::atomic_bytes(&manifest_path,bytes)?;floatboard_client::files::atomic_bytes(&signature_path,signature)?;
  Ok(VerifiedUpdate{manifest:manifest.clone(),archive,manifest_bytes:bytes.to_vec(),signature_bytes:signature.to_vec(),manifest_path,signature_path})
 }
 fn restore_ready(&self)->Result<()>{
  crate::paths::check_path(&self.root)?;let path=self.root.join("ready-manifest.json");let signature=self.root.join("ready-manifest.sig");crate::paths::check_path(&path)?;crate::paths::check_path(&signature)?;
  let bytes=fs::read(path).map_err(|_|"无已下载更新")?;let signature=fs::read(signature).map_err(|_|"无已下载签名")?;
  let manifest=verify_manifest_with_key(&bytes,std::str::from_utf8(&signature).map_err(|_|"缓存签名无效")?,self.key.as_deref().ok_or("缺少更新公钥")?,env!("CARGO_PKG_VERSION"))?;
  let stage=self.cached_stage(&manifest,&bytes,&signature)?;let mut state=self.lock();state.latest_version=Some(manifest.version.clone());state.total_bytes=manifest.size;state.downloaded_bytes=manifest.size;state.manifest=Some(manifest);state.manifest_bytes=bytes;state.signature_bytes=signature;state.verified=Some(stage);state.state="ready".into();Ok(())
 }
 pub fn verified_stage(&self)->Result<VerifiedUpdate>{let stage={let state=self.lock();let stage=state.verified.clone().ok_or("更新文件尚未校验完成")?;if state.manifest.as_ref()!=Some(&stage.manifest){return Err("下载版本与当前更新不一致".into());}stage};
 crate::paths::check_path(&stage.manifest_path)?;crate::paths::check_path(&stage.signature_path)?;
 let manifest_bytes=fs::read(&stage.manifest_path).map_err(|_|"更新清单不可读")?;let signature_bytes=fs::read(&stage.signature_path).map_err(|_|"更新签名不可读")?;
 let verified=verify_manifest_with_key(&manifest_bytes,std::str::from_utf8(&signature_bytes).map_err(|_|"更新签名编码无效")?,self.key.as_deref().ok_or("缺少更新公钥")?,env!("CARGO_PKG_VERSION"))?;if verified!=stage.manifest||manifest_bytes!=stage.manifest_bytes||signature_bytes!=stage.signature_bytes{return Err("更新签名清单被修改".into());}
 crate::paths::check_path(&stage.archive)?;let meta=fs::metadata(&stage.archive).map_err(|_|"更新文件不可读")?;if meta.len()!=stage.manifest.size{return Err("更新文件被修改".into());}let mut file=fs::File::open(&stage.archive).map_err(|_|"更新文件不可读")?;let mut digest=Sha256::new();let mut buffer=[0u8;65536];loop{let count=file.read(&mut buffer).map_err(|_|"更新文件校验失败")?;if count==0{break;}digest.update(&buffer[..count]);}if format!("{:x}",digest.finalize())!=stage.manifest.sha256.to_lowercase(){return Err("更新文件被修改".into());}Ok(stage)}
 pub fn begin_install(&self)->Result<VerifiedUpdate>{let _guard=self.enter("installing")?;match self.verified_stage(){Ok(stage)=>Ok(stage),Err(error)=>{self.install_failed(&error);Err(error)}}}
 pub fn install_failed(&self,error:&str){let mut state=self.lock();state.state="error".into();state.error=Some(error.chars().take(1000).collect());}
}
#[cfg(test)]
mod cache_tests{
 use super::*;use ed25519_dalek::{Signer,SigningKey};
 #[test]fn fallback_authenticates_same_version_and_never_skips_new_version_minimum(){
  let root=std::env::temp_dir().join(format!("floatboard-update-fallback-{}",uuid::Uuid::new_v4().simple()));let key=SigningKey::from_bytes(&[94;32]);let mut service=UpdateService::new(root.clone());Arc::get_mut(&mut service).unwrap().key=Some(STANDARD.encode(key.verifying_key().to_bytes()));
  let mut manifest=Manifest{schema:2,version:env!("CARGO_PKG_VERSION").into(),platform:"windows-x86_64".into(),minimum_version:env!("CARGO_PKG_VERSION").into(),updater_protocol:2,database_schema_min:2,database_schema_max:2,agent_protocol:2,url:format!("https://github.com/{REPO}/releases/download/v{}/archive.zip",env!("CARGO_PKG_VERSION")),size:1,sha256:"ab".repeat(32)};
  let sign=|manifest:&Manifest|{let bytes=serde_json::to_vec(manifest).unwrap();let signature=STANDARD.encode(key.sign(&bytes).to_bytes()).into_bytes();(bytes,signature)};
  let(bytes,sig)=sign(&manifest);service.accept_fallback(&bytes,&sig).unwrap();assert_eq!(service.snapshot()["state"],"up_to_date");assert!(verify_manifest_with_key(&bytes,std::str::from_utf8(&sig).unwrap(),service.key.as_ref().unwrap(),env!("CARGO_PKG_VERSION")).is_err());
  let mut corrupt=bytes.clone();corrupt.push(b' ');assert!(service.accept_fallback(&corrupt,&sig).is_err());
  manifest.version="999.0.0".into();manifest.minimum_version="998.0.0".into();manifest.url=format!("https://github.com/{REPO}/releases/download/v999.0.0/archive.zip");let(bytes,sig)=sign(&manifest);assert!(service.accept_fallback(&bytes,&sig).is_err());
  manifest.minimum_version=env!("CARGO_PKG_VERSION").into();let(bytes,sig)=sign(&manifest);service.accept_fallback(&bytes,&sig).unwrap();assert_eq!(service.snapshot()["state"],"available");
  fs::remove_dir_all(root).unwrap();
 }
 #[test]fn ready_cache_reuses_download_restores_after_restart_and_preserves_offline_install(){
  let root=std::env::temp_dir().join(format!("floatboard-update-cache-{}",uuid::Uuid::new_v4().simple()));
  let key=SigningKey::from_bytes(&[93;32]);let public=STANDARD.encode(key.verifying_key().to_bytes());
  let mut service=UpdateService::new(root.clone());Arc::get_mut(&mut service).unwrap().key=Some(public.clone());
  fs::create_dir_all(&service.root).unwrap();let contents=b"fixture archive no network";
  let manifest=Manifest{schema:2,version:"999.0.0".into(),platform:"windows-x86_64".into(),minimum_version:"0.8.0".into(),updater_protocol:2,database_schema_min:2,database_schema_max:2,agent_protocol:2,url:"https://github.com/Memento1990/codex-task/releases/download/v999.0.0/archive.zip".into(),size:contents.len() as u64,sha256:format!("{:x}",Sha256::digest(contents))};
  let bytes=serde_json::to_vec(&manifest).unwrap();let signature=STANDARD.encode(key.sign(&bytes).to_bytes()).into_bytes();fs::write(service.archive_path(&manifest),contents).unwrap();
  {let mut state=service.lock();state.manifest=Some(manifest.clone());state.manifest_bytes=bytes.clone();state.signature_bytes=signature.clone();}
  service.download().unwrap();assert_eq!(service.snapshot()["state"],"ready");let archive=service.verified_stage().unwrap().archive;
  service.download().unwrap();assert_eq!(service.verified_stage().unwrap().archive,archive);
  service.finish(Err("offline".into())).unwrap_err();assert_eq!(service.snapshot()["state"],"ready");assert_eq!(service.snapshot()["error"],"offline");assert!(service.verified_stage().is_ok());
  floatboard_client::files::atomic_bytes(&service.root.join("ready-manifest.json"),&bytes).unwrap();floatboard_client::files::atomic_bytes(&service.root.join("ready-manifest.sig"),&signature).unwrap();
  let mut restored=UpdateService::new(root.clone());Arc::get_mut(&mut restored).unwrap().key=Some(public);restored.restore_ready().unwrap();assert_eq!(restored.snapshot()["state"],"ready");assert_eq!(restored.verified_stage().unwrap().archive,archive);
  restored.lock().manifest.as_mut().unwrap().version="1000.0.0".into();assert!(restored.verified_stage().is_err());
  fs::remove_dir_all(root).unwrap();
 }
}
