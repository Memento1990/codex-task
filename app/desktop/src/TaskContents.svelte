<script lang="ts">
  import { done, openBlockers, safeReference, status, type Task } from './model';
  import { compactTime, firstRecorded, recordedDate } from './matrix-format';
  let { task, now, columns = false, tab = 0, onOpenEvidence, onCopy }: {
    task: Task; now: number; columns?: boolean; tab?: number;
    onOpenEvidence: (reference: string) => void; onCopy: (text: string) => void;
  } = $props();
  const blockers = $derived([...task.blockers].sort((a, b) => Number(b.open) - Number(a.open)));
  const marks: Record<string, string> = { completed: '✓', inProgress: '▸', blocked: '!', pending: '·' };
</script>

<div class="mx-task-sections" class:mx-columns={columns}>
  {#if columns || tab === 0}
    <section class="mx-section mx-plan" aria-label="任务清单">
      {#if columns}<div class="mx-column-heading"><span>清单</span><span>{done(task)}/{task.plan.length}</span></div>{/if}
      {#each task.plan as step (step.id)}
        <div class="mx-step {step.status}">
          <span class="mx-step-mark" aria-label={status[step.status] || step.status}>{marks[step.status] || '·'}</span>
          <span class="mx-step-title">{step.title}</span>
          <span class="mx-step-time" title={step.timing?.partial ? '部分计时记录' : '自然耗时'}>{compactTime(step.timing, now)}</span>
          {#if step.note}<div class="mx-step-note">{step.note}</div>{/if}
        </div>
      {:else}<div class="mx-empty">尚未汇报清单</div>{/each}
    </section>
  {/if}
  {#if columns || tab === 1}
    <section class="mx-section mx-blockers" aria-label="任务卡点">
      {#if columns}<div class="mx-column-heading"><span>卡点</span><span>{openBlockers(task).length} 未解除</span></div>{/if}
      {#each blockers as blocker (blocker.id)}
        <article class="mx-blocker" class:resolved={!blocker.open} data-blocker-id={blocker.id}>
          <div class="mx-blocker-heading"><strong>{blocker.title}</strong><span>{blocker.open ? compactTime(blocker.timing, now) : '已解除'}</span></div>
          <dl><dt>原因</dt><dd>{blocker.reason || '未填写'}</dd>
            {#if blocker.impact}<dt>影响</dt><dd>{blocker.impact}</dd>{/if}
            <dt>处理方</dt><dd>{blocker.owner || '未指定'}</dd>
            <dt>解除</dt><dd>{blocker.resolve_when || '未填写'}</dd></dl>
        </article>
      {:else}<div class="mx-empty">暂无已汇报卡点</div>{/each}
    </section>
  {/if}
  {#if columns || tab === 2}
    <section class="mx-section mx-evidence" aria-label="验收证据">
      {#if columns}<div class="mx-column-heading"><span>验收证据</span><span>{task.evidence.length}</span></div>{/if}
      {#each task.evidence as evidence}
        <div class="mx-evidence-row">
          <span class="mx-proof-icon" aria-hidden="true">◇</span>
          <div class="mx-proof-body"><div class="mx-proof-label" title={evidence.reference}>{evidence.label}</div>
            <div class="mx-proof-meta"><span class="mx-proof-time" title={`首次记录：${recordedDate(evidence.at)}`}>{firstRecorded(evidence.at, now)}</span><span class="mx-proof-actions">
              {#if safeReference(evidence.reference)}<button class="mx-text-button" onclick={() => onOpenEvidence(evidence.reference)}>打开</button>{:else}<span class="mx-proof-time" title="引用为空或格式不受支持，可复制检查">无效引用</span>{/if}
              <button class="mx-copy-proof mx-text-button" title="复制证据引用" onclick={() => onCopy(evidence.reference)}>复制</button>
            </span></div>
          </div>
        </div>
        {#if !columns}<p class="mx-reference">{evidence.reference || '未提供引用'}</p>{/if}
      {:else}<div class="mx-empty">暂无验收证据</div>{/each}
    </section>
  {/if}
</div>
