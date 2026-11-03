# 开发启动与正式安装隔离

正式版安装在用户 LOCALAPPDATA/Programs/CodexFloatboard 下，由正式入口启动；源码开发不得复用该安装或公开 releases 便携包。本脚本只启动**已经编译好的开发 GUI**，不自动编译、不启动 Vite/其他服务器、不替换正式程序、不结束任何正式进程。

## 当前默认：DEV连接本机Codex（2026-11-02用户授权）

默认 `-CodexMode local`，沿用用户CODEX_HOME（未设置时为用户目录/.codex）；不复制auth。开发LOCALAPPDATA/APPDATA、数据、控制运行时和WebView继续独立，使用沙箱下local-codex子根避免旧独立配置收据混用。启动脚本本身不创建/清空本机配置；用户在接入面板预览/确认后，才添加独立codex_floatboard_dev MCP与Hook条目，严格保留已识别的完整正式连接和其他条目。不完整/被修改的连接阻止操作，不自动覆盖。DEV断开仅移除自身条目。

正式和DEV同时接入时，Codex会看到两个同名report_progress工具（服务器命名空间不同）。DEV上下文明确指向codex_floatboard_dev.report_progress，不能把正式连接session token用于DEV。旧正式程序可能将新增DEV Hook视为冲突：如需调整正式连接，先在DEV中断开。既有会话不强制重启，新配置仍需Codex正常载入和信任。DEV不自动标记授权或同步成功。

`-CodexMode isolated`保留原全隔离测试路径；dev-codex.ps1固定使用该模式，不影响默认本机模式。临时/合成验收必须显式使用isolated或-CodexHome临时目录，禁止写真实索引作为fixture。以下沙箱说明中独立CODEX_HOME仅适用于isolated模式。

## 推荐入口

在 app/desktop 下执行：

```powershell
pnpm dev:isolated
# 同义入口
pnpm dev:native
# 显式使用 debug 构建或其他本机开发产物
pwsh -NoProfile -File ./dev-isolated.ps1 -Configuration debug
pwsh -NoProfile -File ./dev-isolated.ps1 -Executable 'D:\my-build-target\release\CodexFloatboard.exe'
# 仅检查，输出不含 Token/凭据的 JSON，不创建目录、不写配置、不启动 GUI
pwsh -NoProfile -File ./dev-isolated.ps1 -CheckOnly
```

默认使用 D:/Cache/Rust/targets/codex-floatboard-retention/release/CodexFloatboard.exe，debug 参数选择同 target 下 debug。缺失时直接失败，**绝不回退到稳定版**。可执行文件必须名为 CodexFloatboard.exe；LOCALAPPDATA/Programs 中的安装程序、任意 releases 路径、可执行文件与沙箱路径上的 junction/symlink 都拒绝。

不要把 `pnpm tauri dev`、`pnpm tauri` 或直接双击构建 EXE 当作隔离入口：这些裸命令不设置环境。新源码已对debug构建、target/targets目录中的GUI及显式DEV环境加启动门槛：缺隔离参数与环境即退出；不要依赖旧构建具有该保护。现有 `pnpm dev` 仅是前端 Vite；它不是原生隔离方案。本变更没有启动任何新开发服务器。

GUI需要完整内嵌前端。正式式本机开发构建应显式走 build-native 的 release 阶段/Tauri CLI；直接 cargo build GUI 若没有启用 tauri/custom-protocol，仍会访问 devUrl，包装脚本不会偷偷启动服务器来掩盖这个问题。编译由开发者单独执行，不属于启动脚本行为。

## 沙箱布局与环境

根目录为真实用户 LOCALAPPDATA/CodexFloatboard-Dev/<仓库绝对路径SHA256前12位>/，不同源码副本分开：

```text
<workspace-hash>/
  codex/config.toml                 首次创建为空，此后保留DEV接入配置
  local/CodexFloatboard/data/       独立数据库及 desktop preferences.json
  local/CodexFloatboard/control/    独立连接收据/运行时/更新状态
    updates/preferences.json      auto_check=false, auto_download=false
  roaming/                        独立 APPDATA
  webview/                        独立 WebView2 profile
```

显式传递 `--isolated-instance --data-dir <沙箱data>`，并仅给子进程设置 LOCALAPPDATA、APPDATA、CODEX_HOME、CODEX_FLOATBOARD_DATA、WEBVIEW2_USER_DATA_FOLDER 及 FLOATBOARD_DEV_ISOLATED=1、FLOATBOARD_DEVELOPMENT=1，不改变父 shell 环境。

数据目录恰好匹配沙箱 LOCALAPPDATA/CodexFloatboard/data，因此 bridge::control_root 的 managed 分支落在同一沙箱的 control；UpdateService 的真实偏好位置是 control/updates/preferences.json，不是 data/updates。启动时预写自动检查/下载关闭，desktop 的 data/preferences.json 保留其他开发偏好但强制 auto_start=false、startup_command为空。

清除继承的 CODEX_*、FLOATBOARD_*、OPENAI_*、AZURE_OPENAI_*、ANTHROPIC_*、WEBVIEW2_* 环境变量后再写隔离变量，避免生产连接 generation、preflight、私钥路径、API凭据或调试端口渗入。不会读取、复制真实 Codex config.toml、auth.json 或 Token。配置只在首次创建时为空，之后保留隔离接入与测试授权，开发数据与其他文件持久保留；不要把真实凭据粘贴进沙箱。需要验证连接时，仅在独立测试 Codex 环境中显式操作。

启动前控制台清楚打印 DEV、可执行文件、配置、数据、控制目录和 WebView 路径。CheckOnly 不输出当前环境值或任何凭据。

## 可选全隔离模式：开发Codex

以下仅适用于显式`-CodexMode isolated`；默认local模式直接使用本机Codex，无需此独立终端。

日常Codex Desktop使用正式CODEX_HOME，不会看到DEV Hook，这是隔离的预期行为。先在DEV看板确认接入，然后在终端执行：

```powershell
pwsh -NoProfile -File ./app/desktop/dev-codex.ps1
# 只检查路径，不启动Codex
pwsh -NoProfile -File ./app/desktop/dev-codex.ps1 -CheckOnly
```

该入口启动本机Codex CLI，使用与DEV看板完全相同的隔离CODEX_HOME和独立测试workspace，不是另开正式Desktop。必须正常登录、确认信任；不复制正式auth、不绕过Hook信任，也不自动运行模型任务。首次登录可能需用户交互。正式Desktop的Hook页没有DEV配置并不代表DEV写入失败。

最近会话摘要功能已移除，不扫描Codex会话索引、不用历史标题初始化占位任务。看板保留真实业务任务记录和已有的已完成任务查看功能。

## 运行边界

- 同一源码副本共享一个沙箱，不要同时用此入口启动多个实例；重复启动会激活同一开发窗口，不再重置配置。需要更换构建时先正常关闭旧开发 GUI。
- 脚本不写系统注册表。Rust desktop::save_options现已拒绝隔离实例开启系统自启；保存其他设置直接写隔离偏好，不调用正式Run key，亦不会撤销正式自启项。
- DEV界面具有显眼文字标识及[DEV]窗口标题，宿主禁用后台检查及手动更新操作（允许读取状态），防止开发GUI走正式更新渠道。不要用开发构建改变正式数据位置。
- 本机正式版已从GitHub v0.9.0下载验签并安装至用户LOCALAPPDATA/Programs/CodexFloatboard，桌面/开始菜单“Codex Floatboard 正式版”只指向固定根启动器。仓库根启动脚本亦转到该安装，不再启动releases内副本。
- 正式版数据继续使用既有LOCALAPPDATA/CodexFloatboard/data及原Codex配置；不复制数据库、不重装连接、不替换仍活跃的旧Reporter。开发构建绝不拷入正式安装。开发完成后先提交发布新版GitHub Release，再由正式版确认安装。
- 环境与目录隔离不是恶意同用户代码的安全沙箱；独立 APPDATA/LOCALAPPDATA 不会重定向 Windows HKCU 注册表或所有操作系统能力。
- 本脚本不提供清理/删除操作。退出开发程序用自身窗口正常退出，不按名称结束所有 GUI、Reporter、Codex 或 WebView 进程。
