//! Local OAuth authorization code + S256 PKCE. Only authenticated IPC can consent.
use std::{collections::BTreeMap,path::PathBuf,sync::{Arc,Mutex}};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use axum::{extract::{State,Query,Form},Json,response::{Response,IntoResponse,Html,Redirect},http::StatusCode};
use base64::{Engine as _,engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest,Sha256};
use floatboard_client::{Profile,Result,files::{atomic_bytes,crypt}};
use floatboard_protocol::{hash,now,secret_eq};

fn random()->String{use rand::RngCore;let mut bytes=[0u8;32];rand::rngs::OsRng.fill_bytes(&mut bytes);URL_SAFE_NO_PAD.encode(bytes)}
#[derive(Clone,Serialize,Deserialize)]
struct Grant{client:String,refresh:String,refresh_expires:f64,access:String,access_expires:f64}
#[derive(Default,Clone,Serialize,Deserialize)]
struct Saved{profile_id:String,clients:BTreeMap<String,Vec<String>>,grants:Vec<Grant>}
struct Pending{client:String,redirect:String,challenge:String,state:String,expires:f64,code:Option<String>}
pub struct Auth{pub profile:Profile,path:PathBuf,saved:Saved,pending:BTreeMap<String,Pending>,pub authenticated_requests:u64}
pub type Shared=Arc<Mutex<Auth>>;
impl Auth {
    pub fn load(profile:Profile)->Result<Shared>{
        let path=profile.root.join("oauth.dpapi");let saved:Saved=if path.exists(){let bytes=std::fs::read(&path).map_err(|_|"OAuth凭据不可读")?;if bytes.len()>1_000_000{return Err("OAuth凭据过大".into());}serde_json::from_slice(&crypt(&bytes,false)?).map_err(|_|"OAuth凭据损坏")?}else{Saved{profile_id:profile.profile_id.clone(),..Default::default()}};
        if saved.profile_id!=profile.profile_id{return Err("OAuth凭据不属于此profile".into());}
        Ok(Arc::new(Mutex::new(Self{profile,path,saved,pending:BTreeMap::new(),authenticated_requests:0})))
    }
    pub fn authorized_clients(&self)->usize{self.saved.grants.iter().filter(|g|g.refresh_expires>now()).count()}
    fn save(&self)->Result<()>{atomic_bytes(&self.path,&crypt(&serde_json::to_vec(&self.saved).map_err(|_|"OAuth编码失败")?,true)?)}
    pub fn valid(&self,token:&str)->bool{let digest=hash(token.as_bytes());self.saved.grants.iter().any(|g|g.access_expires>now()&&secret_eq(&g.access,&digest))}
    pub fn call(&mut self,method:&str,params:&Value)->Result<Value>{
        self.pending.retain(|_,p|p.expires>now());
        match method{
            "pairing_list"=>Ok(json!(self.pending.iter().map(|(id,p)|json!({"id":id,"client_id":p.client,"redirect_uri":p.redirect,"expires_at":p.expires,"approved":p.code.is_some()})).collect::<Vec<_>>())),
            "pairing_approve"=>{if params["confirmed"]!=true{return Err("请确认本次配对".into());}let id=params["id"].as_str().ok_or("缺少配对ID")?;let pending=self.pending.get_mut(id).ok_or("配对已过期")?;pending.code=Some(random());Ok(json!({"approved":true}))},
            "pairing_revoke"=>{if params["confirmed"]!=true{return Err("撤销授权需要确认".into());}let before=self.saved.clone();self.saved.grants.clear();self.pending.clear();if let Err(e)=self.save(){self.saved=before;return Err(e);}Ok(json!({"revoked":true}))},
            _=>Err("未知配对操作".into())
        }
    }
}
fn bad(code:&str)->Response{(StatusCode::BAD_REQUEST,Json(json!({"error":code}))).into_response()}
fn loopback_redirect(value:&str)->bool{url::Url::parse(value).is_ok_and(|u|u.scheme()=="http"&&matches!(u.host_str(),Some("127.0.0.1"|"localhost"|"[::1]"))&&u.port().is_some()&&u.username().is_empty()&&u.password().is_none()&&u.fragment().is_none())}
pub async fn metadata(State(auth):State<Shared>)->Json<Value>{let origin=auth.lock().unwrap().profile.origin();Json(json!({"issuer":origin,"authorization_endpoint":format!("{origin}/authorize"),"token_endpoint":format!("{origin}/token"),"registration_endpoint":format!("{origin}/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none"],"code_challenge_methods_supported":["S256"],"scopes_supported":["floatboard"]}))}
pub async fn resource(State(auth):State<Shared>)->Json<Value>{let origin=auth.lock().unwrap().profile.origin();Json(json!({"resource":format!("{origin}/mcp"),"authorization_servers":[origin],"scopes_supported":["floatboard"],"bearer_methods_supported":["header"]}))}
pub async fn register(State(auth):State<Shared>,Json(value):Json<Value>)->Response{
    let Some(redirects)=value["redirect_uris"].as_array().filter(|v|!v.is_empty()&&v.len()<=4)else{return bad("invalid_client_metadata")};
    let Some(redirects)=redirects.iter().map(|v|v.as_str().filter(|s|loopback_redirect(s)).map(str::to_owned)).collect::<Option<Vec<_>>>()else{return bad("invalid_redirect_uri")};
    let mut auth=auth.lock().unwrap();if auth.saved.clients.len()>=128{return bad("registration_limit")};let client=random();auth.saved.clients.insert(client.clone(),redirects.clone());
    if auth.save().is_err(){auth.saved.clients.remove(&client);return bad("server_error");}
    (StatusCode::CREATED,Json(json!({"client_id":client,"redirect_uris":redirects,"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]}))).into_response()
}
pub async fn authorize(State(auth):State<Shared>,Query(p):Query<BTreeMap<String,String>>)->Response{
    let mut auth=auth.lock().unwrap();auth.pending.retain(|_,p|p.expires>now());
    let get=|key:&str|p.get(key).map(String::as_str).unwrap_or("");
    let client=get("client_id");let redirect=get("redirect_uri");let challenge=get("code_challenge");
    if get("response_type")!="code"||get("code_challenge_method")!="S256"||challenge.len()!=43||URL_SAFE_NO_PAD.decode(challenge).map(|v|v.len()).ok()!=Some(32)||get("state").len()>2048||!auth.saved.clients.get(client).is_some_and(|v|v.iter().any(|s|s==redirect)){return bad("invalid_request");}
    if p.get("resource").is_some_and(|r|r!=&format!("{}/mcp",auth.profile.origin())){return bad("invalid_target");}
    let id=hash(format!("{client}\0{redirect}\0{challenge}\0{}",get("state")).as_bytes());
    if let Some(pending)=auth.pending.get(&id){if let Some(code)=&pending.code{
        let mut target=url::Url::parse(&pending.redirect).unwrap();target.query_pairs_mut().append_pair("code",code).append_pair("state",&pending.state);
        return Redirect::to(target.as_str()).into_response();
    }}else{
        if auth.pending.len()>=32{return bad("authorization_limit")};
        auth.pending.insert(id,Pending{client:client.into(),redirect:redirect.into(),challenge:challenge.into(),state:get("state").into(),expires:now()+300.,code:None});
    }
    Html("<!doctype html><html lang=zh><meta charset=utf-8><meta http-equiv=refresh content=2><title>Floatboard 配对</title><p>请在任务侧窗的 Codex 接入窗口确认此次配对。确认后本页自动继续。</p></html>").into_response()
}
pub async fn token(State(auth):State<Shared>,Form(p):Form<BTreeMap<String,String>>)->Response{
    let mut auth=auth.lock().unwrap();let get=|key:&str|p.get(key).map(String::as_str).unwrap_or("");let client=get("client_id");
    if p.get("resource").is_some_and(|r|r!=&format!("{}/mcp",auth.profile.origin())){return bad("invalid_target");}
    let before=auth.saved.clone();let refresh_expiry;
    match get("grant_type"){
        "authorization_code"=>{
            let verifier=get("code_verifier");if !(43..=128).contains(&verifier.len())||!verifier.bytes().all(|b|b.is_ascii_alphanumeric()||b"-._~".contains(&b)){return bad("invalid_grant");}
            let challenge=URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            let found=auth.pending.iter().find(|(_,v)|v.expires>now()&&v.client==client&&v.redirect==get("redirect_uri")&&v.code.as_deref().is_some_and(|s|secret_eq(s,get("code")))&&secret_eq(&v.challenge,&challenge)).map(|(id,_)|id.clone());
            let Some(id)=found else{return bad("invalid_grant")};auth.pending.remove(&id);refresh_expiry=now()+30.*86400.;
        },
        "refresh_token"=>{let digest=hash(get("refresh_token").as_bytes());let Some(index)=auth.saved.grants.iter().position(|g|g.client==client&&g.refresh_expires>now()&&secret_eq(&g.refresh,&digest))else{return bad("invalid_grant")};refresh_expiry=auth.saved.grants.remove(index).refresh_expires;},
        _=>return bad("unsupported_grant_type")
    }
    auth.saved.grants.retain(|g|g.refresh_expires>now());if auth.saved.grants.len()>=128{auth.saved=before;return bad("grant_limit");}
    let access=random();let refresh=random();auth.saved.grants.push(Grant{client:client.into(),refresh:hash(refresh.as_bytes()),refresh_expires:refresh_expiry,access:hash(access.as_bytes()),access_expires:now()+900.});
    if auth.save().is_err(){auth.saved=before;return bad("server_error");}
    ([("cache-control","no-store"),("pragma","no-cache")],Json(json!({"access_token":access,"token_type":"Bearer","expires_in":900,"refresh_token":refresh,"scope":"floatboard"}))).into_response()
}
