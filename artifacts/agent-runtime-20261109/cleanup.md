# 旧链路、文档与资源清理

2026-11-09，基线 `38874fda177a1a7e55445a1dcd45cf354472730f`。仅清理本次重构退出的能力，不改动其他未跟踪研究、设计、历史资料和正式安装。

## 已删除

- `app/desktop/reporter/` 整目录及其独立锁：默认 stdio Bridge/Reporter、worker 启动、inbox、扫描与旧运行时切换入口。
- 数据库修复窗口、旧 IPC 能力、抢救、切库重启、修复事务、在线搬库和重定向；旧链专属测试和打包引用。
- 旧演示 fixture 和生产前端读取演示数据的入口。
- 已失效的数据库修复手册、接入设计草案及被当前文档替代的旧微信通知手册。

新维护器仅在同一互斥权下离线操作，恢复写入新目标；它承担明确授权的数据维护，未保留旧运行时兼容或在线抢救分支。针对旧能力的拒绝测试是必要反例，不是旧实现。

## 保留和归档

- 事故证据移至 `docs/archive/incidents/`；18 份旧发布/验证记录移至 `docs/archive/releases/`，另保留早期 changelog。
- 归档只调整必要链接；事故、旧命令和旧版本行为明确标注为历史，不再用于当前操作。
- 冻结 A 原型、未参与本次功能的研究与缓存运维资料原样保留。
- 所有原始数据库、WAL/SHM、历史备份和诊断捕获保留；未为清目录结束旧进程。

当前入口为 README、实施契约、迁移安装手册、协议 2 更新手册与 2.0.0 说明。

## 代码计数

口径为 app/desktop 内 Rust、前端、脚本、文本配置和测试，包含全部新建但未跟踪模块，排除依赖锁、生成构建、node_modules 和二进制。迁移到新目录的代码计入新文件，不能将旧目录删除量直接当净减少量。

| 项目 | 行数 |
|---|---:|
| 基线 | 8,848 |
| 当前 | 6,563 |
| 删除文件内容 | 5,557 |
| 新文件内容 | 3,373 |
| 整体净减少 | 2,285 |

逐文件清单和七项静态检查见 `static-acceptance.json`，可复现入口为 `static-acceptance.py`。GUI/client/shell 依赖树无 rusqlite/libsqlite3；旧 reporter 目录和修复窗口不存在；正式运行代码没有旧启动入口。运行代码扫描排除测试模块，保留的旧方法名称仅用于能力拒绝断言。

## 测试资源

最终 Win32 Desktop run 为 `native-final-4529f90587`：GUI 正常退出后验证 Agent 继续提交，再 IPC 正常 stop；CDP 54010 关闭，Desktop handle 已释放，无自有 GUI/WebView/Agent 残留。

任务托管测试和签名包迁移测试只登记 `CodexFloatboard-Test-<UUID>` 手动任务，无正式登录触发。最终任务查询为空。实际 Codex、历史读取/恢复和迁移入口自有进程均退出。

最终只读核验没有结束进程，`resource-cleanup.json` 记录余留进程和测试任务均为 0。最终原生启动前登记的 24 个既有产品进程，结束核验仍为同一 PID 和创建时间。正式现场保持运行，不能将这些进程计作本轮清理遗漏或擅自结束。

签名更新包精确包含八程序及 LICENSE、THIRD-PARTY-NOTICES.txt、README.txt、migration-install.ps1、MIGRATION.md；程序哈希与最终原生验收副本一致。包内没有库、WAL/SHM、OAuth/DPAPI 文件、Codex 配置或私钥。资产结果见 `verify-candidate.json`。
