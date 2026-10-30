# 更新记录

本项目使用语义化版本号。GitHub 首次公开发布为 `v0.7.0`，此前版本为本机开发版本。

## [0.7.0] - 2026-10-31

### 新增

- C 卡点调度台：全部 / 有卡点 / 执行中筛选，以及宽窗双栏、窄窗纵向布局。
- 可缩放窗口，保存大小与位置，最小化不覆盖正常尺寸。
- MIT 许可证、公开仓库说明、演示截图及 Windows x64 便携发布包。

### 变更

- 显示层迁移到 Tauri 2 + Svelte；采集、SQLite、Hook / MCP、接入和设置改由 Rust 实现。
- 正式运行包仅需 GUI 和 Reporter 两个 EXE；不再依赖 Python / Qt / Node。
- 保留任务、会话绑定、耗时、完成记录、双主题、托盘和不抢焦点通知。

### 兼容与限制

- 兼容既有事件邮箱；已有会话可以继续使用旧 MCP 进程。
- Windows x64 本机验证通过；WebView2 与 Visual C++ 运行库仍为系统前置条件。
- 发布包未签名，无安装器；自动派发 / 恢复任务及跨平台支持尚未实现或验收。

完整说明见 [v0.7.0](docs/releases/v0.7.0.md)。早期本机版本记录保留在 [app/CHANGELOG.md](app/CHANGELOG.md)。

[0.7.0]: https://github.com/Memento1990/codex-task/releases/tag/v0.7.0
