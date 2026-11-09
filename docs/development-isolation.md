# 开发与正式环境隔离

正式安装 `%LOCALAPPDATA%/Programs/CodexFloatboard` 及旧数据、旧进程是保护现场。当前源码不对它们执行覆盖、迁移、重连或停机。2.x 正式受管数据根为 `%USERPROFILE%/.codex-floatboard/profiles/<UUID>`，需后续专用迁移安装后才使用。

开发 GUI 只用 app/desktop/dev-isolated.ps1 或 pnpm dev:isolated。wrapper 启动已构建程序，不构建、不启动 Vite、不回退稳定包。显式 release 构建使用 build-native.ps1 -Phase release，target_directory 由 Cargo metadata 确定。

```powershell
pwsh -NoProfile -File app/desktop/dev-isolated.ps1 -CodexMode isolated -CheckOnly
pwsh -NoProfile -File app/desktop/dev-isolated.ps1 -CodexMode isolated
```

默认 local 模式沿用用户 CODEX_HOME，不复制 auth。只有在接入页预览确认后可修改本 profile 拥有的 codex_floatboard_dev；保留正式 codex_floatboard 和全部非本产品项。新架构不识别旧 receipt 时明确拒绝，不猜测覆盖。

自动化固定 isolated 模式，使用原样复制 wrapper 的本轮独立 fixture 工作区，取得唯一 workspace hash。数据、profile UUID、IPC、端口、AppData、WebView、Codex 配置均独立。wrapper 清除继承凭据环境；不复制真实 auth.json、token 或索引。

沙箱在真实 LocalApplicationData/CodexFloatboard-Dev/<workspace hash>；local 模式另加 local-codex 子根。profile.json 固定登记 Agent 路径和首次端口。只有 wrapper 可显式初始化全新 DEV 空库；旧数据库缺失或身份不符时不自动补建。

wrapper 可以启动自己的测试 Agent；正式 GUI、Hook、MCP 不得启动其他版本 Agent。GUI 只可请求运行已登记计划任务。关闭 GUI 后 Agent 保持运行；测试结束通过本轮 profile 的 stop IPC 正常停止。

DEV 显示标识、拒绝系统登录自启和正式更新。环境变量隔离不是防恶意同用户代码的安全边界；命名对象和目录仍使用当前 SID、UUID 及 ACL 验证身份。

原生验证必须遵守 [CreateDesktop 回归规范](automated-regression.md)。禁止复用既有沙箱、输入 Desktop、真实通知和按名称结束进程。
