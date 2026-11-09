# 单 Agent 运行时与存储重构验收

日期：2026-11-09。基线 `38874fda177a1a7e55445a1dcd45cf354472730f`（1.0.1），当前源码候选 2.0.0，工作区有本次未提交改动。初始 tracked tree 干净；已有未跟踪研究、设计和历史证据保留。原始阶段记录见 `../agent-runtime-20261106/README.md`；固定应用文件摘要见 `application-source-inventory.json`。

## 结论与适用边界

本轮源码重构、构建、历史副本迁移、隔离接入、故障注入和原生验收完成。在线业务库由一个 Agent 持有；GUI/client/shell 依赖树无 SQLite；1、10、100 业务会话共用一个拥有者，默认 Bridge/Reporter 为 0；成功回执对应已提交事务。旧运行链已删除，保留离线维护、备份及事故证据。

建议进入 2.0.0 发布准备。已生成并独立验签本地候选包，尚未提交、推送、创建 GitHub Release、正式安装或迁移。正式环境的旧进程与数据库未切换，不能宣称旧现场已经恢复。1.x 升级必须使用专用迁移入口；旧版本不能使用协议 2 的普通自动更新。

## 分层执行结果

所有下列最终命令退出码为 0；首次失败单列保留。日志位于本目录，生成文件没有进入正式数据目录。

| 层次 | 命令/执行入口 | 最终结果 | 证据 |
|---|---|---|---|
| 前端 | `pnpm --dir app/desktop test` | 40/40 | `frontend-package-final.log` |
| 类型与内嵌前端 | `build-native.ps1 -Phase release`，包含 `pnpm build` | Svelte 0 errors/0 warnings，Vite 成功 | `release-package-final.log` |
| Agent/client/protocol | `build-native.ps1 -Phase test-runtime` | Agent 32、进程 4、client 2，共 38/38 | `runtime-final-stop.log` |
| 存储与安装 | `build-native.ps1 -Phase test-core` | shell 16、微信 5、任务投影 4、留存 3，共 28/28 | `core-package-final.log` |
| 原生宿主单元 | `cargo test -p codex-floatboard --locked`，先加载本地 MSVC/SDK | 9/9 | `native-unit-package-final.log` |
| 安装中断复验 | 单独构建 shell 并两次运行 `signed_cutover_resumes_after_each_durable_phase_without_two_writers` | 2/2 轮；各轮覆盖 5 个持久化中断阶段 | `cutover-final-repeat-fixed.log` |
| 实际 Codex | `transport-probe.py`，使用最终 release 副本和 Desktop 实际二进制 | 16 项通过 | `transport-release-final.log` |
| 原生窗口 | 官方 wrapper、isolated、Win32 CreateDesktop、loopback CDP | 14/14 | `native-package-ui.log`、`native-final-4529f90587/ui-results.json` |
| 原生激活/后台持续 | `native-actions.py gate / after-gui` | GUI/OAuth 门禁和关闭 GUI 后提交成功 | `native-package-activation.log`、`native-package-after-gui.log` |
| 真实任务托管 | `task-recovery-intent.py`，手动测试任务 | 5 项通过 | `task-recovery-intent.log` |
| 历史副本 | 六来源 preview/apply，再由最终 Agent 读取、backup/restore | 完整性通过，6 项读取/恢复核验通过 | `history-migration-corrected.log`、`history-agent-restore-final.log` |
| 包内迁移入口 | `tests/migration-entry.py` 两组 | 11/11、12/12；中文路径、设置和旧接入归属通过 | `migration-entry-stable-settings.log`、`migration-entry-stable-legacy.log` |
| 包和静态检查 | PS 解析、Node self-test、依赖/旧入口扫描、签名校验 | 全部通过 | `packaging-static.log`、`static-acceptance.json`、`verify-candidate.json` |
| 第三方声明 | `generate-notices.py`，锁定依赖及补充许可哈希 | 446 项，缺失许可正文 0 | `notices-complete.log` |

不同层验证不同性质，不把重复轮次叠加为额外业务覆盖。前端基线 47 项，删除旧修复链专属检查后为 40 项，未保留失效恢复功能凑数量。

## 需求与实际证据

| 需求 | 实际验证 |
|---|---|
| 单实例/存储归属 | 并发启动、同 UUID 不同根、不同程序/profile/protocol、端点占用被拒；主库 hardlink/替换、孤立 WAL、身份不符、缺主库均不补建空库 |
| 1/10/100 业务会话 | 真实 Agent 进程测试核对同一数据库拥有者和提交回执；实际 Codex app-server 另开 10 会话共享 HTTP 端点，无 Floatboard 子进程 |
| 连接释放 | 最终 release Agent 进行 100 轮 HTTP 初始化/断开，记录句柄、线程和 RSS；回到预设基线容差，没有持续累积；没有额外 Bridge/Reporter |
| 提交可靠性 | 事务前失败不返回成功，提交后响应丢失幂等重试返回原回执；重复/冲突/过期 revision、重启读取均验证 |
| 存储失败 | 合成满盘限制、Windows ACL 拒绝、损坏库、缺表和坏修订均明确失败，原文件保留；未伪装空库或 revision=-1 |
| 设置 | settings revision、请求回执和重试测试通过；GUI 实际即时保存主题，断连保留快照。明确失败与未确认结果分别处理 |
| 计时 | 缺运行证据保持未知；睡眠/重启间隔冻结于最后证据并标 partial；Hook 停止不自动完成任务 |
| 通知 | 合成凭据与本地 HTTP 服务验证 GUI 不在场仍发送、事务提交后发送、发送中断为 unknown、重启不重发未知记录、导入/paused 不发送 |
| 备份 | backup API 一致副本及 integrity_check；daily 14 份留存，manual/upgrade 不被自动删除；恢复全新 profile/数据库身份并保持 paused |
| 托管 | 同一计划任务重复触发，健康时一个 Agent；异常退出 52.75 秒恢复；正常 stop 后 75 秒不恢复采集；显式 run 恢复；自有任务删除 |
| 安装 | 签名包校验、持久化阶段续跑、不双写、停用旧任务、等待真实进程退出、新库 paused、GUI＋OAuth 门禁；新写入后无自动旧库回退 |
| 配置归属 | 旧回执明确证明拥有的 MCP/Hook 才替换；无关 MCP/Hook、注释保留；配置变化拒绝过期预览 |
| 旧链路清理 | reporter 整目录、独立旧锁、修复界面/IPC/抢救/在线搬库/演示数据删除；负例测试保留用于拒绝旧能力 |

## 实际接入与原生范围

本次实际 Codex 为 `C:/Users/Memento/AppData/Local/OpenAI/Codex/bin/f71a721e0e6193a9/codex.exe`。使用独立 CODEX_HOME、空配置和新 OAuth 授权，不复制 OpenAI/正式凭据；没有远端模型回合。验证真实 app-server 的 OAuth 回调及工具发现，配合实际 HTTP MCP 调用、PKCE 拒绝、刷新轮换、撤销、重启和跨 profile 拒绝。100 会话为真实 Agent 业务进程测试，实际 Codex 会话数量为 10，两者不混称。

最终原生 run：`native-final-4529f90587`；sandbox hash `7b1d3b77648f`；CDP 54010。Desktop `FloatboardRegression_...` 的精确名称、启动 PID 和窗口 PID/HWND 见 run 内 `desktop.json`、`windows.tsv`。官方 wrapper 原样复制到唯一 fixture workspace，先 CheckOnly、后 CreateProcess.lpDesktop 启动。未调用 SwitchDesktop 或系统输入注入。

已目视检查最终主窗、浅色设置、断连截图；另一次同 UI 构建的详情/392px 窄窗截图已检查。顶部统计、44px 行、行内三栏、完整抽屉、完成档案、主题、工具窗、头像球及失联恢复通过。GUI 正常退出后 Agent 可继续提交，随后正常停止并释放 Desktop，清理结果见 run 内 `cleanup.json`。

## 历史迁移核对

只使用已有只读诊断捕获，SQLite 只打开副本。来源为完整历史、安装侧捕获、Codex 侧 main+WAL 与 main-only，以及初始安装捕获的 main+WAL 与 main-only，共六种来源视图。非协调来源在预览中明确标识，未将其称为一致性快照。

| 项目 | 结果 |
|---|---|
| 完整基线 | 22,540 事件、39 条原始任务、25 条完成档案 |
| 预览版本数 | 22,917 事件版本、47 任务版本、25 档案版本，共 22,989 条 provenance |
| 逐项决议后的原始表 | 22,917 事件、41 原始任务、25 完成档案 |
| 新 Agent 实际可见任务 | 32；与原始表数分别报告 |
| 唯一冲突 | 同 revision=1 业务一致，仅运行时间约差 53 秒；保留全部两版本和来源，runtime=unknown，runtime_at/updated_at=null |
| 已精简/缺失证据 | 22,608 个已精简事件 payload；1 段保留修订缺口，26 条任务快照无可用 progress 事件；不补造丢失耗时 |
| 不可读来源 | 0；未来不可读来源必须显式确认 |
| 输入完整性 | main/WAL/SHM 的全部原始哈希前后相同 |
| 恢复 | API 备份后恢复到全新 DEV profile/数据库身份，完整性通过且最终 Agent 可读 |

明细为 `history-321ff5f6f9/preview/summary.json`、`preview/preview.json`、`resolutions.json`、`apply-result.json` 和 `agent-restore-validation.json`。这些含私有任务正文及路径的本地资料不进入发布源码清单。正式维护窗口仍必须取得停止旧写入者后的最终增量，不能直接采用本次历史演练副本切正式库。

旧窗口、偏好、通知及微信密文迁移另用合成来源验证；不同设置冲突未决拒绝，源哈希变化拒绝，DPAPI 密文保留且发送中状态变 unknown。正式凭据从未复制到隔离沙箱。

## 首次失败与修正记录

| 首次问题 | 处理与复验证据 |
|---|---|
| 本机 Windows 26300 不执行 RestartOnFailure | 多种触发和独立 PowerShell 最小对照同样失败。改同一任务每分钟重复触发＋IgnoreNew＋当前登录停止意图；`task-recovery-intent.log` 真实五项通过，不增加常驻监督进程 |
| 一次 healthy_paused 超时 | 保留 `core-post-intent.log`；stop 客户端持有验证后的真实进程句柄并等待完全退出。`core-full-exit-fixed.log` 与两轮单独中断测试通过，不以单次重跑抹去首次失败 |
| 独立 shell 编译缺少 WindowsAndMessaging feature | 工作区特性合并曾掩盖，已直接声明；`cutover-final-repeat.log` 保留编译失败，fixed 两轮通过 |
| 包内 PowerShell 单元素数组展开、空字符串 JSON 键解析失败 | JsonFile 使用 InputObject，工具结果使用 AsHashtable；中文/单元素/空数组、旧回执两组实际入口通过，首次日志保留 |
| 原生首轮 3 个断言失败 | 正确检查展开状态、关闭按钮及 DPI/frame 尺寸后 14/14；`native-823d7e94a5/ui-results-initial.json` 和截图保留 |
| 历史读取 Python GBK 解码失败 | 显式 UTF-8，原始失败保留；最终 Agent/restore 六项通过 |
| 签名调用未传外部密钥路径 | 未生成新钥、未改公钥；按已登记路径传入后签名验证成功，`package-candidate.log` 保留 |
| 静态扫描命中拒绝旧能力的测试字符串 | 保留负例，扫描限定运行代码；初次扫描和最终结果分别保存 |
| 全新 Prepare target 中安装测试缺少程序夹具 | 首轮 `official-prepare-initial.log` 保留。test-core 显式先构建 Agent/client/shell，再执行进程安装测试；新外部目录重跑，结果见 release-prepare.json |
| 第二次 Prepare 编译 glob 时缓存包装器退出 1，无 Rust 诊断 | `official-prepare-cache-failure.log` 保留。独立缓存/直接编译均成功，尚不能确定原始根因；仅对下一轮 Prepare 禁用 RUSTC_WRAPPER、并发限为 4，在全新目录执行全部检查，结果单独记录 |
| Publish 暂存检查发现新文件格式问题 | `official-publish-whitespace-failure.log` 保留；停止于 commit 前，无远端变更。规范 5 个新 Cargo 文件的 EOF 空行、3 份许可的行尾/EOF，许可单词不变，原件哈希和规范化哈希均保留；重新生成 notices。Prepare 增加未跟踪清单格式预检，重新完整构建签名后才发布 |
| 后续全新 core 测试中调试 Agent 在健康检查前退出 | 任务回执为 Windows `0xC0000142`；同文件以 CREATE_NO_WINDOW 运行正常。调试后台原为控制台子系统，现统一为 Windows 子系统，避免计划任务隐式分配控制台。`scheduled-console-fixed.log` 三轮真实安装中断续跑及 PE 子系统回归通过；完整 Prepare 和真实任务恢复结果另记。该诊断不推断数据库损坏，未调整健康超时或添加自动重试掩盖失败 |

更早的编译、断言和工具入口失败亦保存在本地原始日志；只有最终明确复验的结果计通过。

## 测试资产与删除范围

沿用并迁移任务投影、留存、计时和微信协议测试；前端模型、通知、工具窗和版本检查按新客户端契约调整。新增 Agent 事务/通知/迁移测试、ownership 进程测试、计划任务意图测试、shell 安装中断测试和包内 `tests/migration-entry.py`。旧数据库修复/抢救/切库链专属测试随功能删除，损坏/身份/孤立 WAL 等必要反例已转新架构回归。

`static-acceptance.json` 按 app/desktop 文本源码、配置和测试计数，包含未跟踪新模块，排除 lock 与生成物；删除及净增减详见 [清理报告](cleanup.md)。没有用 `git diff --stat` 漏算新代码。

## 包、发布与剩余边界

本地签名资产目录记录于 `candidate-current.json`，五项资产哈希见 `verify-candidate.json`。八个运行程序与最终原生 run 的 `binary-hashes.json` 一致；包内有迁移脚本与 MIGRATION.md，不含私钥、数据库、真实配置或授权文件。发行签名为 pinned Ed25519 更新签名，不宣称 Windows Authenticode 签名。第三方许可缺文件补充有锁定来源及哈希；四项使用标准全文＋包作者元数据，不等于法律审查。

发布使用 docs/releasing.md 的 Prepare（新外部 target、明确文件 allowlist）及其 receipt；现有本地候选不是绕过 Publish 检查的通行证。后续本地 Prepare 的命令、路径、退出码和 receipt 记录在 `release-prepare.json`（如已执行）。本轮 GitHub 只读核对远端 main 与 HEAD 相同、最新 v1.0.1、无 v2.0.0 草稿；未写远端。

未覆盖并且不计通过：真实注销/登录触发、用户系统焦点/输入、托盘真实输入、拖动/跨屏、真实微信或群机器人联系人送达，以及正式旧现场切换。这些不阻止源码与隔离阶段收口，但正式安装须独立维护窗口及确认。任何“磁盘永不损坏”承诺不在方案内。

## 资源清理

最终 GUI/Agent 正常退出，CDP 端口关闭，自建 Desktop 已释放；真实 transport 的自有 Agent/app-server 已退出。测试专属计划任务已全部删除，未登记正式登录自启。原件、WAL/SHM、历史备份和诊断证据保留。清理核验及原有进程对照见 `resource-cleanup.json`；不按名称批量结束进程。
