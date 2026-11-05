<script lang="ts">
  import { onMount } from 'svelte';
  import { call, native } from './host';
  import UtilityDialog from './UtilityDialog.svelte';

  let {blocked=false,dataDirectory='',onbusychange=(_v:boolean)=>{}}:{blocked?:boolean;dataDirectory?:string;onbusychange?:(v:boolean)=>void}=$props();
  type RepairStatus='not_started'|'preparing'|'ready'|'blocked'|'failed'|'committed'|'awaiting_health'|'health_failed'|'completed';
  type RepairSnapshot={status:RepairStatus;source?:string;recovered_tasks?:number;recovery_mode?:'full'|'partial';history_recovered?:boolean;timing_recovered?:boolean;message?:string;last_attempt?:{message?:string;reason?:string};details?:unknown};
  let snapshot=$state<RepairSnapshot|null>(null);
  let pending=$state(false),loaded=$state(false),confirm=$state(false),error=$state(''),actionError=$state('');
  let revision=0,stopped=false;
  const activeStatuses:RepairStatus[]=['preparing','ready','committed','awaiting_health'];
  let active=$derived(snapshot!==null&&activeStatuses.includes(snapshot.status));
  let busy=$derived(pending||active);
  const labels:Record<RepairStatus,string>={
    not_started:'可验证并完整恢复任务记录',
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
  function reason(value:unknown){return (value instanceof Error?value.message:String(value)).replace(/[\u0000-\u001f\u007f]/g,' ').slice(0,600);}
  async function repairRestart(){
    if(!confirm||busy||blocked||!native||!loaded||!known||error)return;
    ++revision;pending=true;confirm=false;error='';actionError='';
    try{
      const next=await call<RepairSnapshot>('database_repair',{action:'repair_restart',confirmed:true});
      if(!stopped)snapshot=next;
    }catch(cause){
      if(!stopped)actionError='本次修复未完成：'+reason(cause);
    }finally{if(!stopped)pending=false;}
  }
  onMount(()=>{
    let timer:ReturnType<typeof setTimeout>;
    async function loop(){await loadSnapshot();if(!stopped)timer=setTimeout(loop,1500);}
    if(native)void loop();
    return()=>{stopped=true;clearTimeout(timer);onbusychange(false);};
  });
</script>

<section class="repair" aria-labelledby="repair-heading" aria-busy={busy}>
  <p class="utility-note">任务记录无法正常显示时，可验证并恢复记录后安全重启看板。原始记录会保留，Codex 和正在进行的会话不会被强行结束。</p>
  <section class="settings-section"><h3 id="repair-heading">修复状态</h3>
  <div class="settings-row repair-status" role="status" aria-live="polite"><div class="row-label">
    <strong>{!native?'请从桌面程序打开此功能':!loaded?'正在读取修复状态…':known?labels[snapshot!.status]:'修复状态暂时无法确认'}</strong>
    {#if snapshot?.status==='completed'}
      <p>{recoveredCount!==null?`已恢复 ${recoveredCount} 个任务，重启后的检查已通过。`:'任务记录已恢复，重启后的检查已通过。'}</p>
      {#if snapshot.recovery_mode==='full'}<p>事件、耗时和历史归档均已保留。</p>{:else}<p>{snapshot.message||'此前为部分恢复，历史记录与耗时可能不完整。'}</p>{/if}
    {:else if snapshot?.status==='committed'||snapshot?.status==='awaiting_health'}
      <p>{recoveredCount!==null?`${recoveredCount} 个任务等待验证，目前还不能确认修复成功。`:'任务记录正在等待验证，目前还不能确认修复成功。'}原始记录仍保留。</p>
    {:else if snapshot?.status==='health_failed'}
      <p>任务记录已切换，尚未通过启动检查；不会退回可能过期的旧数据。可正常重新打开看板继续验证，原始记录仍保留。</p>
    {:else if snapshot?.status==='preparing'||snapshot?.status==='ready'}
      <p>看板会在准备完成后自动安全退出并重新打开，请勿重复操作。不会强行结束正在进行的会话。</p>
    {:else if snapshot?.status==='blocked'||snapshot?.status==='failed'}
      <p>{snapshot?.message||'为保护任务记录，本次操作未能安全完成。原始记录仍保留，正在进行的会话未被结束。不要删除原始记录。'}</p>
    {:else if loaded&&!known}
      <p>原始记录仍保留，不会强行结束正在进行的会话。请稍后重新打开此页面。</p>
    {/if}
  </div></div>
  <div class="settings-row"><span class="row-label">原始记录目录</span><code class="repair-directory" title={snapshot?.source||dataDirectory}>{snapshot?.source||dataDirectory||'目录暂不可用'}</code></div>
  </section>
  {#if error}<p class="utility-note warning" role="alert">{error}</p>{/if}
  {#if actionError}<p class="utility-note warning" role="alert">{actionError}</p>{:else if snapshot?.last_attempt}<p class="utility-note warning" role="alert">{snapshot.last_attempt.message||snapshot.last_attempt.reason}</p>{/if}
  {#if blocked}<p class="utility-note warning">请先完成当前设置、更新或数据迁移操作，再修复任务记录。原始记录仍保留，正在进行的会话不会被结束。</p>{/if}
  <p class="utility-note warning">仅在完整验证通过后切换记录。部分抢救只保留副本并停止，不自动启用。请勿删除原始记录目录。</p>
  <div class="buttons"><button type="button" class="primary" disabled={blocked||busy||!native||!loaded||!known||!!error} onclick={()=>confirm=true}>{busy?'正在修复…':'修复并重启'}</button></div>
  {#if confirm}
    <UtilityDialog title="修复并重启看板？" locked={busy||blocked} onClose={()=>confirm=false}>
      <p>仅在完整验证通过后切换，保留事件、耗时和历史归档。如只能部分抢救，会保留副本并停止，不自动启用。准备就绪后看板会安全重启，不会强行结束 Codex 或正在进行的会话。</p>
      <div class="buttons">
        <button type="button" disabled={busy} onclick={()=>confirm=false}>取消</button>
        <button type="button" class="primary" disabled={blocked||busy||!known||!!error} onclick={repairRestart}>确认修复并重启</button>
      </div>
    </UtilityDialog>
  {/if}
</section>

<style>
  .repair{min-width:0;font-size:12px;line-height:1.65}
  .repair p{overflow-wrap:anywhere}
  .repair-status strong{display:block;font-size:13px;font-weight:500;color:var(--text)}
  .repair-status p{margin:4px 0 0;color:var(--muted)}
  .repair-directory{max-width:65%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--muted);font:11px var(--mx-mono)}
  .buttons{display:flex;gap:8px;flex-wrap:wrap;margin-top:12px}
</style>
