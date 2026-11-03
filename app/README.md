# Codex Task · 应用源码

当前源码版本为 **0.9.0**，Windows 桌面实现使用 Tauri 2 + Svelte + Rust。

- [项目介绍、截图、下载与接入](../README.md)
- [构建与验证](../README.md#源码构建)
- [当前版本说明](../docs/releases/v0.9.0.md)
- [公开发行验证](../docs/releases/v0.9.0-validation.md)
- [历史变更](CHANGELOG.md)

## 源码边界

`desktop/src` 是界面，`desktop/src-tauri` 是原生宿主，`desktop/reporter` 是 Hook / MCP、采集和存储核心。

旧 Python / Qt 实现及其专属测试、依赖和打包脚本已移除，历史代码可从 Git 历史查阅。`desktop/reporter/tests/fixtures` 中的合成数据库与任务快照用于验证旧数据兼容性，必须保留，不依赖 Python。

`desktop/tests` 包含前端模型测试和原生交互验收片段，使用方式与安全边界见 [测试说明](desktop/tests/README.md)。

本机 `build/local-msvc` 是 `desktop/build-native.ps1` 使用的便携 C++ 工具链，不是旧 Python 构建产物。工具链、`releases`、数据库、配置备份及缓存不进入 Git；公开的演示截图和验证摘要放在仓库根目录 `docs`。

## 本机产物与编译缓存

- 日常正式安装位于用户LOCALAPPDATA/Programs/CodexFloatboard，仅从GitHub Release安装和更新；仓库根启动脚本指向该固定安装，不再运行releases副本。开发使用 `pnpm dev:isolated`，布局及限制见[开发隔离说明](../docs/development-isolation.md)。旧releases副本仅为历史归档/活动旧Reporter保留，不作为开发入口，不强制结束它们。
- 旧 Python dist、PyInstaller 构建缓存及 `app/releases` 的重复发行目录已清理；`build/local-msvc` 工具链保留，历史原生验收资料归档到仓库外。
- 项目内两个 Rust `target` 目录可安全回收（先停止构建及使用其中程序的测试进程）。首次重新构建需要重新编译。
- Cargo 默认已在用户级 `CARGO_HOME` 共享下载的依赖源码；`CARGO_TARGET_DIR` 可把构建产物放到外部公共目录，但工具链、profile、features、依赖版本和参数不同仍会产生不同产物，多项目也可能竞争构建锁。更精细的跨项目编译复用可另行配置 `sccache`。
- 本机已配置：项目 target 为 `D:\Cache\Rust\targets\codex-floatboard`（仓库根 `.cargo/config.toml`，仅本机忽略、不提交）；用户级 Cargo 配置启用 `D:\Cache\Rust\tools\sccache.exe`，缓存位于 `D:\Cache\Rust\sccache`，上限 20 GB。未设置全局 `CARGO_TARGET_DIR`，避免不同项目混用 target；其他项目需各自指定 `D:\Cache\Rust\targets\<项目名>`。
- Reporter 与 Tauri 共用本项目 target，两个 release EXE 会位于同一个 `release` 目录。以 `cargo metadata --manifest-path <Cargo.toml> --no-deps --format-version 1 --locked` 返回的 `target_directory` 为准；原生辅助脚本会打印实际目录，不要再从已清理的旧 target 路径取包。
- Cargo 使用用户级配置后会自动经 sccache 编译；新开的终端会继承用户级 `SCCACHE_DIR` / `SCCACHE_CACHE_SIZE`。已运行的其他终端或 IDE 可重启以刷新环境；也可直接通过 Cargo 使用配置文件中的环境默认值。维护说明与新项目接入脚本位于本机 `D:\Cache\Rust`。

0.8.0 新增自定义标题栏、离线目录迁移和同步诊断；数据库事件格式保持兼容。迁移要求先暂停所有 Codex 生产者并升级接入，不能继续混用旧版 Hook。
