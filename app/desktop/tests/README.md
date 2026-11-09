# 回归入口

```powershell
pnpm --dir app/desktop test
pwsh -NoProfile -File app/desktop/build-native.ps1 -Phase test-runtime
pwsh -NoProfile -File app/desktop/build-native.ps1 -Phase test-core
pwsh -NoProfile -File app/desktop/build-native.ps1 -Phase release
```

protocol/client/agent 的测试覆盖协议、事务、幂等、实例竞争和真实进程；storage/shell 覆盖投影、计时、通知、签名包及安装中断续跑。安装测试使用手动测试计划任务、独立 DEV profile、合成签名密钥，不登记正式登录自启。

前端 tests/*.test.ts 是逻辑和契约检查；native-*.cjs 是历史场景片段，复用须传入本轮独立 CDP，不能直接指向现有实例。本轮实际执行入口及结果见 artifacts/agent-runtime-20261109/acceptance.md。

真实 Tauri 验证遵循 docs/automated-regression.md：显式构建、官方 wrapper、-CodexMode isolated、Win32 CreateDesktop、loopback CDP，不切换用户 Desktop 或注入系统输入。只清理本轮有身份记录的资源。

签名包构建并通过 verify-release 后，用 Python 3.11+ 和 psutil 执行专用入口回归（不启动 GUI）：

```powershell
python app/desktop/tests/migration-entry.py --assets <已验证资产目录> --evidence <本轮证据根> --version 2.0.0
python app/desktop/tests/migration-entry.py --assets <已验证资产目录> --evidence <本轮证据根> --version 2.0.0 --no-states --legacy-receipt
```

每次创建独立 DEV UUID、中文库名、配置副本和手动测试任务；覆盖包内 Preview/Capture/Apply/Cutover、单元素与空数组、设置、旧回执归属、非本产品配置保留。激活须另经原生 GUI/OAuth 验收。结束正常停止自有 Agent、删除自有任务，保留结果与失败现场。
