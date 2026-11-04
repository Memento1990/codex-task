<script lang="ts">
  import { onMount } from 'svelte';
  import { call, native } from './host';

  let {blocked=false,onbusychange=(_v:boolean)=>{}}:{blocked?:boolean;onbusychange?:(v:boolean)=>void}=$props();
  type RepairStatus='not_started'|'preparing'|'ready'|'blocked'|'failed'|'committed'|'awaiting_health'|'health_failed'|'completed';
  type RepairSnapshot={status:RepairStatus;recovered_tasks?:number;message?:string;details?:unknown};
  let snapshot=$state<RepairSnapshot|null>(null);
  let pending=$state(false),loaded=$state(false),confirm=$state(false),error=$state('');
  let revision=0,stopped=false;
  const activeStatuses:RepairStatus[]=['preparing','ready','committed','awaiting_health'];
  let active=$derived(snapshot!==null&&activeStatuses.includes(snapshot.status));
  let busy=$derived(pending||active);
  const labels:Record<RepairStatus,string>={
    not_started:'可尝试恢复任务记录',
    preparing:'正在准备修复，请稍候…',
    ready:'准备已就绪，等待看板安全重启…',
    blocked:'暂时无法安全修复',
    failed:'本次修复未完成',
    committed:'任务记录已切换，等待启动验证',
    awaiting_health:'看板已重启，正在等待验证',
    health_failed:'任务记录已切换，启动验证尚未通过',
    completed:'修复完成，启动验证已通过',
  };
  let known=$derived(snapshot!==null&&Object.hasOwn(labels,snapshot.status));
  let recoveredCount=$derived(typeof snapshot?.recovered_tasks==='number'&&Number.isSafeInteger(snapshot.recovered_tasks)&&snapshot.recovered_tasks>=0?snapshot.recovered_tasks:null);
  $effect(()=>onbusychange(busy));

  async function loadSnapshot(){
    if(!native||pending)return;
    const observed=revision;
    try{
      const next=await call<RepairSnapshot>('database_repair',{action:'snapshot'});
      if(stopped||observed!==revision)return;
      snapshot=next;loaded=true;error='';
    }catch{
      if(stopped||observed!==revision)return;
      error='暂时无法确认修复状态。原始记录仍保留，不会强行结束 Codex 或正在进行的会话。请稍后重新打开此页面。';
    }
  }
  async function repairRestart(){
    if(!confirm||busy||blocked||!native||!loaded||!known||error)return;
    ++revision;pending=true;confirm=false;error='';
    try{
      const next=await call<RepairSnapshot>('database_repair',{action:'repair_restart',confirmed:true});
      if(!stopped)snapshot=next;
    }catch{
      if(!stopped)error='未能安全完成修复。原始记录仍保留，不会强行结束 Codex 或正在进行的会话。请稍后重新打开此页面确认状态，不要删除原始记录。';
    }finally{if(!stopped)pending=false;}
  }
  function escapeConfirmation(event:KeyboardEvent){
    if(event.key==='Escape'&&confirm){event.preventDefault();event.stopPropagation();confirm=false;}
  }
  function focusConfirmation(node:HTMLDivElement){queueMicrotask(()=>node.querySelector<HTMLButtonElement>('button')?.focus());}
  onMount(()=>{
    let timer:ReturnType<typeof setTimeout>;
    async function loop(){await loadSnapshot();if(!stopped)timer=setTimeout(loop,1500);}
    if(native)void loop();
    return()=>{stopped=true;clearTimeout(timer);onbusychange(false);};
  });
</script>

<section class="repair" aria-labelledby="repair-heading" aria-busy={busy}>
  <strong id="repair-heading">修复任务记录</strong>
  <p>任务记录无法正常显示时，可尝试安全修复并重启看板。原始记录会保留，Codex 和正在进行的会话不会被强行结束。</p>
  <div class="repair-status" role="status" aria-live="polite">
    <strong>{!native?'请从桌面程序打开此功能':!loaded?'正在读取修复状态…':known?labels[snapshot!.status]:'修复状态暂时无法确认'}</strong>
    {#if snapshot?.status==='completed'}
      <p>{recoveredCount!==null?`已恢复 ${recoveredCount} 个任务，重启后的检查已通过。`:'任务记录已恢复，重启后的检查已通过。'}</p>
    {:else if snapshot?.status==='committed'||snapshot?.status==='awaiting_health'}
      <p>{recoveredCount!==null?`${recoveredCount} 个任务等待验证，目前还不能确认修复成功。`:'任务记录正在等待验证，目前还不能确认修复成功。'}原始记录仍保留。</p>
    {:else if snapshot?.status==='health_failed'}
      <p>任务记录已切换，尚未通过启动检查；不会退回可能过期的旧数据。可正常重新打开看板继续验证，原始记录仍保留。</p>
    {:else if snapshot?.status==='preparing'||snapshot?.status==='ready'}
      <p>看板会在准备完成后自动安全退出并重新打开，请勿重复操作。不会强行结束正在进行的会话。</p>
    {:else if snapshot?.status==='blocked'||snapshot?.status==='failed'}
      <p>为保护任务记录，本次操作未能安全完成。原始记录仍保留，正在进行的会话未被结束。请稍后再试，不要删除原始记录。</p>
    {:else if loaded&&!known}
      <p>原始记录仍保留，不会强行结束正在进行的会话。请稍后重新打开此页面。</p>
    {/if}
  </div>
  {#if error}<p class="repair-warning" role="alert">{error}</p>{/if}
  {#if blocked}<p class="repair-warning">请先完成当前设置、更新或数据迁移操作，再修复任务记录。原始记录仍保留，正在进行的会话不会被结束。</p>{/if}
  {#if confirm}
    <div use:focusConfirmation class="confirmation" role="group" aria-label="修复并重启确认">
      <strong>修复并重启看板？</strong>
      <p>会保留原始记录，尽可能找回可读取的任务，并在准备就绪后自动重启看板。部分历史记录可能无法找回；未保存的看板设置不会自动保存。不会强行结束 Codex 或正在进行的会话。</p>
      <div class="buttons">
        <button type="button" onkeydown={escapeConfirmation} onclick={()=>confirm=false}>取消</button>
        <button type="button" class="primary" disabled={blocked||busy||!known||!!error} onkeydown={escapeConfirmation} onclick={repairRestart}>确认修复并重启</button>
      </div>
    </div>
  {:else}
    <div class="buttons"><button type="button" class="primary" disabled={blocked||busy||!native||!loaded||!known||!!error} onclick={()=>confirm=true}>{busy?'正在修复…':'修复并重启'}</button></div>
  {/if}
</section>

<style>
  .repair{display:grid;gap:8px;border-top:1px solid var(--divider);padding-top:14px;font-size:12px;line-height:1.65}
  .repair p{margin:0;color:var(--muted);overflow-wrap:anywhere}
  .repair-status{display:grid;gap:4px}
  .buttons{display:flex;gap:8px;flex-wrap:wrap}
  .confirmation{padding:12px;border:1px solid var(--stroke);background:var(--layer);border-radius:5px;display:grid;gap:8px}
  .repair-warning{border-left:2px solid var(--stroke);padding-left:8px}
</style>
