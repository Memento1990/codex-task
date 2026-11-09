use std::{fs,path::{Path,PathBuf,Component}};
use crate::Result;
pub fn control_root(root:&Path)->PathBuf{root.join("control")}
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
