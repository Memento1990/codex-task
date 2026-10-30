# 参与贡献

欢迎围绕多会话进度、卡点可见性和本机可靠性提交问题或修改。

## 提交问题

在 [Issues](https://github.com/Memento1990/codex-task/issues) 中提供版本、Windows / Codex 环境、复现步骤、预期与实际结果。截图使用合成数据或先脱敏；不要上传配置备份、会话绑定、Token、真实任务库或原始提示词。

安全问题请按 [SECURITY.md](SECURITY.md) 处理，不公开漏洞利用细节或真实凭据。

## 提交代码

1. Fork 仓库并创建独立分支，每个 PR 解决一个问题。
2. 使用 README 中的 Windows 标准工具链和冻结锁文件安装依赖。
3. 保持现有风格，避免无关重构。修改行为时补充对应测试。
4. 至少运行 `pnpm --dir app/desktop test`、`pnpm --dir app/desktop build` 和 `cargo test --manifest-path app/desktop/reporter/Cargo.toml --locked`。
5. 原生窗口、通知、托盘或接入有变化时，在 Windows 真正运行 Tauri 程序；Vite 页面不能代替原生验收。
6. PR 说明问题、行为变化、验证结果、截图及已知限制；无法运行的检查明确标记。

测试使用隔离临时目录和合成数据，不操作正式 Codex 配置、任务库或自启项。人工接入验收前应明确范围。

## 发布

版本号同步于 `app/desktop/package.json`、两个 `Cargo.toml` 和 `app/desktop/src-tauri/tauri.conf.json`；Cargo 锁文件同步更新。

发布前完成生产构建、原生运行验证、敏感内容检查和版本一致性检查。便携包中的两个 EXE 放在同一目录；附上许可证、依赖与升级说明，并为 ZIP 生成 SHA-256。发布资产放在 GitHub Release，避免将二进制、依赖或本机证据提交到 Git。

已发布的 tag 和资产保持不可变；修复后发布新版本。提交贡献表示你有权按本项目的 MIT 许可证提供相应代码。
