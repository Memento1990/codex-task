# Codex 接入复盘与重构建议

状态：2026-11-02复盘提案（下文保留设计时判断）。0.8.1已实现主要固定入口、事务恢复、旧归属确认、MCP事件和单入口向导；真实Codex+模拟模型+产品GUI验证通过，完整交付范围与限制以[0.8.1说明](releases/v0.8.1.md)和[验证记录](releases/v0.8.1-validation.md)为准，不将未实测事件或正式账户链路冒称已验。

## 结论

现有 Hook + MCP 的业务职责基本合理，但接入的部署、归属恢复、事务恢复和健康状态没有形成产品级闭环。不能通过取消保护、伪造信任或让用户求助 AI 修改配置来解决。

产品仍是 Tauri/Svelte/Rust；此前仓库外 Python 脚本仅为授权的临时维护工具，不属于发行版，也没有恢复 Python 依赖。不过人工恢复不应成为正常产品流程，后续检查/修复能力应实现在 Rust 内。

## 已确认问题

1. 配置直接包含发行目录下 Reporter 路径和数据目录。每次解压到新版位置都可能需要重写接入、重新加载和重新确认信任。
2. integration.json 与任务数据混放，是唯一归属依据。记录缺失时默认把“当前版本期望配置”当作旧配置；路径变化即被判同名冲突。本次实际属于此情形，缺失原因尚无证据。
3. prepared receipt 在 config 更新之前替换旧 receipt。若随后失败/崩溃，会出现新 receipt + 旧 config；目前没有状态恢复分支。
4. 旧 Hook 只有完全相等才识别。人工微调的旧条目可能被保留并追加新版，缺乏疑似本工具冲突分支，可能双报。
5. 配置项数量齐全就 ready；历史任意 Hook 成功或 sessions 文件就 hook_seen。不能证明当前连接、当前版本或本次会话端到端可用。
6. MCP 预检是本工具直接启动子进程并列出工具，不是正式 Codex 已加载的证明。
7. “让 Codex 帮我连接”与主连接并列，把同一操作变成两条不确定路径，也把配置维护负担推给 AI 和用户。
8. Windows 命令 Hook 经 cmd /C，存在本机 AutoRun 输出污染；不能声称只需重启就必定修好。

## 用户体验目标

普通路径只需要：打开看板 → 点击“连接 Codex” → 必要时在 Codex 原生界面确认信任 → 方便时重启 Codex → 自动验证。

不要求复制路径、修改 TOML、理解 MCP/Hook、操作 receipt 或运行脚本。不承诺零确认或无重启；用户有活动任务时提供“稍后完成”，不得强制结束 Codex。固定版本官方 app-server 提供 `config/mcpServer/reload` 等能力，但本工具没有现有 Desktop 的 stdio 控制连接，另启 app-server 不能刷新正在工作的正式进程；不应把保守重启建议说成所有版本一律只能重启。

主界面只有一个主操作。高级页承载配置差异、版本、位置、脱敏诊断导出、断开连接；删除“让 Codex 帮我连接”主流程入口。

## 推荐架构

### 固定用户级连接入口

首次授权后，将产品自有 Rust Bridge 放在固定用户级程序目录，不需要管理员权限。Codex 仅引用这个固定入口，不再引用解压目录和任务数据路径。

程序、控制状态、任务数据分离：

- 稳定 Bridge 与版本化运行时：用户级程序目录。
- integration registry、事务 journal、数据位置指针：固定控制状态目录，不随任务数据迁移。
- 用户可迁移任务库：data目录。

Bridge根据已验证的安装注册表加载版本化运行时；升级先落盘完整新版和校验，再原子切换指针。不能直接覆盖正在执行的 EXE，不能因旧 GUI 启动而降级。长驻 MCP/采集器的协议版本要显式兼容，旧会话可以保留旧版本直到退出，不强制热切换。

稳定路径只能减少不必要的配置变化，不保证更新后永远无需信任；Codex安全规则变化时仍走其正常确认流程。该默认模式有用户级安装副作用，必须说明；纯便携模式放高级选项，明确移动后可能要重连。

### 可恢复归属及配置事务

归属记录不再是丢失即不可恢复的单点。识别分三类：

- 完整匹配已登记本工具配置：幂等检查/升级。
- 无记录，但 MCP、Hook 集合、参数、数据位置与已知本工具配置严格一致：展示“发现旧版连接”，用户确认恢复；不能仅按文件名或路径中包含产品名认领。
- 有陌生命令、额外权限/env、被微调的疑似旧 Hook或其他差异：不覆盖、不追加重复 Hook；给出明确冲突项和可取消的处理流程。

journal保存before/after配置hash及旧/新归属。重启恢复时磁盘等于before则回到旧状态/重试；等于after则完成commit；两者都不是则停止并提示并发修改。备份不得覆盖，断开也按实际匹配归属进行，不能整体回滚覆盖用户后续更改。

### 新鲜且分层的连接状态

状态拆分：未连接 → 准备连接 → 等待信任/重载（可确认时才给确切原因）→ 等待真实会话 → 已收到当前会话 → 已验证任务同步。

MCP记录真实initialize的实例/版本；Hook记录当前代次及会话；结构化进度记录真实accepted/consumed事件。独立显示本地自检和正式Codex连通，不把隔离预检当真实成功。不凭历史文件置绿，不因无业务任务就判失败。

连接验证可提供“一次性验证任务”短指令，由用户主动在Codex中运行，确认会话绑定与报告到达；验证不读项目秘密、不修改项目、不能把模拟测试通知当真实连通。超时说明尚未验证并提供下一步，而不是反复写配置。

## Hook 与 MCP 技术路线

保留MCP负责结构化业务进度，事件负责真实会话身份/上下文和运行状态。不让模型猜session_id或放弃会话鉴权；Stop也不等于业务完成。

新增优先原型：固定版本Codex 0.165源码存在 mcp_tool Hook，可模板化事件字段并直接调用MCP，可能绕开命令shell。SessionEnd明确不支持，不能将9项逐项无损替换。需实测SessionStart时MCP就绪顺序、上下文输出契约、审批与信任、恢复会话、并发与错误处理，再决定默认启用。不能只按版本字符串认为Desktop能力一致。

按实际价值裁剪事件：基本任务汇报优先依赖会话绑定/提示上下文及MCP；运行态、审批、子Agent等逐项验证。缺少可靠SessionEnd时明确显示运行状态未知/最近活动，不能伪造已结束。旧版本兼容模式不允许悄悄改变全局shell或注册表。

不推荐只读历史日志替代接入：格式、隐私与业务语义稳定性不足；也不推荐接管另启Codex app-server作为小工具默认方案，它不能自动代表用户已有Desktop会话。

## 实施顺序及发布门槛

1. 隔离原型验证当前实际Desktop的MCP Hook和真正往返；若不通过，先明确支持范围，不继续靠重启掩盖。
2. 实现固定入口、控制状态分离及配置journal/恢复；Rust内置旧接入识别，增加故障注入测试。
3. 实现单主流程连接向导和分层状态，兼容旧配置迁移，取消要求用户运行脚本。
4. 实测首次安装、0.7升级、重复点击、解压移动/删除、receipt丢失、写入崩溃、并发编辑、其他MCP/Hook共存、中文空格路径、AutoRun、旧会话持续工作、断开/回滚及数据迁移。

发布验收：普通首次接入不编辑配置、不运行命令、不找AI修复；升级不因解压位置重写接入；可修复故障有产品内恢复；陌生配置不覆盖；只有真实数据链路验证后才声明同步可用；不擅自停止用户工作。

## 源码证据

- 项目 `app/desktop/reporter/src/integration.rs`：归属比对、10项present、prepared/apply顺序及MCP预检。
- 项目 `app/desktop/reporter/src/ingest.rs`：Hook会话鉴权绑定和MCP结构化上报。
- 项目 `app/desktop/src/App.svelte`：现有多入口接入弹窗与状态展示。
- 官方固定版本命令Hook执行：[command_runner.rs](https://raw.githubusercontent.com/openai/codex/rust-v0.165.0/codex-rs/hooks/src/engine/command_runner.rs)。
- 官方MCP Hook执行：[mcp_runner.rs](https://raw.githubusercontent.com/openai/codex/rust-v0.165.0/codex-rs/hooks/src/engine/mcp_runner.rs)。
- 官方发现/限制/信任：[discovery.rs](https://raw.githubusercontent.com/openai/codex/rust-v0.165.0/codex-rs/hooks/src/engine/discovery.rs)。
- 官方会话开始上下文解析：[session_start.rs](https://raw.githubusercontent.com/openai/codex/rust-v0.165.0/codex-rs/hooks/src/events/session_start.rs)。
