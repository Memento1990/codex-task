//! Stable per-user Rust entry point. Portable test roots never touch the user's install.
use std::{fs,path::{Path,PathBuf},process::{Command,Stdio}};
use std::os::windows::process::CommandExt;
use serde_json::{json,Value};
use crate::{Result,store::{atomic_bytes,atomic_json,read_json,hash},runtime::ProcessLock};

fn identity(path:&Path)->String{path.to_string_lossy().trim_start_matches(r"\\?\").trim_end_matches(['\\','/']).replace('/',"\\").to_lowercase()}
fn default_root()->PathBuf{PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("CodexFloatboard/data")}
pub fn managed(root:&Path)->bool{
    let initial=default_root();
    identity(root)==identity(&initial)||crate::migration::resolve_root(&initial).is_ok_and(|p|identity(root)==identity(&p))
}
pub fn control_root(root:&Path)->PathBuf{if managed(root){default_root().parent().unwrap().join("control")}else{root.to_path_buf()}}
pub fn receipt_path(root:&Path)->PathBuf{control_root(root).join("integration.json")}
fn install_root(root:&Path)->PathBuf{control_root(root).join("runtime")}
pub fn integration_reporter(root:&Path)->Result<PathBuf>{
    if managed(root){Ok(install_root(root).join("FloatboardBridge.exe"))}else{crate::runtime::reporter_path()}
}
/// Install only on explicit connection. A stable binary is copied once; payloads
/// are content-addressed and never overwritten while a prior client uses them.
pub fn install(root:&Path)->Result<()> {
    if !managed(root){return Ok(());}
    let base=install_root(root);crate::migration::check_path(&base)?;
    fs::create_dir_all(&base).map_err(|_|"连接组件目录不可写")?;
    let _lock=ProcessLock::acquire(&base.join("install.lock"))?;
    let source=crate::runtime::reporter_path()?;
    let bytes=fs::read(&source).map_err(|_|"缺少同目录 Reporter，请完整解压新版看板")?;
    let digest=hash(&bytes);
    let relative=format!("versions/{}/{digest}/FloatboardReporter.exe",env!("CARGO_PKG_VERSION"));
    let payload=base.join(&relative);crate::migration::check_path(&payload)?;
    if payload.exists(){if hash(&fs::read(&payload).map_err(|_|"连接运行时不可读")?)!=digest{return Err("已安装连接组件校验失败".into());}}
    else{atomic_bytes(&payload,&bytes)?;}
    let stable=base.join("FloatboardBridge.exe");let launcher_meta=base.join("launcher.json");
    if stable.exists(){let metadata=read_json(&launcher_meta).map_err(|_|"固定连接入口来源无法确认，请在高级选项检查；未覆盖同名文件")?;if metadata["sha256"]!=hash(&fs::read(&stable).map_err(|_|"连接入口不可读")?){return Err("固定连接入口校验失败，未覆盖文件".into());}}
    else{atomic_bytes(&stable,&bytes)?;atomic_json(&launcher_meta,&json!({"schema":1,"sha256":digest}))?;}
    // Never silently downgrade a newer registered runtime from an old GUI.
    let active=base.join("active.json");
    if active.exists(){let old=read_json(&active)?;
        if old["schema"]!=1{return Err("现有运行时记录损坏或不兼容，未覆盖".into());}
        if version(old["version"].as_str().unwrap_or(""))>version(env!("CARGO_PKG_VERSION")){return Err("已安装更新版本的连接组件，请使用新版看板；未降级接入".into());}
    }
    atomic_json(&base.join("pending-runtime.json"),&json!({"schema":1,"version":env!("CARGO_PKG_VERSION"),"payload":relative,"sha256":digest,"data_root":default_root()}))
}
pub fn activation(root:&Path)->Result<Option<(PathBuf,Option<Value>,Value)>>{
    if !managed(root){return Ok(None);}
    let base=install_root(root);let path=base.join("active.json");
    let after=read_json(&base.join("pending-runtime.json"))?;
    let before=if path.exists(){Some(read_json(&path)?)}else{None};
    Ok(Some((path,before,after)))
}
fn version(value:&str)->Vec<u64>{value.split('.').map(|v|v.parse().unwrap_or(0)).collect()}
pub fn is_bridge()->bool{std::env::current_exe().ok().and_then(|p|p.file_name().map(|n|n.to_string_lossy().eq_ignore_ascii_case("FloatboardBridge.exe"))).unwrap_or(false)}
/// Launcher passes stdin/stdout through unchanged; it never emits diagnostics on
/// protocol stdout. Metadata controls only verified descendants of this install.
pub fn run()->Result<i32>{
    let exe=std::env::current_exe().map_err(|_|"连接入口不可用")?;
    let base=exe.parent().ok_or("连接入口不可用")?;
    crate::migration::check_path(base)?;
    let active=read_json(&base.join("active.json"))?;
    if active["schema"]!=1{return Err("连接组件版本不兼容".into());}
    let relative=active["payload"].as_str().ok_or("连接组件记录无效")?;
    let relative=Path::new(relative);
    if relative.is_absolute()||relative.components().any(|c|!matches!(c,std::path::Component::Normal(_))){return Err("连接组件路径无效".into());}
    let payload=base.join(relative);crate::migration::check_path(&payload)?;
    let bytes=fs::read(&payload).map_err(|_|"连接运行时缺失，请在看板中修复连接")?;
    if hash(&bytes)!=active["sha256"].as_str().unwrap_or(""){return Err("连接运行时校验失败".into());}
    let selected=PathBuf::from(active["data_root"].as_str().ok_or("连接数据位置无效")?);
    let root=crate::migration::resolve_root(&selected)?;
    let args:Vec<String>=std::env::args().skip(1).collect();
    // The stable config owns data selection; callers cannot redirect this bridge.
    if args.iter().any(|v|v=="--data-dir"){return Err("固定连接入口不接受外部数据目录".into());}
    let inherited_generation=std::env::var("FLOATBOARD_CONNECTION_GENERATION").unwrap_or_else(|_|generation(&root));
    let status=Command::new(payload).args(args).arg("--data-dir").arg(&root).env("FLOATBOARD_CONNECTION_GENERATION",inherited_generation).stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit()).creation_flags(0x08000000).status().map_err(|_|"连接运行时无法启动")?;
    Ok(status.code().unwrap_or(1))
}
/// Current generation scopes evidence to the latest actual config application.
pub fn generation(root:&Path)->String{read_json(&receipt_path(root)).ok().and_then(|v|v["generation"].as_str().or_else(||v["after_hash"].as_str()).map(str::to_owned)).unwrap_or_default()}
static PROCESS_GENERATION:std::sync::OnceLock<String>=std::sync::OnceLock::new();
pub fn process_generation(root:&Path)->String{PROCESS_GENERATION.get_or_init(||std::env::var("FLOATBOARD_CONNECTION_GENERATION").unwrap_or_else(|_|generation(root))).clone()}
pub fn observe(root:&Path,kind:&str,session:Option<&str>)->Result<()> {observe_event(root,kind,session,None)}
pub fn observe_event(root:&Path,kind:&str,session:Option<&str>,event_id:Option<&str>)->Result<()> {
    if std::env::var("FLOATBOARD_PREFLIGHT").as_deref()==Ok("1"){return Ok(());}
    let generation=if std::env::current_exe().ok().is_some_and(|p|p.file_name().is_some_and(|n|n.to_string_lossy().eq_ignore_ascii_case("FloatboardReporter.exe"))){process_generation(root)}else{generation(root)};
    // One atomic evidence write avoids cross-session read/modify/write races.
    atomic_json(&root.join(format!("connection-{kind}.json")),&json!({"at":crate::now(),"version":env!("CARGO_PKG_VERSION"),"generation":generation,"session":session.map(|s|hash(s.as_bytes())),"event_id":event_id}))
}
pub fn decorate_connection(root:&Path,mut state:Value)->Value{
    let config=state["state"].as_str().unwrap_or("error").to_owned();state["config_state"]=json!(config);
    if state["recovery_required"]==true{state["state"]=json!("recovery_required");return state;}
    if config=="error"{state["state"]=json!("conflict");return state;}
    if config=="missing"{state["state"]=json!("missing");return state;}
    if state["hook_error"]==true {state["state"]=json!("error");return state;}
    let receipt=read_json(&receipt_path(root)).unwrap_or(json!({}));
    let generation=generation(root);let since=receipt["connected_at"].as_f64().unwrap_or(f64::INFINITY);
    let fresh=|kind:&str|->Option<Value>{read_json(&root.join(format!("connection-{kind}.json"))).ok().filter(|v|!generation.is_empty()&&v["generation"]==generation&&v["version"]==env!("CARGO_PKG_VERSION")&&v["at"].as_f64().is_some_and(|at|at>=since&&at<=crate::now()+5.))};
    let mcp=fresh("mcp");let hook=fresh("hook");let report=fresh("report");let consumed=fresh("consumed");
    state["mcp_connected"]=json!(mcp.is_some());state["hook_seen"]=json!(hook.is_some());
    state["diagnostics"]=json!(if state["upgrade_available"]==true{vec!["发现旧版连接，可在高级选项升级；不会结束当前 Codex 会话。"]}else if hook.is_some(){vec!["已收到当前连接的会话事件；任务汇报入库后才完成同步验证。"]}else if mcp.is_some(){vec!["当前连接组件已由 Codex 初始化，等待真实会话事件。"]}else{vec!["配置就绪不代表 Codex 已载入。请按原生提示确认信任，方便时重载或完整重启 Codex。"]});
    let persisted=report.as_ref().and_then(|r|r["event_id"].as_str()).is_some_and(|id|{
        rusqlite::Connection::open_with_flags(root.join("events.sqlite3"),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok().is_some_and(|db|db.query_row("select exists(select 1 from events where id=?1 and kind='progress')",[id],|row|row.get::<_,bool>(0)).unwrap_or(false))
    });
    let verified=hook.as_ref().zip(report.as_ref()).is_some_and(|(h,r)|h["session"]==r["session"]&&(persisted||consumed.as_ref().is_some_and(|c|r["session"]==c["session"]&&r["event_id"].is_string()&&r["event_id"]==c["event_id"])));
    state["state"]=json!(if state["upgrade_available"]==true||config=="partial"{"awaiting_reload"}else if verified&&mcp.is_some(){"sync_verified"}else if hook.is_some(){"session_received"}else if mcp.is_some(){"awaiting_session"}else{"awaiting_reload"});
    state
}
