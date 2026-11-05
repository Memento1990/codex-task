# BUG-20261106-fidelity-storage

状态：release-candidate。基线：8665ba7 / v1.0.0。目标：兼容修正版 v1.0.1；完整发布门禁及精确准备包冒烟仍须通过后才能发布。现场数据库恢复另行验收。

## 目标和确认

修复已确认的主题、可读性、工具栏入口和原型还原偏差，并修复数据库现场暴露出的文件身份/旧采集器交接缺口，完成隔离验收后发布 GitHub Release。用户2026-11-06明确确认上一轮分析报告的推荐方案，并要求数据库处理结果优先。

需求理解98/100：行为、受影响版本、范围和验收已明确；当前GUI的即时SQLite错误码与历史文件替换者尚不明，属于必须通过诊断和复现继续查证的技术事实，不作已确认根因。现场恢复结果与代码回归结论分别记录，不以发版替代现场验收。

本轮不重置数据、不强杀旧采集器、不替换安装目录、不修改真实Codex配置。发布已获授权；正式安装及可能需要的有序系统重启仍需独立明确操作。

## 视觉和交互基准

不可变视觉源：Git 3d2b660 的 designs/redesign-v2/a-inspection-matrix.html 及 shared/data.js、shared/kit.js。增强Mock只提供已批准交互，不能覆盖原始A的分区、开关、按钮、工具窗、球和完成行结构。现有原型不继续改动以迁就实现。

已确认差异：标题13px、正文12px、辅助11px、标签10–11px；直接置顶/主题切换、真实更多菜单、合并搜索；真实数据/确认/诊断/高级字段仍保留。首次升级采用原型暗色，之后明确选择的浅色保留，窗口尺寸/位置不重置。

## 验收标准

- AC1：顶栏搜索、头像球、置顶、日月、更多各自执行正确动作；菜单支持键盘/外部关闭，置顶状态和主题持久化，主题保存失败恢复。
- AC2：原始A视觉结构逐屏移植；980×680、772×570、392×570的主窗/行内/抽屉，以及设置/接入/通知/更新/修复/完成/球/摘要/通知态有等条件对照。浅暗双主题无裁切，字号达到基准。
- AC3：文件对象分裂的合成场景先复现；旧锁/主DB被替换仍共享WAL/SHM时，不启动另一个写入者、不误认唯一所有者、不切库。底层失败保留脱敏阶段/SQLite码/退出原因。
- AC4：完整可读库的恢复保留events、耗时和全部归档；部分抢救明确降级，不隐含用户同意历史丢失；失败保留原库并展示本次实际原因。
- AC5：真实修复/重启/迟到事件/再次启动在Win32独立Desktop通过；不得以截图采集代替视觉审阅。数据库损坏/旧协议/身份分裂负例均不修改原库或强停进程。
- AC6：前端、核心、原生构建及相关回归通过；发布使用正式两阶段脚本，检查签名、资产、版本、GitHub哈希；不自动安装。
- AC7：现场只读证据和必要后续操作单列。未在安装现场验证完整读写链路前，不宣称生产数据库已修复。

## 任务和映射

| AC | 工作 | 验证 |
|---|---|---|
| 1 | App工具栏、偏好主题迁移 | 原生按钮/菜单/持久化/失败回滚 |
| 2 | 冻结参考、移植展示层、去旧结构影响 | 逐屏同状态截图与结构/字体/主题检查 |
| 3 | 文件身份与runtime诊断保护 | 新失败复现+修复后的核心集成测试 |
| 4 | 完整恢复和降级分离 | WAL完整历史fixture、partial拒绝/独立确认 |
| 5 | 原生修复流程 | 独立Desktop业务回归与清理证据 |
| 6 | 发布准备/检查/发布 | build/test、prepared.json、远端资产核验 |
| 7 | 安装现场验证 | 允许的只读元数据、明确确认后的实际验证 |

## 技术边界、迁移和回滚

保留现有模型、Agent单一汇报源、IPC、通知和更新安全行为。主题采用一次性版本标记，后续设置仍即时保存。数据修复必须先证明写入者/源文件身份，不放宽repair pause。0.7没有新停机协议，普通关闭MCP不保证worker退出；必要现场操作在可审阅产物准备后明确安排。

回滚保留既有发布资产与原数据目录，不降级/覆盖数据库。无关未跟踪研究/设计/artifacts不进入发布提交；测试数据全部合成。

## 实际证据与验收

### 已执行的隔离验收

- 前端 `pnpm test`：47/47；Svelte 检查 0 errors / 0 warnings。包含命令搜索范围、主题迁移/失败回滚、修复状态的永久回归。日志：`artifacts/fidelity-storage-fix-20261106/frontend-window-scope.log`、`svelte-final.log`。
- 核心相关门禁：100/100（lib 54、migration 13、process 5、repair_runtime 5、repair_transaction 11、runtime_file_identity 10、storage_error 2）。真实 v0.9 opt-in 升级与 20 个旧启动器竞争场景重复 5/5；恢复定向集成 42/42，另有相关 lib 7/7。这些集合有重叠，不累计为一个总数。首轮红灯、修正及最终命令保留在 `artifacts/redesign-fidelity-audit-20261106/runtime-fix-acceptance.md`、`repair-implementation-validation.md`。
- Win32 独立 Desktop 原生 v2：40/40，分为交互矩阵 22、完整恢复 9、partial 拒绝 4、旧协议拒绝 5。均由官方 wrapper 在专属 isolated sandbox 启动，无切换输入桌面或系统输入注入；退出码 0，源码/GUI/Reporter 哈希前后一致，正常退出后自有进程、CDP 端口及 Desktop 句柄已清理。
- 完整恢复真实验证 4 个历史事件、2 段各 10 秒耗时、逐表原始内容摘要一致；迟到事件与重复重放仅计一次，再次启动历史保留。partial 场景保持源库不变、不产生切换或 helper；旧协议场景不发送不受支持的停机命令、不产生重定向，并在真实工具页说明正常重启要求。
- 原生截图 50 张确定性 `compare-*.png`，另有左右浮球 2 张。独立人工查看全量 contact sheet 与 29 张关键原图；50 份 geometry 均无外层滚动、主要容器越界或 PNG 尺寸不符。对应原始 A 补采 14 对同 viewport / DPR / 时钟参考；原版硬编码 Mock 工具页仅比较布局，浅色/详情抽屉无原版参考，不伪造比较。

原生 v2 固定构建：GUI `955E71B1D9D9DE879BDCEED64F57C52ABB4EC168D822ABBFFD64302C1677C4DF`，Reporter `ABC87F3913B54E5445B1045C61ABE457EF08D36451BF73D59263DCC10E5C59AB`。源码清单 `artifacts/fidelity-storage-native-20261106/source-final-build-v2.json`。独立数据库核验见该目录的 `database-independent-verification-v2.json`；视觉结论见 matrix v2 run 的 `independent-visual-review/README.md`。首轮断言/启动清理竞态及冻结时钟问题的证据原样保留，未计入最终通过数。

### 发布前复核发现并修正的兼容回归

独立复核发现，同步等待采集器就绪可能超过命令 Hook 的 3／5 秒期限。真实进程 fixture 先复现 2/2 超时；初次异步方案仍因 Windows 继承协议管道而延迟 EOF，保留第二次失败与管道诊断。最终 Hook/MCP 启动同版本独立检查进程，隔离自身标准句柄继承后立即返回协议，后台复用原身份、锁、交接和真实就绪检查；GUI 仍同步确认真实 `ok`。没有将协议回复冒充采集器健康。

新增 `tests/producer_startup.rs` 4/4，直接验证 MCP/SessionEnd 完整 EOF 在 2.5 秒内、SessionStart 绑定及时返回、0.7 即使带 protocol 1 仍不被停止、竞争检查复用一个 worker。最终相关门禁 105/105（54 lib + 1 bridge + 13 migration + 5 process + 4 producer + 5 repair_runtime + 11 transaction + 10 identity + 2 storage），真实 v0.9 加 20 个竞争启动器再运行 5/5。证据：`producer-runtime-core-fixed.log`、`producer-runtime-legacy-gate.log`、`runtime-final-review.md`，均在 `artifacts/redesign-fidelity-audit-20261106/`。与前述集合有重叠，不相加。

首轮 Prepare r1 因源码改变被正式脚本拦截（Source changed during build），无签名资产或可发布收据，明确作废并保存日志。接入页配置内部码 `missing` 同时改为“尚未配置”，高级配置状态采用中文标签；精确准备包原生回归将验证该显示及新增工具下半页/菜单截图。

### 发布门禁与限制

上述结果支持隔离修复验收，结论为 **CONDITIONAL PASS**：可进入两阶段发布准备，不能据此宣称安装现场已恢复。正式 `publish-release.ps1` Prepare 将重新执行完整前端、核心及 native Rust 测试和发布构建；精确签名准备包还须再次通过独立 Desktop 业务冒烟、资产/锁文件人工审阅及签名/哈希验证，之后才执行已授权 Publish。最终命令、计数、收据、远端资产与发布状态保留在本轮 artifacts 及外部 prepared 目录，不预填未执行成功结果。

CDP 覆盖真实 Tauri IPC 和文件持久化，不覆盖真实鼠标/焦点、托盘、拖拽、多屏与桌面透明合成；当前工具页初始截图不代表下半页已审阅，精确包回归补充内部滚动后的视觉检查。字号放大、真实高级字段/确认/错误以及不可用证据的禁用状态属于已确认落地差异，不作逐像素一致承诺。

### 正式现场开放项

只读检查证明历史 0.7 worker 持有的主 DB/collector.lock FileID 与当前同名路径不同，而 WAL/SHM 相同；当前路径 DB/WAL/SHM 独立复制件完整性正常（39 条任务、25 条归档，1.0 Store 投影 31 个可见任务）。尚无当次 GUI 最新 SQLite 扩展码，未把身份分裂推断为唯一即时故障原因。细节见 `docs/database-runtime-diagnosis.md`。

旧 0.7 没有安全退出协议，不能用关闭 MCP 会话、反复重连或再次升级界面保证释放。后续须用户保存工作并正常重启 Windows，经正式更新流程显式安装已验证版本，再确认任务列表、采集器 ok 心跳和新 Agent 汇报。未执行重启、安装、生产写入或重置；此开放项在实际链路恢复前不关闭。
