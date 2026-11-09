> 历史事故证据，基于重构前源码；旧路径和处置命令不适用于当前版本。当前流程见 ../../migration-install.md。

# 数据库运行时身份与诊断

本记录对应 2026-11-06 的只读现场检查与隔离修复。原始证据位于 `artifacts/redesign-fidelity-audit-20261106/`。检查没有停止生产 Codex、Reporter 或看板，没有在生产 SQLite 上运行查询，也没有执行生产修复或修改生产配置。

## 现场结论与边界

旧生产 worker PID 67852 使用 v0.7.0，当前 GUI 与 runtime 指针使用 1.0.0。五个 0.11.0 `--mcp` 是会话进程，不能仅凭旧版本将它们认定为采集 worker。0.7 worker 没有新协作停机协议，也不随 MCP 会话结束必然退出，因此“关闭旧会话后再试”不足以保证安全。用户应先保存工作并正常重启 Windows，不能自行强杀 Reporter 或删除锁/SHM。

句柄元数据证明旧 worker 的 `collector.lock` 和主库 FileID 不同于当前同名路径，而 WAL/SHM FileID 相同。旧文件上的锁不能保护被替换后的新文件对象；这解释了为何历史 worker 存在时，新 worker 仍可能获取当前路径锁并写出 `starting`。谁在何时替换文件没有证据，不能归因于特定操作。

当前路径 DB/WAL/SHM 的独立副本通过完整性检查，39 条任务和 25 条归档 JSON 可读；确切 1.0 Store 读取出 31 个可见投影。旧恢复报告只有 3 个最终任务不构成当前恢复上限。没有现场最近一次 SQLite 扩展错误码，不能将文件身份分裂直接宣称为已证明的 GUI 即时读取错误根因。详细 FileID 与证据在 `db-runtime-audit.md`、`repair-source-triage.md`。

## 实现保证

- Store 与 ProcessLock 持有拒绝删除共享的句柄，并核对当前路径 FileID；正常持有期间替换主库/锁会失败。
- 打开 Store、普通启动、修复授权前检查相关 Reporter 的句柄元数据。不会读取其数据库内容或关闭对方句柄，不以 exe 版本/进程存在直接判冲突。
- 同名不同 FileID 直接拒绝。原名因 unlink 丢失或被改名为主库/锁的后缀备份时，若仍持有目标 WAL/SHM、又缺少当前主库与锁的完整成对身份，保守拒绝。完整当前成对身份可排除无关临时文件删除造成的误报。
- GUI 同步启动检查必须看到当前版本或已驻留更新版本的 `ok`，不是写过 `starting` 即成功。Hook/MCP 将同版本 Reporter 的 `--ensure-worker` 检查放在独立后台进程，协议回复与邮箱接收不等待采集器就绪，也不宣称已收集；后台检查完整复用相同文件身份、交接锁、旧版协议和真实就绪校验。启动后台进程前暂时清除本进程标准管道的继承标志，随后恢复，避免 Windows 让后台进程持管道阻止 Hook 输出 EOF。
- 持迁移锁的内部暂停路径只启动空闲当前采集器，不等待被自己锁住的存储初始化。普通迁移继续等 nonce 暂停确认；修复另须验证新鲜 nonce、实例和全部旧根锁身份。
- Store 错误保留固定读取阶段和 SQLite 数值码；JSON 原子替换保留固定阶段和 Win32 码。对瞬时 5/32/33 最多重试 250ms；永久拒绝保持旧值并失败。
- 页面遇到文件身份冲突直接显示保存工作、正常重启 Windows、保留 DB/WAL 且勿删库/重建/反复重连的固定提示。其他 SQLite 错误保持通用安全提示，不透传底层错误文字。

## 保守限制

句柄清单是一次元数据观察，不是任意外部写入者协议。无法检查的高权限/受保护进程、任意文件改名形态或未来直接写旧路径的外部程序不在完整保证范围。原名丢失且缺少完整身份对时，可能因其他已删除磁盘对象被保守阻止；此时保留现场并正常重启，不放宽保护。新版本长期拒绝删除共享，可避免本版本自身形成同类替换。

诊断持久化依赖控制目录可写；磁盘满、权限或异常终止可能使最后记录缺失。退出 JSON 不含私密错误正文，`storage.unknown` 表示未取得固定阶段，不能据此推断故障原因。

后台检查结果在 `runtime/worker-check.json`，保存时间、PID、版本、`ok/error` 和允许字段诊断；正式目录使用 `control/runtime/`。后台进程创建或标准管道隔离失败分别使用固定阶段 `worker.check_launch`、`worker.check_handles`，协议响应仍不阻断用户工作。该文件表示最近一次检查结果，不是持续健康或事件已收集的证明。

## 回归证据

永久用例在 `tests/runtime_file_identity.rs`，覆盖真实替换/共享 WAL、改名备份、主库缺失、健康临时文件、原锁不丢、快照错误码、不泄露正文、真实启动就绪和正常/异常退出。`tests/migration.rs`、`tests/repair_runtime.rs` 与 `tests/repair_transaction.rs` 继续约束源锁不释放、旧入口迟到上报、损坏库保活与安全暂停。

`tests/producer_startup.rs` 在真实 worker 被迁移锁保持为 `starting` 时验证 MCP 初始化与 SessionEnd 命令 Hook 在 2.5 秒内完整结束并关闭输出管道，在已有 starting owner 时仍返回 SessionStart 绑定；另外验证 0.7 不被后台检查停机，以及竞争检查复用同一当前 worker。

首次红灯、修正后输出和最终数量记录在 `artifacts/redesign-fidelity-audit-20261106/runtime-fix-acceptance.md`。原生 Win32 Desktop 验收与最终发布由主任务分别执行，核心测试不能替代原生或视觉还原验收。
