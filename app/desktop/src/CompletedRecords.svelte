<script lang="ts">
  import { done, isReportedTask, matchesTask, taskTitle, type Task } from './model';
  import { compactTime, recordedDate } from './matrix-format';
  let { tasks, now, query = $bindable(''), page = $bindable(1), pageSize = $bindable(20), scopeTask = '', onDetails }: {
    tasks: Task[]; now: number; query?: string; page?: number; pageSize?: number; scopeTask?: string;
    onDetails: (task: Task, tab?: number) => void;
  } = $props();
  const filtered = $derived(tasks.filter(task => isReportedTask(task) && task.status === 'completed' && (!scopeTask || task.id === scopeTask || task.source_task_id === scopeTask) && matchesTask(task, query)).sort((a, b) => (b.timing?.ended_at || 0) - (a.timing?.ended_at || 0)));
  const pages = $derived(Math.max(1, Math.ceil(filtered.length / pageSize)));
  const currentPage = $derived(Math.max(1, Math.min(page, pages)));
  const visible = $derived(filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize));
  const day = (task: Task) => recordedDate(task.timing?.ended_at).split(' ')[0];
  $effect(() => { if (page !== currentPage) page = currentPage; });
</script>

<div class="mx-history" data-testid="completed-records">
  <div class="mx-history-heading"><span><b>{filtered.length}</b> 已完成任务</span><input aria-label="搜索完成任务" placeholder="搜索完成任务" bind:value={query} oninput={() => page = 1}></div>
  <div class="mx-history-scroll">
    {#each visible as task, index (task.id)}
      {#if index === 0 || day(task) !== day(visible[index - 1])}<div class="mx-history-day">{day(task)}</div>{/if}
      <div class="mx-history-row"><span class="mx-history-check" aria-hidden="true">✓</span><button class="mx-history-task" aria-label={`查看 ${taskTitle(task)}`} onclick={() => onDetails(task, 0)}><span>{taskTitle(task)}</span><small title={task.cwd}>{task.cwd.split(/[\\/]/).filter(Boolean).at(-1) || task.host_id} · {done(task)}/{task.plan.length} 步骤 · {recordedDate(task.timing?.ended_at)}</small></button><span class="mx-history-time">{compactTime(task.timing, now)}</span><button class="mx-history-evidence" aria-label={`查看 ${taskTitle(task)} 的 ${task.evidence.length} 项证据`} onclick={() => onDetails(task, 2)}>{task.evidence.length} 证据 ↗</button></div>
    {:else}<div class="mx-empty">{query ? '无匹配完成任务' : '暂无完成记录'}</div>{/each}
  </div>
  <div class="mx-pagination"><span>共 {filtered.length} 条</span><select aria-label="每页条数" bind:value={pageSize} onchange={() => page = 1}>{#each [20, 50, 100] as size}<option value={size}>{size}</option>{/each}</select><button disabled={currentPage === 1} onclick={() => page = currentPage - 1}>上一页</button><span>{currentPage}/{pages}</span><button disabled={currentPage === pages} onclick={() => page = currentPage + 1}>下一页</button></div>
</div>
