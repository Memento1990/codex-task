//! Local-only, copy-before-switch data migration. Never rewrites Codex configuration.
use std::{fs, path::{Component, Path, PathBuf}, time::{Duration, Instant}};
use serde_json::{json, Value};
use crate::{Result, runtime::{self, ProcessLock}, store::{atomic_json, read_json, Store}};

pub const REDIRECT: &str = "data-redirect.json";
pub const LOCK: &str = "migration.lock";
pub const STOP: &str = "migration-stop.json";
pub const ACK: &str = "migration-paused.json";

/// Reject links/junctions on every existing ancestor, including the selected root.
pub fn check_path(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("数据目录须为不含 .. 的本地绝对路径".into());
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))) {
        return Err("数据目录不支持 UNC 或设备路径".into());
    }
    #[cfg(windows)]
    for component in path.components() {
        if let Component::Normal(name)=component {
            let name=name.to_string_lossy();
            let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
            if name.ends_with(['.', ' ']) || name.contains(':') || ["CON","PRN","AUX","NUL","COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9","LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9"].contains(&stem.as_str()) {
                return Err("数据目录包含不安全的 Windows 路径别名".into());
            }
        }
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)]
                { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return Err("数据目录不允许符号链接或 reparse point".into()); } }
                if meta.file_type().is_symlink() { return Err("数据目录不允许符号链接".into()); }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err("数据目录属性不可读".into()),
        }
    }
    Ok(())
}
fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map_err(|_| "当前目录不可用")?.join(path) };
    check_path(&path)?;
    // Canonicalize the existing ancestor as well as existing roots so Windows
    // short-name aliases cannot bypass the same/nested-directory checks.
    let mut ancestor=path.as_path();let mut tail=Vec::new();
    while !ancestor.exists() {
        tail.push(ancestor.file_name().ok_or("数据目录路径无效")?.to_os_string());
        ancestor=ancestor.parent().ok_or("数据目录路径无效")?;
    }
    let mut canonical=fs::canonicalize(ancestor).map_err(|_| "数据目录规范路径不可读")?;
    for name in tail.into_iter().rev(){canonical.push(name);}
    #[cfg(windows)]
    let canonical=PathBuf::from(canonical.to_string_lossy().trim_start_matches(r"\\?\"));
    check_path(&canonical)?;
    Ok(canonical)
}
fn identity(path: &Path) -> String { path.to_string_lossy().trim_start_matches(r"\\?\").trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase() }
fn related(a: &Path, b: &Path) -> bool {
    let a=identity(a); let b=identity(b);
    a==b || a.starts_with(&(b.clone()+"\\")) || b.starts_with(&(a+"\\"))
}
pub fn resolve_root(root: &Path) -> Result<PathBuf> {
    let mut root=absolute(root)?; let mut seen=std::collections::HashSet::new();
    for _ in 0..16 {
        if !seen.insert(identity(&root)) { return Err("数据目录重定向循环".into()); }
        let redirect=root.join(REDIRECT); check_path(&redirect)?;
        if !redirect.try_exists().map_err(|_| "数据目录重定向不可读")? { return Ok(root); }
        let value=read_json(&redirect)?;
        if value["version"]!=1 { return Err("数据目录重定向版本无效".into()); }
        let next=PathBuf::from(value["path"].as_str().ok_or("数据目录重定向无效")?);
        check_path(&next)?;
        if !next.is_dir() { return Err("迁移后的数据目录不可用，请恢复目录后重启".into()); }
        root=next;
    }
    Err("数据目录重定向过多".into())
}
/// Serialize short-lived producers with migration. Re-resolve after locking to
/// cover a redirect committed between the first resolution and lock acquisition.
pub fn with_root<T>(root: &Path, action: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    let resolved=resolve_root(root)?;
    let deadline=Instant::now()+Duration::from_secs(1);
    let lock=loop {
        if let Ok(lock)=ProcessLock::acquire(&resolved.join(LOCK)){break lock;}
        if Instant::now()>=deadline{return Err("数据目录正在迁移或写入，请稍后重试".into());}
        std::thread::sleep(Duration::from_millis(10));
    };
    let current=resolve_root(root)?;
    if identity(&resolved)!=identity(&current) {
        drop(lock);
        return with_root(&current,action);
    }
    action(&current)
}

fn check_tree(path: &Path) -> Result<()> {
    check_path(path)?;
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| "迁移目录不可读")? {
            check_tree(&entry.map_err(|_| "迁移文件不可读")?.path())?;
        }
    }
    Ok(())
}
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    check_path(from)?; check_path(to)?;
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|_| "新数据目录不可写")?;
        for entry in fs::read_dir(from).map_err(|_| "迁移目录不可读")? {
            let entry=entry.map_err(|_| "迁移文件不可读")?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else if from.is_file() { fs::copy(from,to).map_err(|_| "迁移文件复制失败")?; }
    else { return Err("不支持迁移特殊文件".into()); }
    Ok(())
}
struct StopGuard(PathBuf);
impl Drop for StopGuard { fn drop(&mut self) { let _=fs::remove_file(&self.0); } }

/// Caller closes its Store before calling. Commit point is the atomic redirect;
/// on failure old data stays authoritative and a partial destination is retained.
pub fn migrate(root: &Path, destination: &Path) -> Result<PathBuf> {
    let root=resolve_root(root)?;
    check_path(destination)?;
    let destination=absolute(destination)?;
    if related(&root,&destination) { return Err("新旧目录不能相同或互相嵌套".into()); }
    if destination.exists() && (!destination.is_dir() || fs::read_dir(&destination).map_err(|_| "新目录不可读")?.next().is_some()) {
        return Err("请选择空的新数据目录".into());
    }
    check_tree(&root)?;
    let _migration=ProcessLock::acquire(&root.join(LOCK)).map_err(|_| "数据目录正在迁移，请稍后重试")?;
    // Integration receipts and backups must not change under the copy.
    let _integration=ProcessLock::acquire(&root.join("integration.lock")).map_err(|_| "接入操作正在进行，请稍后迁移")?;
    // Start a cooperative collector even when idle: it must retain the source
    // collector identity continuously across commit and destination activation.
    // A stale STOP is ours to replace because migration.lock is held.
    let _=fs::remove_file(root.join(STOP));
    runtime::spawn_worker(&root)?;
    let nonce=uuid::Uuid::new_v4().to_string();
    let stop=root.join(STOP); atomic_json(&stop,&json!({"version":1,"nonce":nonce,"at":crate::now()}))?;
    let _stop=StopGuard(stop);
    let deadline=Instant::now()+Duration::from_secs(5);
    loop {
        if read_json(&root.join(ACK)).is_ok_and(|ack|ack["nonce"]==nonce) {break;}
        if Instant::now()>=deadline { return Err("采集器未安全停止（可能是旧版本）；请退出旧版本后重试，未迁移数据".into()); }
        std::thread::sleep(Duration::from_millis(40));
    };
    // Drain with exclusive collector ownership. VACUUM INTO is a SQLite
    // consistent snapshot, not a live copy of a WAL database.
    let mut store=Store::open(&root,false)?;
    while runtime::drain(&root,&mut store)?.0 > 0 {}
    fs::create_dir_all(&destination).map_err(|_| "新数据目录不可写")?;
    // Recheck after locks: never traverse a pre-existing reparse tree.
    check_tree(&root)?; check_tree(&destination)?;
    if fs::read_dir(&destination).map_err(|_| "新目录不可读")?.next().is_some() { return Err("新数据目录已被占用".into()); }
    store.db.execute("VACUUM INTO ?1", [destination.join("events.sqlite3").to_str().ok_or("目录编码无效")?]).map_err(|_| "任务库一致性快照失败，未切换目录")?;
    drop(store);
    for entry in fs::read_dir(&root).map_err(|_| "旧数据目录不可读")? {
        let entry=entry.map_err(|_| "迁移文件不可读")?; let name=entry.file_name(); let name_text=name.to_string_lossy();
        if name_text.starts_with("events.sqlite3") || name_text.ends_with(".lock") || name_text.starts_with(".floatboard-") || [REDIRECT,STOP,ACK,"health.json","migration-origins.json"].contains(&name_text.as_ref()) { continue; }
        copy_tree(&entry.path(),&destination.join(&name))?;
    }
    // Old long-lived clients may have resolved the old root before commit.
    // The replacement worker also drains these retained legacy inboxes.
    let mut origins=read_json(&root.join("migration-origins.json")).ok().and_then(|v|v["roots"].as_array().cloned()).unwrap_or_default();
    origins.push(json!(root));
    atomic_json(&destination.join("migration-origins.json"), &json!({"roots":origins}))?;
    let verify=Store::open(&destination,true)?;
    let valid:String=verify.db.query_row("PRAGMA quick_check",[],|row|row.get(0)).map_err(|_| "新任务库校验失败")?;
    if valid!="ok" { return Err("新任务库校验失败，未切换目录".into()); } drop(verify);
    atomic_json(&root.join(REDIRECT), &json!({"version":1,"path":destination,"at":crate::now()}))?;
    Ok(destination)
}
pub fn location(root: &Path, restart_required: bool) -> Value {
    json!({"current":root,"path":root,"restart_required":restart_required})
}
