# Codex Floatboard

Windows 桌面任务看板，集中查看多个 Codex 会话的清单、卡点、证据和完成档案。采用 Rust、Tauri 2、Svelte 和 SQLite。

当前源码为 **2.0.0 候选版本**。本地验收及正式发布是独立步骤；已安装旧版本不会因源码更新而切换。

## 运行方式

每个 Windows 用户、每个 profile 只有一个 Agent。GUI 和短命 Hook 通过受限命名管道连接；Codex 使用 loopback Streamable HTTP MCP。连接数和业务会话数增加不会增加 Bridge/Reporter。

Agent 独占业务 SQLite，使用 WAL、synchronous=FULL 和串行执行器。report_progress 的事件、任务投影、完成档案、计时依据、通知待发送记录及回执在同一事务提交，提交成功后才返回 accepted。相同修订和内容可幂等重试，不同内容明确报冲突。

正式数据固定在 `%USERPROFILE%\.codex-floatboard\profiles\<profile UUID>`。正式 Agent 由当前用户计划任务随登录启动；看板是否自动打开单独设置。关闭或退出看板后后台继续采集、发送已启用的通知。显式停止后台会排空已接收请求并关闭数据库。

## Codex 接入

安装并登记 Agent 后，在「Codex 接入」预览并确认本产品的 MCP/Hook 配置，再通过 Codex 的 OAuth 登录流程发起本地配对，在看板中明确批准。使用授权码和 PKCE，令牌绑定 profile，可刷新、撤销。不复制 OpenAI 凭据。

保留 hook_context、report_progress 及现有业务字段。任务完成要求清单完成、存在验收证据且没有开放卡点；Hook Stop/SessionEnd、断连和休眠均不代表完成。缺失时间保持未知，有缺口的累计时间保持 partial。

## 数据维护

有业务变化时每日生成一致性备份，保留最近 14 份；手动、升级前和迁移前备份不自动删除。「备份与诊断」由 Agent 导出，诊断不包含任务正文和凭据。

存储异常明确失败并保留现有界面快照，不将不可读数据库当成空库，不自动补建替代库。没有在线搬库、修复窗口、运行中抢救或切库重启入口。独立离线维护只在 Agent 已停止并持有同一互斥权时处理数据；恢复写入全新目标。

从 1.x 升级需要专用迁移安装流程，不能用普通覆盖安装。先核对全部历史来源和增量，逐项解决冲突，再登记新 Agent；新 GUI、数据库、OAuth 验证通过后才开放写入。参见 [迁移安装手册](docs/migration-install.md)。

## 开发与验证

```powershell
pnpm --dir app/desktop install --frozen-lockfile
pnpm --dir app/desktop test
pwsh -NoProfile -File app/desktop/build-native.ps1 -Phase test-runtime
pwsh -NoProfile -File app/desktop/build-native.ps1 -Phase test-core
pwsh -NoProfile -File app/desktop/build-native.ps1 -Phase release
pwsh -NoProfile -File app/desktop/dev-isolated.ps1 -CodexMode isolated
```

原生回归遵循 [Win32 独立 Desktop 规范](docs/automated-regression.md)，构建后经官方隔离 wrapper 启动。禁止覆盖正式安装、正式库、现有连接或停止其他会话。详见 [开发隔离](docs/development-isolation.md)。

源码模块、依赖和运行状态见 [实施契约](docs/plans/2026-11-06-agent-runtime-redesign.md)。[发布流程](docs/releasing.md) 将本地准备、签名验证和 GitHub 发布分开；正式安装仍需另外确认。

## 文档

- [任务留存](docs/task-retention.md) · [消息通知与微信连接](docs/notifications.md)
- [自动更新协议](docs/automatic-updates.md) · [本轮验收](artifacts/agent-runtime-20261109/acceptance.md)
- `docs/archive/incidents/` 仅保留旧事故证据；其中的旧命令不适用于当前架构。

[MIT](LICENSE) · [第三方许可](docs/THIRD-PARTY-NOTICES.txt) · [GitHub Releases](https://github.com/Memento1990/codex-task/releases)
