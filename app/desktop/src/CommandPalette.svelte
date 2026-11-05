<script lang="ts">
  import { isReportedTask, taskTitle, type Task } from './model';
  import { modalFocus } from './matrix-format';
  type Command = { id: string; label: string; detail?: string; run: () => void };
  let { tasks, commands, onClose, onSelect }: { tasks: Task[]; commands: Command[]; onClose: () => void; onSelect: (task: Task) => void } = $props();
  let query = $state(''), selectedIndex = $state(0), list: HTMLDivElement;
  const entries = $derived([
    ...tasks.filter(isReportedTask).map(task => ({ id: `task-${task.id}`, label: taskTitle(task), detail: [task.cwd, task.session_id].join(' · '), search: [task.current_step, ...task.plan.map(step => step.title), ...task.blockers.flatMap(blocker => [blocker.title, blocker.reason])].join(' '), kind: '任务', run: () => onSelect(task) })),
    ...commands.map(command => ({ ...command, search: '', kind: '功能' })),
  ]);
  const matches = $derived(entries.filter(entry => [entry.label, entry.detail, entry.search].join(' ').toLowerCase().includes(query.trim().toLowerCase())));
  $effect(() => { selectedIndex = Math.min(selectedIndex, Math.max(0, matches.length - 1)); });
  function run(index: number) { const entry = matches[index]; if (entry) { onClose(); entry.run(); } }
  function keys(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); }
    else if (['ArrowDown', 'ArrowUp'].includes(event.key)) { event.preventDefault(); selectedIndex = (selectedIndex + (event.key === 'ArrowDown' ? 1 : -1) + matches.length) % Math.max(matches.length, 1); queueMicrotask(() => list?.querySelector('.active')?.scrollIntoView({ block: 'nearest' })); }
    else if (event.key === 'Enter' && event.target instanceof HTMLInputElement) { event.preventDefault(); run(selectedIndex); }
  }
</script>

<div class="mx-overlay mx-command-overlay"><button class="mx-backdrop" tabindex="-1" aria-label="关闭命令搜索" onclick={onClose}></button>
  <div class="mx-command-dialog" role="dialog" aria-modal="true" aria-label="命令搜索" tabindex="-1" use:modalFocus onkeydown={keys}>
    <header class="mx-command-input"><input data-initial-focus aria-label="搜索任务、项目、命令" placeholder="搜索任务、项目、命令…" bind:value={query} oninput={() => selectedIndex = 0}><button aria-label="关闭命令搜索" onclick={onClose}>Esc</button></header>
    <div class="mx-command-list" bind:this={list}>{#each matches as entry, index (entry.id)}<button class="mx-command-entry" class:active={selectedIndex === index} onclick={() => run(index)} onpointerenter={() => selectedIndex = index}><span>{entry.label}{#if entry.detail}<small>{entry.detail}</small>{/if}</span><kbd>{entry.kind}</kbd></button>{:else}<div class="mx-empty">没有匹配的任务或命令</div>{/each}</div>
    <footer class="mx-command-footer">↑↓ 选择 · Enter 执行 · Esc 关闭</footer>
  </div>
</div>
