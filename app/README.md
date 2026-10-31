# Codex Task · 应用源码

当前源码版本为 **0.8.0 候选版（未发布）**，Windows 桌面实现使用 Tauri 2 + Svelte + Rust。

- [项目介绍、截图、下载与接入](../README.md)
- [构建与验证](../README.md#源码构建)
- [当前版本说明](../docs/releases/v0.8.0.md)
- [公开发行验证](../docs/releases/v0.8.0-validation.md)
- [历史变更](CHANGELOG.md)

## 源码边界

`desktop/src` 是界面，`desktop/src-tauri` 是原生宿主，`desktop/reporter` 是 Hook / MCP、采集和存储核心。

旧 Python / Qt 实现及其专属测试、依赖和打包脚本已移除，历史代码可从 Git 历史查阅。`desktop/reporter/tests/fixtures` 中的合成数据库与任务快照用于验证旧数据兼容性，必须保留，不依赖 Python。

`desktop/tests` 包含前端模型测试和原生交互验收片段，使用方式与安全边界见 [测试说明](desktop/tests/README.md)。

本机 `build/local-msvc` 是 `desktop/build-native.ps1` 使用的便携 C++ 工具链，不是旧 Python 构建产物。工具链、`releases`、数据库、配置备份及缓存不进入 Git；公开的演示截图和验证摘要放在仓库根目录 `docs`。

0.8.0 新增自定义标题栏、离线目录迁移和同步诊断；数据库事件格式保持兼容。迁移要求先暂停所有 Codex 生产者并升级接入，不能继续混用旧版 Hook。
