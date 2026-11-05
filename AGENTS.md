# UI 原型优先范围（2026-11-05）

- 本轮以 `designs/redesign-v2/a-inspection-matrix.html` 及共享样式、交互为视觉基准，落地约定见 `docs/brainstorms/2026-11-05-redesign-a.md`。适用主窗、392px 窄窗、行内三栏详情、完整详情抽屉、头像球、通知和全部工具窗。
- 优先保留 A 的顶部统计、44px 任务行、行内展开、右侧详情抽屉及暗色布局。通用 UI 默认规则中的固定左导航、36–40px 行高、禁止概览区、详情仅用抽屉、统一分页数量等，仅用于原型和已确认需求未规定的部分；不得据此重排已确认的 A 方案。
- 新安装默认 980×680、暗色，最小 392×570；已有用户保存的主题、窗口尺寸和位置保留。中间宽度及浅色主题须响应式适配，滚动由实际内容区域闭环处理。
- 普通设置即时保存，失败恢复已确认值并显示错误。扩展字段放入高级配置或详情；迁移、接入变更、修复和安装沿用真实确认及校验流程，不补造不存在的 Dry run 或成功结果。
- 任务由 Agent 汇报驱动。正式界面提供查看、复制和定位；原型外壳、场景切换、Mock 数据与人工推进等演示动作不进入正式运行界面。缺失时间保持未知，累计耗时保留 partial，证据时间仅表示首次记录。
- 此处只调整 UI 默认规则的适用范围，不放宽下述正式环境保护、开发隔离、凭据、真实行为验证及 Win32 独立 Desktop 回归要求。

# Local development and installed release separation

- Daily-use installation: `%LOCALAPPDATA%/Programs/CodexFloatboard`. Treat this directory as deployed production; never copy Cargo outputs, locally packaged ZIPs, or modified assets into it.
- Production data: `%LOCALAPPDATA%/CodexFloatboard/data`; production Codex configuration and existing Reporter sessions are not development fixtures. Do not stop them or overwrite their owned connection configuration. User explicitly authorized DEV to use local Codex on 2026-11-02: only independently owned codex_floatboard_dev entries may be added/removed after preview/confirmation; retain codex_floatboard and all other configuration.
- Start development GUI ONLY via `app/desktop/dev-isolated.ps1` / `pnpm dev:isolated` in that directory. It uses `%LOCALAPPDATA%/CodexFloatboard-Dev/<workspace-hash>` with separate LOCALAPPDATA, APPDATA, data/control and WebView profile. Default local Codex mode shares the user's CODEX_HOME without copying auth; -CodexMode isolated explicitly uses the independent CODEX_HOME for fixtures. Never copy actual credentials into the sandbox.
- The DEV wrapper launches an already-built executable, not a server. Build explicitly first. Avoid bare Tauri dev or direct build-output execution. New development builds reject unisolated target/debug launches, display DEV, and deny production updates/system auto-start.
- Repository root `启动任务侧窗.cmd` and the desktop “正式版” shortcut launch the installed root launcher, not a repository releases directory.
- Development lifecycle: code -> isolated build/test -> user-authorized commit/push/GitHub Release -> installed app checks/downloads verified release -> explicit install confirmation. Do not silently publish a new version or replace a published tag/asset to deliver local changes.
- Existing historical releases may be occupied by old Reporter processes; retain them until sessions end normally. Do not kill Codex/Reporter/GUI to clean directories.
- A user request to “发版” / “发布版本” defaults to a GitHub Release for `Memento1990/codex-task`, not a local package or production installation. Use `app/desktop/publish-release.ps1` and follow `docs/releasing.md`; run preparation first, inspect the artifacts, then explicitly publish. Never bypass a failed verification or replace an existing published tag/asset.
- Details: docs/development-isolation.md.

# Required local automated regression

- User-approved default (2026-11-04): after development changes that require automated testing, run the relevant frontend/unit/core tests and build checks; run native GUI regression on a **Win32 independent Desktop (`CreateDesktop`)**. Windows Task View virtual desktops are not a substitute. Follow `docs/automated-regression.md`.
- Do not switch the user's input desktop or use system-wide mouse/keyboard injection. Drive the isolated WebView through loopback-only CDP and verify real Tauri/native behavior. Ordinary unit/core tests may run outside the independent Desktop; report the layers separately.
- Build current source explicitly first, then launch DEV only through `app/desktop/dev-isolated.ps1` with `-CodexMode isolated`. Use a dedicated test workspace/sandbox and WebView profile; do not reuse, activate, stop, or overwrite an existing DEV/production instance. Never copy real credentials or modify the user's Codex configuration for regression fixtures.
- Record source revision and dirty state, test commands/results, desktop/PID ownership, logs and screenshots. Use readiness checks rather than a fixed startup sleep. Investigate failures; preserve initial results when correcting test assertions. Do not claim unexecuted/unsupported cases passed or claim screenshot capture alone is visual validation.
- Close only the test-owned GUI normally, release owned Desktop handles, and verify debug-port/process cleanup. Never kill GUI/WebView/Reporter/Codex by process name. Real-input/focus/tray/drag/multi-monitor cases not covered by CDP require explicitly reported gaps and a separately authorized interactive test; do not silently fall back to the user's active desktop.
- Testing permission does not authorize commit, push, release, installation, or production changes.
