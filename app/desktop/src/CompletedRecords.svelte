<script lang="ts">
  import { done, isReportedTask, matchesTask, taskTitle, type Task } from './model';
  import { compactTime, recordedDate } from './matrix-format';
  let { tasks, now, query = $bindable(''), page = $bindable(1), pageSize = $bindable(20), scopeTask = '', onDetails }: {
    tasks: Task[]; now: number; query?: string; page?: number; pageSize?: number; scopeTask?: string;
    onDetails: (task: Task, tab?: number) => void;
  } = $props();
  const filtered = $derived(tasks.filter(task => isReportedTask(task) && task.status === 'completed' && (!scopeTask || task.id === scopeTask || task.source_task_id === scopeTask) && matchesTask(task, query)));
  const pages = $derived(Math.max(1, Math.ceil(filtered.length / pageSize)));
  const currentPage = $derived(Math.max(1, Math.min(page, pages)));
  const visible = $derived(filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize));
  $effect(() => { if (page !== currentPage) page = currentPage; });
</script>

<div class="mx-history" data-testid="completed-records">
  <div class="mx-searchbar"><input aria-label="搜索完成任务" placeholder="搜索完成任务" bind:value={query} oninput={() => page = 1}><span class="mx-result-count">{filtered.length} 项</span></div>
  <div class="mx-history-scroll"><table class="mx-history-table"><thead><tr><th>任务</th><th>步骤</th><th>耗时</th><th>证据</th><th>结束</th></tr></thead><tbody>
    {#each visible as task (task.id)}<tr tabindex="0" role="button" aria-label={`查看 ${taskTitle(task)}`} onclick={() => onDetails(task, 0)} onkeydown={event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); onDetails(task, 0); } }}><td><span>{taskTitle(task)}</span><small>{task.cwd}</small></td><td>{done(task)}/{task.plan.length}</td><td>{compactTime(task.timing, now)}</td><td>{task.evidence.length}</td><td>{recordedDate(task.timing?.ended_at)}</td></tr>
    {:else}<tr><td colspan="5" class="mx-empty">{query ? '无匹配完成任务' : '暂无完成记录'}</td></tr>{/each}
  </tbody></table></div>
  <div class="mx-pagination"><span>共 {filtered.length} 条</span><select aria-label="每页条数" bind:value={pageSize} onchange={() => page = 1}>{#each [20, 50, 100] as size}<option value={size}>{size}</option>{/each}</select><button disabled={currentPage === 1} onclick={() => page = currentPage - 1}>上一页</button><span>{currentPage}/{pages}</span><button disabled={currentPage === pages} onclick={() => page = currentPage + 1}>下一页</button></div>
</div>
