<script lang="ts">
  import TaskContents from './TaskContents.svelte';
  import { done, filterTasks, isReportedTask, openBlockers, runtime, status, summaryTone, taskCounts, taskTitle, type Task } from './model';
  import { compactTime } from './matrix-format';
  let { tasks, now, selected = $bindable(''), filter = $bindable('active'), query = $bindable(''), loading = false, readError = '', onDetails, onOpenEvidence, onCopy, onCompleted, onCommand }: {
    tasks: Task[]; now: number; selected?: string; filter?: string; query?: string; loading?: boolean; readError?: string;
    onDetails: (task: Task, tab?: number) => void; onOpenEvidence: (reference: string) => void; onCopy: (text: string) => void; onCompleted: () => void; onCommand: () => void;
  } = $props();
  let showDone = $state(false);
  const counts = $derived(taskCounts(tasks));
  const visible = $derived(filterTasks(tasks, filter, query, showDone));
  const completed = $derived(tasks.filter(task => isReportedTask(task) && task.status === 'completed'));
  const segments = $derived([
    { key: 'running', label: '在跑', value: counts.running }, { key: 'blocked', label: '受阻', value: counts.blocked },
    { key: 'waiting', label: '待确认', value: counts.waiting }, { key: 'done', label: '完成', value: counts.done },
    { key: 'unknown', label: '未汇报', value: counts.unreported }, { key: 'cancelled', label: '已取消', value: counts.cancelled },
  ]);
  const filters = [['active', '进行中'], ['all', '全部'], ['blocked', '有卡点'], ['running', '执行中'], ['waiting', '待确认'], ['completed', '已完成'], ['unreported', '未汇报'], ['cancelled', '已取消']];
  const project = (task: Task) => task.title.split(' · ')[0] || task.cwd.split(/[\\/]/).filter(Boolean).at(-1) || task.host_id;
  function toggleCompleted() { showDone = !showDone; filter = 'active'; }
</script>

<div class="mx-board" data-testid="matrix-board">
  <div class="mx-overview" aria-label="任务概况">
    <div class="mx-stat running"><div class="mx-stat-label">在跑</div><div class="mx-stat-value">{counts.running}</div></div>
    <div class="mx-stat blocked"><div class="mx-stat-label">受阻</div><div class="mx-stat-value">{counts.blocked}<small>{counts.blockers} 卡点</small></div></div>
    <div class="mx-stat"><div class="mx-stat-label">待你确认</div><div class="mx-stat-value">{counts.waiting}</div></div>
    <div class="mx-stat done"><div class="mx-stat-label">已完成</div><div class="mx-stat-value">{counts.done}</div></div>
    <div class="mx-distribution"><div class="mx-dist-label">任务分布 · {counts.total}</div>
      <div class="mx-dist-bar">{#each segments.filter(segment => segment.value) as segment (segment.key)}<i class={segment.key} style:width={`${counts.total ? segment.value / counts.total * 100 : 0}%`} title={`${segment.label} ${segment.value}`}></i>{/each}</div>
      <div class="mx-legend">{#each segments.filter((segment, index) => index < 4 || segment.value) as segment (segment.key)}<span><i class={segment.key}></i>{segment.label}{segment.key === 'unknown' || segment.key === 'cancelled' ? ` ${segment.value}` : ''}</span>{/each}</div>
    </div>
  </div>
  {#if query}<div class="mx-search-scope"><span>搜索：{query}</span><span class="mx-result-count">{visible.length} 项</span><button aria-label="清除任务搜索" onclick={() => query = ''}>清除 ×</button></div>{/if}
  <div class="mx-grid-heading"><span class="mx-task-heading"><span>任务</span><select aria-label="任务状态" bind:value={filter}>{#each filters as [value, label]}<option {value}>{label}</option>{/each}</select></span><span>进度</span><span>当前步骤</span><span>卡点</span><span>耗时</span></div>
  <div class="mx-rows" data-testid="matrix-rows">
    {#each visible as task (task.id)}
      {@const tone = summaryTone(task)}{@const blockers = openBlockers(task)}{@const progress = task.plan.length ? done(task) / task.plan.length : 0}
      <button class="mx-task-row" class:selected={selected === task.id} data-task-id={task.id} aria-expanded={selected === task.id} aria-controls={selected === task.id ? `mx-details-${task.id}` : undefined} onclick={() => selected = selected === task.id ? '' : task.id}>
        <span class="mx-stripe {tone}"></span>
        <span class="mx-task-name"><b title={taskTitle(task)}>{taskTitle(task)}</b><small>{project(task)} · {task.status === 'completed' ? '已结束' : runtime[task.runtime] || '未知'}{task.status === 'unreported' ? ' · 等待结构化汇报' : ''}</small></span>
        <span class="mx-progress-cell"><span class="mx-progress"><i class={tone} style:width={`${progress * 100}%`}></i></span><span class="mx-progress-numbers"><span>{task.plan.length ? `${done(task)}/${task.plan.length}` : '暂无步骤'}</span><span>{task.plan.length ? `${Math.round(progress * 100)}%` : '—'}</span></span></span>
        <span class="mx-current-step"><span>{task.current_step || (task.status === 'completed' ? '—' : '等待下一步进展')}</span><small>{blockers[0]?.title || status[task.status] || task.status}</small></span>
        <span class="mx-blocker-count" class:has-blockers={blockers.length > 0}>{blockers.length ? `! ${blockers.length}` : '—'}</span>
        <span class="mx-task-time" class:live={task.status !== 'completed'} title={task.timing?.partial ? '部分计时记录' : '自然耗时，包含等待与受阻'}>{compactTime(task.timing, now)}</span>
      </button>
      {#if selected === task.id}
        <div class="mx-expanded" id={`mx-details-${task.id}`} data-testid="matrix-expanded">
          <TaskContents {task} {now} columns {onOpenEvidence} {onCopy}/>
          <div class="mx-expanded-actions"><button onclick={() => onDetails(task, blockers.length ? 1 : 0)}>任务详情 ↗</button></div>
        </div>
      {/if}
    {:else}<div class="mx-empty mx-list-empty">{loading ? '正在读取任务' : readError || (query ? '无匹配任务' : '当前筛选下没有任务')}</div>{/each}
  </div>
  <div class="mx-completed-band"><b>✓ {completed.length}</b><span>项已完成</span><button onclick={toggleCompleted}>{showDone ? '收起已完成' : '在表中显示 ↗'}</button><button class="mx-history-button" onclick={onCompleted}>完成记录 ↗</button></div>
</div>
