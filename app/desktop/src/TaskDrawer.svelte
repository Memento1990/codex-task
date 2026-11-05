<script lang="ts">
  import TaskContents from './TaskContents.svelte';
  import { done, openBlockers, status, taskTitle, type Task } from './model';
  import { compactTime, modalFocus } from './matrix-format';
  let { task, now, tab = $bindable(0), onClose, onOpenEvidence, onCopy }: {
    task: Task | null; now: number; tab?: number; onClose: () => void;
    onOpenEvidence: (reference: string) => void; onCopy: (text: string) => void;
  } = $props();
</script>

{#if task}
  <div class="mx-overlay mx-drawer-overlay">
    <button class="mx-backdrop" aria-label="关闭任务详情" tabindex="-1" onclick={onClose}></button>
    <div class="mx-drawer" role="dialog" aria-modal="true" aria-label="任务详情" tabindex="-1" use:modalFocus onkeydown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); } }}>
      <header class="mx-panel-header"><strong>任务详情</strong><span class="mx-badge">{status[task.status] || task.status}</span><button class="mx-close" aria-label="关闭详情" onclick={onClose}>×</button></header>
      <div class="mx-drawer-heading"><h2>{taskTitle(task)}</h2><div class="mx-drawer-meta"><span class="mx-badge">{done(task)}/{task.plan.length}</span><span title="自然耗时，包含等待与受阻">{compactTime(task.timing, now)}</span><button class="mx-action-button" onclick={() => onCopy(task!.session_id)}>复制会话 ID</button></div><p class="mx-reference">{task.cwd}</p></div>
      <nav class="mx-detail-tabs" aria-label="任务详情内容">{#each ['清单', `卡点 ${openBlockers(task).length}`, `证据 ${task.evidence.length}`] as label, index}<button class:active={tab === index} aria-pressed={tab === index} onclick={() => tab = index}>{label}</button>{/each}</nav>
      <div class="mx-drawer-body" data-testid="drawer-scroll"><TaskContents {task} {now} {tab} {onOpenEvidence} {onCopy}/></div>
    </div>
  </div>
{/if}
