# 桌面测试与原生验收

## 自动测试

从仓库根目录运行：

```powershell
pnpm --dir app/desktop test
pnpm --dir app/desktop build
cargo test --manifest-path app/desktop/reporter/Cargo.toml --locked
```

Rust 测试使用临时数据目录、合成会话和隔离的注册表测试键；进程测试会启动并回收测试采集器。不要将正式 Codex 配置或任务库传入测试。

`reporter/tests/fixtures/legacy-events.sqlite3` 和 `legacy-tasks.json` 是旧数据兼容基线，不是待清理的运行数据；删除旧 Python 实现不影响这些测试。

## 原生交互验收

`native-ui.cjs`、`native-completed.cjs` 和 `native-notifications.cjs` 是交给外部 Playwright/CDP 运行器执行的 `async (page) => ...` 函数片段，不是可直接用 Node 运行的独立测试，也未包含在 `pnpm test` 中。

运行前必须：

1. 构建当前 GUI 和 Reporter，置于同一目录。
2. 设置独立的 `CODEX_HOME` 和 `CODEX_FLOATBOARD_DATA`（或向程序传入 `--data-dir`），使用临时配置和合成任务；必须在启动 GUI 前设置，确保其子进程继承隔离环境。
3. 使用可连接真实 Tauri WebView2 的外部运行器，预先准备合成数据。界面片段依赖特定任务标题和数量，不会自动装载夹具；可参考 `../public/fixture.json` 的演示内容。
4. 从固定工作目录执行片段；截图输出到相对该目录的 `test-results/native/`，运行器应先创建该目录。

主窗片段会实际执行连接/断开操作，通知片段会实际保存偏好；不得附着正式运行实例。普通 Vite 浏览器页面不能替代原生验收。清理临时数据前，先退出测试 GUI 并回收测试采集器。

`window-probe.ps1` 使用 Win32 API 检查或调整指定进程窗口，必须显式传入测试进程的 `TargetProcessId`，不可对日常实例执行。

公开发行的历史验证摘要见仓库根目录 `docs/releases/v0.7.0-validation.md`，不代表每次源码修改均已重新执行原生验收。
