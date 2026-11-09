# 应用源码

当前候选版本 2.0.0；统一 Cargo workspace 和 Cargo.lock 位于 desktop。

| 目录 | 职责 |
|---|---|
| desktop/protocol | 领域类型、校验、协议与偏好规则 |
| desktop/agent | 单实例、HTTP MCP/OAuth、存储执行器、通知、备份、离线维护 |
| desktop/storage | SQLite 任务投影、完成档案、计时及 Agent 的微信传输 |
| desktop/client | 身份握手、命名管道、短命 Hook、计划任务及 GUI 适配 |
| desktop/shell | 安装、更新、启动器和 DEV 微信实验室 |
| desktop/src-tauri | 原生窗口与客户端桥接 |
| desktop/src | 冻结 A 原型的 Svelte 界面 |

GUI、Hook、安装与更新壳的依赖图不包含 SQLite。只有 Agent 和取得相同维护互斥权的离线维护工具可打开业务库。

使用 [build-native.ps1](desktop/build-native.ps1) 加载本机 MSVC/SDK。以 cargo metadata 的 target_directory 为准，不从已删除的 Reporter target 取包。本机 build/local-msvc 为工具链，不属于产品源码或发行包。

开发 GUI 只经官方 [隔离 wrapper](desktop/dev-isolated.ps1) 启动。真实界面测试须使用 [独立 Desktop](../docs/automated-regression.md)。正式安装、旧版本目录和正在运行的进程不属于清理范围。
