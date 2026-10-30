# Codex Task · 应用源码

当前产品版本为 **0.7.0**，Windows 桌面实现使用 Tauri 2 + Svelte + Rust。

- [项目介绍、截图、下载与接入](../README.md)
- [构建与验证](../README.md#源码构建)
- [当前版本说明](../docs/releases/v0.7.0.md)
- [公开发行验证](../docs/releases/v0.7.0-validation.md)
- [历史变更](CHANGELOG.md)

## 源码边界

`desktop/src` 是界面，`desktop/src-tauri` 是原生宿主，`desktop/reporter` 是 Hook / MCP、采集和存储核心。

旧 Python / Qt 文件与 `tests` 保留作迁移对照，不参与 0.7.0 产品运行。部分历史人工脚本包含作者本机的环境路径，不属于通用自动测试入口。

本机 `docs/evidence`、`build`、`dist`、`releases`、数据库、配置备份及缓存不进入 Git；公开的演示截图和验证摘要放在仓库根目录 `docs`。

作者本机升级验收发生在 2026-10-30，GitHub 首次公开发行日期为 2026-10-31。两者版本同为 0.7.0；本次不改变应用功能或已有数据协议。
