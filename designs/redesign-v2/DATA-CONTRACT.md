# redesign-v2 数据落地契约

本文保留三套 HTML Mock 的数据约定。A 已移植到实际 `model.ts` / Reporter；下表中的证据首次记录和当前等待段投影已实现，步骤 `segments` 保留为原型能力，实际产品继续使用累计 Timer，不增加历史流水。
原型使用 camelCase，实际组件直接消费 snake_case 快照；缺失数据保持未知，不补造历史。

## 字段映射与可提供性

| 原型字段 | 实际来源 / 建议提供方式 | 语义与限制 |
|---|---|---|
| `sessionId`、`hostId`、`currentStep` | `session_id`、`host_id`、`current_step` | 直接改名映射 |
| `resolveWhen` | `resolve_when` | 直接改名映射 |
| 任务、步骤 `startedAt` / `endedAt` | `timing.started_at` / `timing.ended_at` | 是业务记录的起止；不代表用户真实开工时间 |
| `tag` | 标题的 ` · ` 前缀；无分隔符时取标题或工作目录短名 | 派生显示名，无需新增持久化字段 |
| 卡点 `openedAt` / `closedAt` | 当前开放段取 `timing.since`；完整区间从现有事件投影 | 重开卡点不能用最早起点计算本次开放时长 |
| 证据 `id` | 任务 episode ＋ `label/reference` 的稳定标识 | 不用数组下标；改标签或引用视为新证据，可后续增加显式 id |
| 证据 `at` | progress 中该证据首次出现的事件时间 | 表示首次记录时间；没有记录则为 `null` |
| `waitingSince` | 最近进入 `waiting` 的 progress 状态转换 | 不用任务起点或最后更新时间代替 |
| 步骤 `segments` | progress/plan 中状态转换形成的活动区间 | 支持多段；不把未知间隔画成持续执行 |
| 自然耗时 | 原 `timing.seconds + (now - timing.since)` | `since` 为空时只取 `seconds`；保留 `partial` |

实际类型见 `app/desktop/src/model.ts`。Agent 上报仍只提交 `label/reference`；Reporter 在快照边界补充 `id/at/time_source`，不会信任调用方补造的记录时间。同值重复证据共享身份，界面允许重复项安全显示。

## 证据时间

优先从现有 progress 记录计算 `at`，并携带 `timeSource: "first_reported"`。
界面称“首次记录”或“首次汇报”，不得称“实际产出时间”。没有可追溯记录时使用
`at: null, timeSource: "unknown"`，放入无时间分组，不参与有时间节点排序。

稳定 id 应包含任务 episode。同一会话可以先后完成多个任务，完成归档的身份不能仅由会话 id 决定；
相关机制见 `app/desktop/reporter/src/store.rs:98`。
未来若需要 Agent 显式提供证据时间，应另行扩展上报 schema，并区分报告时间与产出时间。

## 计时、多段与部分记录

`timing.started_at/ended_at` 不能替代权威耗时：步骤暂停、恢复或卡点重新打开后，
起止跨度可能大于累计活动时间。Timer 的累计机制见 `app/desktop/reporter/src/timing.rs:10`。
步骤处于 `inProgress/blocked` 时计时，卡点 `open` 时计时，见同文件 `:39`。

首次看到的步骤已经完成时，真实起点未知，现有 Timer 会标记 `partial`。
时间线保留未知起点；泳道只绘制可追溯区间；耗时沿用“已记录”或“起点未记录”的表达。
数据库修复若未恢复计时历史，也不得从任务标题、更新时间或当前状态反推过去的区间。

建议视图契约保留原型起止字段，并附带：

```js
{
  timing: { seconds, since, partial },
  segments: [{ startedAt, endedAt }],
  waitingSince: number | null,
  evidence: [{ id, label, reference, at: number | null,
    timeSource: "first_reported" | "unknown" }]
}
```

## 状态汇总口径

三套方案统一按互斥优先级分类：**完成 → 取消 → 未汇报 → 有开放卡点 → 待确认 → 执行中**。
各状态数量相加应等于当前统计集合大小；`running` 且有开放卡点的任务归入“受阻”。
“受阻任务数”和“开放卡点数”分开显示，同一任务可以有多个卡点。
运行时 `runtime` 单独表达 Codex 活动情况，不用来替代任务业务状态。

主列表默认排除完成、取消、无结构化清单的未汇报会话；完成记录包含独立归档 episode，避免重复展示。
筛选只改变可见集合，不改变全局状态总数的定义。
“需要介入”由开放卡点及没有开放卡点的 waiting 任务组成，按本次开放/等待起点升序。
起点未知的项单独标记，不能给出虚构等待时长。

## 历史与隐私边界

已有最小 progress/plan 事件及时间可支持上述投影，见
`app/desktop/reporter/src/store.rs:166`、`app/desktop/reporter/src/timing.rs:23`。
无需恢复 `history` 动态页，不读取原始对话、工具参数、凭据或聊天索引；约束见 `docs/task-retention.md:9`。
Mock 数据、模拟时间及模拟操作结果不能写入真实任务库，也不能被标为真实安装或接入证据。

## 分阶段落地

1. **原型阶段（已完成）**：合成数据和内存状态驱动完整交互；明确模拟标识，补齐缺失/失败/重试/取消状态。
2. **适配阶段（A 已完成）**：保留原型结构，映射现有字段；复用真实 Timer，未知字段保持 `null/partial`。
3. **快照投影阶段（A 已完成）**：从现存业务事件计算证据首次记录、waiting 起点；同步类型与快照协议，验证旧库和完成归档。步骤区间未扩展，继续累计 Timer。
4. **必要时扩展上报**：仅在自动推导不能满足语义时增加可选稳定证据 id 或显式时间，兼容旧客户端，不强制补写旧记录。

以上阶段都保持“清单全完成、有验收证据、无开放卡点”才允许完成的业务门槛，
实际门槛见 `app/desktop/reporter/src/store.rs:90`。
