# 专用迁移安装与异常处置

1.x 与 2.x 的数据库所有权、接入和更新协议不同。先完成隔离验收和发布，再另行安排正式维护窗口。本轮不执行正式迁移。

## 单一入口

维护者准备一份 request.json，固定包、签名、工作目录、目标 profile、旧程序根和全部数据来源。使用 app/desktop/migration-install.ps1，不需要逐个拼装内部程序参数。

发行包的版本目录同时包含 migration-install.ps1 与本手册 MIGRATION.md。在该目录执行时，下列命令中的 ./app/desktop/migration-install.ps1 使用 ./migration-install.ps1；tools_directory 指向该已验证版本目录。历史来源必须显式登记，不允许用空清单替代不可读取的旧库。

request 字段：version、work_directory、install_root、tools_directory、archive、manifest、signature、registration、profile、legacy_roots、sources、state_sources、legacy_receipt（没有时显式 null）。profile 字段遵循 client/src/profile.rs；正式 root 只能为当前 KnownFolder 用户目录/.codex-floatboard/profiles/<UUID>，使用当前 SID。首次登记端口选定后保持稳定。tools_directory 使用已验证的新包工具，不指向旧运行时。

来源条目为 label/role/database/main_only。完整历史备份、两侧增量以及维护窗口取得的最终稳定来源都要登记，不能只放最后一个可读主库。来源主库与 WAL/SHM 保留原件，工具只打开复制品。main-only 与 main+WAL 作为不同来源；不能将它们误称为协调快照。

state_sources 显式登记旧设置文件，条目为 label/kind/path，kind 为 preferences、window、notifications 或 weixin_account。分别指向旧 preferences.json、window.json、notifications.json、weixin-production/account.dpapi；没有文件时使用空列表，不猜来源。不同版本的设置也进入逐项冲突决议和来源哈希校验。窗口尺寸、位置、显式主题和订阅保留；旧主题迁移规则继续生效。DPAPI 凭据保持密文，旧发送中记录转 unknown，paused 期间不发送。正式凭据只在另行授权的正式维护窗口迁移；隔离演练只用合成凭据。

```powershell
./app/desktop/migration-install.ps1 -RequestFile ./request.json -Phase Preview
./app/desktop/migration-install.ps1 -RequestFile ./request.json -Phase Capture -Confirmed
./app/desktop/migration-install.ps1 -RequestFile ./request.json -Phase Apply -Confirmed
./app/desktop/migration-install.ps1 -RequestFile ./request.json -Phase Cutover -Confirmed
# 完成下面的 GUI / OAuth 验证后才执行：
./app/desktop/migration-install.ps1 -RequestFile ./request.json -Phase Activate -Confirmed
```

Preview 验签、固定请求和预览接入及自启变更。Capture 前先保存工作并等待清单中的旧会话/写入者正常退出；必要时正常重启 Windows。工具不强杀旧进程。Capture 每次生成全新证据目录；原来源变化则重做预览/决议，不能忽略最终增量。

Apply 前逐项审阅冲突，写 resolutions.json，绑定本次 preview_sha256。choices 中选择明确的版本摘要；仅业务一致的运行态差异可用 unknown_runtime。全部原始版本及来源仍保存。不可读来源必须单独确认，不能默认丢弃。Apply 只接受新目标，失败不覆盖旧库；留存失败目标供诊断，重试另用新 UUID/目录。

Cutover 复核输入哈希及迁移校验，精确移除已预览的本产品旧 Run 项、迁移旧 receipt 证明拥有的配置；其他配置保持原样。新版本计划任务随正式登录启动，后台先保持 paused。DEV 演练使用测试专属任务，仅显式运行及一分钟重复触发，无登录触发。

## 开放写入前的实际验证

运行根启动器打开新看板，核对新 Agent/profile/database 身份与迁移回执，核对任务及完成档案。使用实际 Codex 二进制在新接入上发起 OAuth，在新看板批准配对，完成一次经过认证的 MCP 往返。切换时新 Agent 必须同时记录本实例 GUI 已就绪与认证请求。然后 Activate 开放业务写入。

原件、迁移前和升级前备份均保留。已接收新写入后，不允许自动用旧库回退。程序回退须人工确认 schema 兼容；恢复数据由停止的维护工具写入全新目标，再重新核对启用。

## 中断与异常

install-transaction.json 记录 prepared/quiescing/backed_up/stopping/registered/healthy_paused/activating/complete。先读 journal 和 Agent identity，再继续相同请求。未完成事务禁止另一个版本覆盖。旧任务停用后才退出旧 Agent；不出现两个写入者。

磁盘满、权限、损坏、主库缺失/替换、身份不符或端点占用均明确失败。保留数据库、WAL/SHM、备份和诊断，不删除文件、反复重连或补建空库。存储故障 Agent 保持可诊断，不靠失败重启循环掩盖异常。看板关闭不停止后台；显式 stop 是维护入口。

同一个计划任务每分钟触发，IgnoreNew 保持健康实例不重复启动。异常退出后由下一次调度恢复；正常 stop 原子保存当前登录的停止意图，之后调度只检查意图并退出，不打开数据库或恢复采集。用户显式运行后台会清除停止意图，下次登录使用新的登录身份恢复启动。安装停用旧任务后才进行离线维护。本机 Windows 26300 的 RestartOnFailure 未执行，已用独立 PowerShell 对照复现；运行时不依赖该设置，也不增加常驻监督进程。

新库 paused 不等于迁移失败，表示等待验收；不要直接改 runtime_kv 绕过激活门禁。签名或兼容检查失败不更换公钥、不改清单绕过验证。诊断导出不包含任务正文和凭据。

## 覆盖边界

本轮验收使用隔离 DEV 根、配置副本、本地通知、合成签名夹具和独立 Desktop。真实登录自启、系统焦点/托盘/拖动/跨屏，以及真实微信/群机器人送达须另行安排，不能由隔离截图代替。
