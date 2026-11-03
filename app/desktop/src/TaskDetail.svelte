<script lang="ts">
  import { tick } from 'svelte';
  import { done, elapsed, taskTitle, openBlockers, runtime, safeReference, status, type Task } from './model';
  import { action, native } from './host';
  let { task, tab = $bindable(0), now, completedOnly = false, onCompleted = () => {} }: {task: Task; tab?: number; now: number; completedOnly?: boolean; onCompleted?: () => void} = $props();
  const tabs = ['清单', '卡点', '证据'];
  let steps = $derived(task.plan.filter(step => completedOnly ? step.status === 'completed' : step.status !== 'completed'));
  let blockers = $derived([...task.blockers].sort((a,b) => Number(b.open)-Number(a.open)));
  let scroll: HTMLDivElement, previous = '';
  $effect(() => {const key=`${task.id}|${tab}|${completedOnly}`;if(key!==previous){previous=key;void tick().then(()=>{if(scroll)scroll.scrollTop=0;});}});
  function open(reference: string) { if (native) void action('open_reference', reference); }
</script>
<section class="detail">
  <div class="detail-heading"><div class="detail-title"><span class="state-dot {task.status}"></span><h1 title={task.title}>{taskTitle(task)}</h1><span class="tag {task.status}">{status[task.status]}</span></div>
    <div class="detail-meta"><span title={task.cwd}>{task.cwd.split(/[\\/]/).filter(Boolean).at(-1) || task.host_id}</span><span>Codex {runtime[task.runtime]}</span><span>{done(task)}/{task.plan.length}</span><span class="elapsed" title="自然经过时间，包含等待与受阻">耗时 {elapsed(task.timing,now)}</span></div>
    {#if task.current_step}<p class="current-step">{task.current_step}</p>{/if}
    <div class="progress"><i class:complete={task.status === 'completed'} style:width={`${task.plan.length ? done(task)/task.plan.length*100 : 0}%`}></i></div>
  </div>
  <nav class="tabs" aria-label="任务详情">
    {#each tabs as name,i}<button class:active={tab === i} onclick={() => tab = i}>{name}{#if i === 0}<small>{completedOnly ? done(task) : `${done(task)}/${task.plan.length}`}</small>{:else if i === 1 && openBlockers(task).length}<small class="warning-count">{openBlockers(task).length}</small>{/if}</button>{/each}
  </nav>
  <div class="detail-scroll" data-testid="detail-scroll" bind:this={scroll}>
    {#if tab === 0}
      {#each steps as step (step.id)}<article class="step-row"><span class="step-mark {step.status}" aria-label={status[step.status]}>{step.status === 'completed' ? '✓' : step.status === 'blocked' ? '!' : '○'}</span><div class="step-content"><div>{step.title}</div>{#if step.note}<p>{step.note}</p>{/if}</div><div class="step-side"><span class={step.status}>{status[step.status]}</span><small>{elapsed(step.timing,now)}</small></div></article>{/each}
      {#if !steps.length}<div class="empty">{completedOnly ? '暂无已完成清单' : task.plan.length ? '当前清单已完成' : '尚未汇报清单'}</div>{/if}
      {#if !completedOnly && done(task)}<button class="completed-entry" onclick={onCompleted}><span>已完成清单 <b>{done(task)}</b></span><span>查看 <i>↗</i></span></button>{/if}
    {:else if tab === 1}
      {#each blockers as blocker (blocker.id)}<article class="blocker" class:resolved={!blocker.open}><div class="blocker-top"><strong>{blocker.title}</strong><span class="tag" class:blocked={blocker.open}>{blocker.open ? '未解除' : '已解除'}</span></div><small class="muted">{blocker.open ? '持续' : '累计'} {elapsed(blocker.timing,now)}</small><dl><dt>原因</dt><dd>{blocker.reason || '未填写'}</dd>{#if blocker.impact}<dt>影响</dt><dd>{blocker.impact}</dd>{/if}<dt>处理方</dt><dd>{blocker.owner || '未指定'}</dd><dt>解除条件</dt><dd>{blocker.resolve_when || '未填写'}</dd></dl></article>{/each}
      {#if !blockers.length}<div class="empty"><span class="empty-check">✓</span>暂无已汇报卡点</div>{/if}
    {:else if tab === 2}
      {#each task.evidence as item}<article class="evidence"><strong>{item.label}</strong>{#if item.reference}<p>{item.reference}</p>{#if safeReference(item.reference)}<button class="text-button" onclick={() => open(item.reference)}>打开证据 ↗</button>{/if}{/if}</article>{/each}
      {#if !task.evidence.length}<div class="empty">暂无验收证据</div>{/if}
    {/if}
  </div>
</section>
