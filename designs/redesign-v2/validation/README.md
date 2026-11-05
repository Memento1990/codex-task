# redesign-v2 原型验证

日期：2026-11-05。原始基线为 `3d2b660`，验证对象为该提交之后的本地未提交工作区。
此记录对应原型补全阶段，范围为 `designs/redesign-v2/`。后续用户已选 A 并完成产品移植，原生验证另见 [v1.0.0 说明](../../../docs/release-1.0.0.md)；下表保留原始 Mock 结果，不作为原生通过证据。

## 验证结果

| 层次 | 结果 | 证据 |
|---|---:|---|
| Mock 状态与计时 | 9/9 | [状态脚本](../tests/mock-state.test.cjs)、[最终输出](state-regression.txt) |
| A 基础交互 | 30/30 | [browser-a.json](browser-a.json) |
| B 基础交互 | 30/30 | [browser-b.json](browser-b.json) |
| C 基础交互 | 30/30 | [browser-c.json](browser-c.json) |
| 共用流程 | 20/20 | [browser-flows.json](browser-flows.json) |
| 主题、窄窗、接入边界及 C 卡点焦点 | 13/13 | [browser-final-smoke.json](browser-final-smoke.json) |
| 通知启停、空任务通知、预览入口链接与文案 | 15/15 | [browser-delivery-smoke.json](browser-delivery-smoke.json) |

基础交互覆盖搜索/空结果/筛选、详情与焦点、卡点结果校验、证据、命令键盘操作、任务完成门槛、
工具窗、完成记录分页、多种场景、392×570 窄窗、头像悬停/恢复及运行时错误观察。
共用流程覆盖接入分阶段回执、断开、渠道校验/启停/测试/重试、模拟微信绑定、迁移确认、
更新安装确认、修复失败/成功、模拟窗口操作与头像左右吸附。
共享脚本语法检查、`git diff --check` 及 HTML/文档本地链接检查均通过。

## 首次结果与修正

保留首次失败，未将修正后的结果覆盖为首次通过：

- 状态首轮 5/9：修正解除卡点时执行段未冻结、重开任务计入归档后的时长，
  以及重置后旧修复/安装回调污染新场景的问题；最终 9/9。
- [A 首轮](browser-a-initial.json)：窄窗连接状态挤出边界；限制状态文本宽度，
  并给验证 URL 增加版本参数以排除旧 CSS 缓存。
- [B 首轮](browser-b-initial.json)：证据按钮选择器同时匹配背景与抽屉；
  修正测试选择器，使其只在当前抽屉内选择。
- [C 首轮](browser-c-initial.json)：搜索输入触发重绘导致输入节点断开；保留输入所在工具栏节点。
- [共用流程首轮](browser-flows-initial.json)：拖动头像后的合成点击意外恢复主窗；
  在主事件捕获处理器内抑制拖动结束后的点击。首轮 19 个断言通过，拖动断言及随后执行状态失败；最终 20/20。
- [通知补测首次异常](browser-notice-initial.txt)：浏览器操作完成后，Node HTTP 链接检查触发
  Undici 内部断言，未生成完整结果；改用页面 fetch 检查链接，不将此轮计入通过数。

## 执行方式

在仓库根目录启动仅本机 HTTP 服务：

```powershell
python -m http.server 4311 --bind 127.0.0.1 --directory designs
node --check designs/redesign-v2/shared/mock.js
node --check designs/redesign-v2/shared/data.js
node --check designs/redesign-v2/shared/kit.js
node --test designs/redesign-v2/tests/mock-state.test.cjs
git diff --check
```

浏览器由 ego-browser 驱动，使用本任务独占的 TaskSpace 8 / Page p1；
未使用用户输入桌面、系统鼠标键盘注入或原生 Tauri 实例。
对应脚本为 [基础回归](../tests/browser-regression.mjs)、[共用流程](../tests/browser-flows.mjs)、
[收尾检查](../tests/browser-final-smoke.mjs)。基础回归通过脚本 URL 的 `variant=a|b|c` 选择方案。
最终补测见 [交付检查](../tests/browser-delivery-smoke.mjs)。

示例命令（本次会话已绑定 TaskSpace 8）：

```powershell
ego-browser nodejs -e "await import('file:///D:/projects/codex-floatboard/designs/redesign-v2/tests/browser-regression.mjs?variant=a')"
ego-browser nodejs -e "await import('file:///D:/projects/codex-floatboard/designs/redesign-v2/tests/browser-flows.mjs')"
ego-browser nodejs -e "await import('file:///D:/projects/codex-floatboard/designs/redesign-v2/tests/browser-final-smoke.mjs')"
ego-browser nodejs -e "await import('file:///D:/projects/codex-floatboard/designs/redesign-v2/tests/browser-delivery-smoke.mjs')"
```

本次结束后该 TaskSpace 会关闭。其他会话重放时应创建自己的测试空间并替换脚本里的空间 id，
不要接管用户的浏览器空间。

## 视觉核查与限制

已实际查看三套主窗、窄窗和浅色主题截图，以及 C 卡点详情、设置与头像吸附截图，
核对信息可读性、边界和内部滚动；截图捕获本身不等于视觉通过。

| 方案 | 主窗 | 窄窗 | 浅色主题 | 头像球 |
|---|---|---|---|---|
| A | [主窗](a-main.png) | [窄窗](a-narrow.png) | [浅色](a-light.png) | [头像](a-dock.png) |
| B | [主窗](b-main.png) | [窄窗](b-narrow.png) | [浅色](b-light.png) | [头像](b-dock.png) |
| C | [主窗](c-main.png) | [窄窗](c-narrow.png) | [浅色](c-light.png) | [头像](c-dock.png) |

补充：[C 卡点下钻](c-blocker-detail.png)、[头像左侧吸附](dock-drag-left.png)。

这些结果验证的是 HTML Mock。未执行真实消息投递、配置写入、文件迁移、安装、修复或联网接入，
也未执行原生 Tauri / 独立 Win32 Desktop 回归。这些限制仅指本节的 Mock 验证。后续 A 产品移植已另行构建并通过独立 Desktop 回归，仍须遵守 `docs/automated-regression.md` 的独立 Desktop 验证要求。
