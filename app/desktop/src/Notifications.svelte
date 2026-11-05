<script lang="ts">
  import { onMount } from 'svelte';
  import WeixinChannel from './WeixinChannel.svelte';
  import UtilityDialog from './UtilityDialog.svelte';
  import { call, native } from './host';
  import {
    filterNotificationRecords, notificationChannelNames, notificationChannelState, notificationEvents,
    notificationEventName, notificationPreview, notificationRecordState, notificationTime, safeNotificationError,
    validNotificationWebhook, type NotificationChannel, type NotificationChannelId, type NotificationEvent,
    type NotificationSnapshot,
  } from './notification-model';

  // Mount inside App's modal-body. App owns scrolling, modal chrome and its fixed close footer.
  // onbusychange reports foreground bridge requests only; delivery/polling does not lock the modal.
  let { visible = true, blocked = false, onbusychange = (_busy: boolean) => {} }:
    { visible?: boolean; blocked?: boolean; onbusychange?: (busy: boolean) => void } = $props();
  let snapshot = $state<NotificationSnapshot | null>(null);
  let weixinBusy = $state(false);
  let pending = $state(false), error = $state(''), feedback = $state('');
  let wechatOpen=$state(false), failedOnly = $state(false);
  let editing = $state<NotificationChannelId | null>(null);
  let webhook = $state(''), keyword = $state(''), signingSecret = $state('');
  let channelEnabled = $state(true), clearSigningSecret = $state(false);
  let previewEvent = $state<NotificationEvent>('blocker_opened');
  type Confirmation = { kind: 'save' | 'delete' | 'test' | 'retry' | 'clear'; id: string };
  let confirmation = $state<Confirmation | null>(null);
  let revision = 0, disposed = false, polling = false;
  let locked = $derived(pending || weixinBusy || blocked || !native || !snapshot);
  let records = $derived(filterNotificationRecords(snapshot?.records ?? [], failedOnly));
  let preview = $derived(notificationPreview(previewEvent));
  let editedChannel = $derived(snapshot?.channels.find(channel => channel.id === editing));
  let canSaveChannel = $derived(!!editing && ((!webhook.trim() && !!editedChannel?.configured) || validNotificationWebhook(editing, webhook))
    && !(clearSigningSecret && !!signingSecret.trim()));
  $effect(() => { onbusychange(pending || weixinBusy); });
  $effect(() => { if (!visible) { clearEditor(); confirmation = null; wechatOpen = false; } });

  function clearEditor() {
    editing = null; webhook = ''; signingSecret = ''; keyword = ''; clearSigningSecret = false;
  }
  function editChannel(channel: NotificationChannel) {
    clearEditor(); confirmation = null; editing = channel.id; keyword = channel.keyword;
    channelEnabled = channel.enabled; error = ''; feedback = '';
  }
  async function request(method: string, params: Record<string, unknown> = {}, message = '') {
    if (pending || blocked || !native) return;
    ++revision; pending = true; error = ''; feedback = '';
    try {
      const next = await call<NotificationSnapshot>(method, params);
      if (disposed) return;
      snapshot = next; feedback = message; confirmation = null;
      if (method === 'notification_channel_save' || method === 'notification_channel_delete') clearEditor();
    } catch {
      if (!disposed) error = '操作未完成。请检查本地通知服务、网络和平台配置后重试；真实凭据不会显示在这里。';
    } finally { if (!disposed) pending = false; }
  }
  async function poll() {
    if (!native || !visible || pending || weixinBusy || polling) return;
    const observed = revision; polling = true;
    try {
      const next = await call<NotificationSnapshot>('notification_snapshot');
      if (!disposed && observed === revision && !pending && !weixinBusy) { snapshot = next; }
    } catch {
      if (!disposed && observed === revision) error = '暂时无法刷新通知状态，请确认本地通知服务正在运行。';
    } finally { polling = false; }
  }
  function saveGlobal(enabled: boolean, event?: NotificationEvent, checked?: boolean) {
    if (!snapshot) return;
    const subscriptions = { ...snapshot.subscriptions };
    if (event) subscriptions[event] = !!checked;
    void request('notification_save', { enabled, subscriptions }, '通知设置已保存');
  }
  function ask(kind: Confirmation['kind'], id = '') { confirmation = { kind, id }; error = ''; feedback = ''; }
  async function confirmAction() {
    if (!confirmation) return;
    const { kind, id } = confirmation;
    if (kind === 'save' && editing && canSaveChannel) {
      const params: Record<string, unknown> = { id: editing, enabled: channelEnabled, keyword: keyword.trim() };
      if (webhook.trim()) params.webhook = webhook.trim();
      if (signingSecret.trim()) params.signing_secret = signingSecret.trim();
      if (clearSigningSecret) params.clear_signing_secret = true;
      await request('notification_channel_save', params, '渠道已保存。请单独发送测试；保存不代表平台验证通过。');
    } else if (kind === 'test') await request('notification_test', { id }, '测试已提交，请查看发送记录中的最终结果。');
    else if (kind === 'delete') await request('notification_channel_delete', { id }, '渠道配置已删除');
    else if (kind === 'retry') await request('notification_retry', { id }, '重试已提交，请在记录中查看最终状态；可能重复送达。');
    else if (kind === 'clear') await request('notification_clear', {}, '发送记录已清空');
  }
  onMount(() => {
    let timer: ReturnType<typeof setTimeout>;
    async function loop() { await poll(); if (!disposed) timer = setTimeout(loop, visible ? 2000 : 8000); }
    void loop();
    return () => { disposed = true; ++revision; clearTimeout(timer); onbusychange(false); };
  });
</script>

{#if visible}
<section class="notifications" aria-label="消息通知" aria-busy={pending}>
  {#if !native}<p class="utility-note warning" role="status">请从桌面应用打开，当前不会模拟连接或投递。</p>{:else if !snapshot&&!error}<p class="hint" role="status">正在读取通知设置…</p>{/if}
  {#if error}<p class="utility-note warning" role="alert">{error}</p>{/if}
  {#if snapshot?.service_error}<p class="utility-note warning" role="alert">通知服务异常，请检查连接后刷新。</p>{/if}
  {#if feedback}<p class="utility-note success" role="status">{feedback}</p>{/if}
  <section class="settings-section notification-channels"><h3 class="section-heading">通知渠道 <span>{snapshot?.channels.filter(channel=>channel.enabled).length??0} 个已启用</span></h3>
    <label class="settings-row"><span class="row-label">外部通知<small>独立于桌面浮窗通知</small></span><input type="checkbox" checked={snapshot?.enabled??false} disabled={locked} onchange={event=>{const checked=event.currentTarget.checked;event.currentTarget.checked=snapshot?.enabled??false;saveGlobal(checked);}}></label>
    {#each (['wechat','wecom','feishu'] as NotificationChannelId[]) as id}
      {@const channel=snapshot?.channels.find(c=>c.id===id)}
      <div class="channel-row"><span class="channel-icon {id}" aria-hidden="true">{{wechat:'微',wecom:'企',feishu:'飞'}[id]}</span><div class="channel-label"><strong>{notificationChannelNames[id]}</strong><small>{channel?notificationChannelState(channel):'状态未读取'}</small></div>
        <span class="status-pill" class:success={channel?.configured}>{channel?.configured?'已配置':'未配置'}</span>
        <input class="switch" type="checkbox" aria-label={`启用${notificationChannelNames[id]}`} checked={channel?.enabled??false} disabled={locked||!channel?.configured} onchange={event=>{const checked=event.currentTarget.checked;event.currentTarget.checked=channel?.enabled??false;void request('notification_channel_save',{id,enabled:checked,keyword:channel?.keyword||''});}}>
        <div class="channel-actions"><button disabled={locked||!channel} onclick={()=>{confirmation=null;if(id==='wechat'){wechatOpen=true;clearEditor();}else if(channel){wechatOpen=false;editChannel(channel);}}}>{id==='wechat'?'绑定 / 管理':'配置'}</button><button disabled={locked||!channel?.configured||!channel.enabled||!snapshot?.enabled} onclick={()=>ask('test',id)}>测试</button></div>
      </div>
    {/each}
  </section>
  <section class="settings-section subscriptions"><h3 class="section-heading">事件订阅</h3>
    {#each notificationEvents as event}<label class="settings-row"><span class="row-label">{event.name}<small>{event.detail}</small></span><input type="checkbox" checked={snapshot?.subscriptions[event.id]??false} disabled={locked} onchange={change=>{const checked=change.currentTarget.checked;change.currentTarget.checked=snapshot?.subscriptions[event.id]??false;saveGlobal(snapshot!.enabled,event.id,checked);}}></label>{/each}
  </section>
  <section class="settings-section"><h3 class="section-heading">发送记录 <span>{records.length} 条</span></h3>
    <div class="record-toolbar"><details class="record-options"><summary>筛选与管理</summary><label class="check"><input type="checkbox" bind:checked={failedOnly}>仅看失败／未知</label><button disabled={locked||!snapshot?.records.length} onclick={()=>ask('clear')}>清空已结束记录…</button></details><button disabled={!native||pending||weixinBusy||blocked} onclick={()=>request('notification_snapshot')}>刷新</button></div>
    <div class="records-table"><table><thead><tr><th>时间</th><th>渠道</th><th>事件</th><th>标题</th><th>状态</th></tr></thead><tbody>{#each records as record (record.id)}{@const state=notificationRecordState(record.status)}<tr><td><time>{notificationTime(record.created_at)}</time></td><td>{notificationChannelNames[record.channel]??'通知渠道'}</td><td>{notificationEventName(record.event)}</td><td><span class="record-title" title={record.title}>{record.title||'通知'}</span>{#if record.error}<small class="validation">{safeNotificationError(record.error)}</small>{/if}</td><td><span class="status-pill" class:success={state.tone==='success'} class:warning={state.tone==='warning'}>{state.label}</span>{#if record.attempts>1}<small>尝试 {record.attempts} 次</small>{/if}{#if record.status==='failed'}<button disabled={locked||!snapshot?.enabled} onclick={()=>ask('retry',record.id)}>重试</button>{/if}</td></tr>{:else}<tr><td colspan="5" class="empty">{failedOnly?'没有失败记录':'暂无发送记录'}</td></tr>{/each}</tbody></table></div>
  </section>
  <details class="advanced"><summary>高级配置 · 内容预览与发送策略</summary><div class="preview-select"><label>预览事件<select bind:value={previewEvent}>{#each notificationEvents as event}<option value={event.id}>{event.name}</option>{/each}</select></label></div><div class="message"><strong>{preview.title}</strong><p>{preview.body}</p></div><p class="hint">此处预览不会发送。平台受理不代表已读，手动重试可能重复送达；微信结果未知不自动重发。</p><p class="hint">退出、休眠或断网会影响投递。默认不发送路径、代码或日志。</p></details>
  {#if wechatOpen && !confirmation}<UtilityDialog title="微信 · 绑定 / 管理" locked={weixinBusy||pending||blocked} onClose={()=>wechatOpen=false}><WeixinChannel snapshot={snapshot?.weixin} enabled={snapshot?.channels.find(c=>c.id==='wechat')?.enabled??false} blocked={blocked||pending} onbusychange={value=>weixinBusy=value} onupdate={value=>{++revision;snapshot=value;}}/></UtilityDialog>{/if}
  {#if editing && !confirmation}
    <UtilityDialog title={`${notificationChannelNames[editing]} · 连接配置`} locked={pending||blocked} onClose={clearEditor}>
      <p class="hint">凭据留空保留已有值，不回显；保存不会发送测试。</p>
      {#if error}<p class="validation" role="alert">{error}</p>{/if}
      <fieldset disabled={pending || blocked}>
        <label class="check"><input type="checkbox" bind:checked={channelEnabled}>启用此渠道</label>
        <label class="field">Webhook 地址<input type="password" bind:value={webhook} autocomplete="new-password" spellcheck="false" placeholder={editedChannel?.configured ? '已有地址已保存；留空保留，输入新地址替换' : editing === 'wecom' ? 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=…' : 'https://open.feishu.cn/open-apis/bot/v2/hook/…'}><small>仅接受对应平台的 HTTPS 机器人地址，不支持自定义代理或其他域名。</small></label>
        {#if webhook.trim() && !validNotificationWebhook(editing, webhook)}<p class="validation" role="alert">地址不符合所选平台要求。请检查域名和完整机器人路径。</p>{/if}
        <label class="field">安全关键词（选填）<input bind:value={keyword} maxlength="200" autocomplete="off" placeholder="平台要求的安全关键词"><small>将加入通知文本，请勿填写密钥。</small></label>
        {#if editing === 'feishu'}<details class="advanced"><summary>高级配置 · 签名校验</summary>
          <label class="field">签名密钥（选填）<input type="password" bind:value={signingSecret} disabled={clearSigningSecret} autocomplete="new-password" spellcheck="false" placeholder={editedChannel?.has_signing_secret ? '已有密钥已保存；留空保留' : '如机器人启用了签名校验，请填写'}></label>
          <label class="check"><input type="checkbox" bind:checked={clearSigningSecret} onchange={() => { signingSecret = ''; }}>明确删除已保存的签名密钥</label>
          <p class="hint">仅在平台也关闭签名校验时删除，否则后续投递会失败。</p></details>
        {/if}
        <div class="actions"><button type="button" onclick={clearEditor}>取消</button><button type="button" class="primary" disabled={!canSaveChannel} onclick={() => ask('save')}>保存配置…</button>{#if editedChannel?.configured}<button type="button" class="danger" onclick={() => ask('delete', editing!)}>删除配置…</button>{/if}</div>
      </fieldset>
    </UtilityDialog>
  {/if}
  {#if confirmation}
    <UtilityDialog title={confirmation.kind === 'save' ? '保存渠道配置？' : confirmation.kind === 'delete' ? '删除此渠道的配置？' : confirmation.kind === 'test' ? '真实发送测试通知？' : confirmation.kind === 'retry' ? '确认重新投递？' : '清空所有发送记录？'} locked={pending||blocked} onClose={()=>confirmation=null}>
      <p>{confirmation.kind === 'save' ? (clearSigningSecret ? '将删除已保存的飞书签名密钥。保存不会发送测试，也不表示连接已经验证。' : '将应用当前配置；空白凭据保留已有值。保存后请单独发送测试，不会自动验证连接。') : confirmation.kind === 'delete' ? '将移除此渠道的本机配置和凭据，停止后续投递。已发出的消息不能撤回。' : confirmation.kind === 'test' ? '这不是模拟：将向此渠道已配置的真实接收群发送一条测试通知。请确认接收对象正确。平台受理不代表已读。' : confirmation.kind === 'retry' ? '上次请求可能已被平台受理，网络错误并不能证明未送达。手动重试可能造成重复消息，仍要发送吗？' : '仅删除本机已结束的记录，待发记录保留；不能撤回平台消息，正在投递的操作也不会因此取消。此操作不可撤销。'}</p>
      {#if error}<p class="validation" role="alert">{error}</p>{/if}
      <div class="actions"><button type="button" disabled={pending} onclick={() => confirmation = null}>取消</button><button type="button" class="primary" disabled={locked} onclick={confirmAction}>{pending ? '正在处理…' : '确认'}</button></div>
    </UtilityDialog>
  {/if}
</section>
{/if}

<style>
  .notifications{min-width:0;color:var(--text);font-size:12px;line-height:1.5}
  .notification-channels{container:notifications / inline-size}
  .channel-row{display:grid;grid-template-columns:24px minmax(90px,1fr) auto 30px auto;align-items:center;gap:8px;padding:9px 12px;border-bottom:1px solid var(--divider)}
  .channel-row:last-child{border-bottom:0}.channel-icon{display:grid;place-items:center;width:24px;height:24px;border:1px solid var(--stroke);border-radius:4px;color:var(--muted);font-size:12px}.channel-icon.wechat{color:var(--success)}.channel-icon.wecom{color:var(--mx-blue)}.channel-icon.feishu{color:var(--mx-violet)}
  .channel-label{min-width:0}.channel-label strong{font-size:12px;font-weight:400}.channel-label small{display:block;font-size:11px;color:var(--muted);margin-top:2px;overflow-wrap:anywhere}.channel-actions{display:flex;gap:6px}.channel-actions button{white-space:nowrap;font-size:11px;padding:4px 7px}
  .status-pill{display:inline-block;max-width:100%;padding:2px 6px;border:1px solid var(--stroke);border-radius:3px;color:var(--muted);font:11px/1.5 var(--mx-mono);overflow-wrap:anywhere}.status-pill.success{color:var(--success);border-color:color-mix(in srgb,var(--success) 38%,var(--divider))}.status-pill.warning{color:var(--warning);border-color:color-mix(in srgb,var(--warning) 38%,var(--divider))}
  fieldset{border:0;padding:0;margin:8px 0 0;min-width:0}.field{display:grid;gap:5px;margin:11px 0}.field input{width:100%;min-width:0}.field small,.hint{color:var(--muted);font-size:11px;line-height:1.65}.check{display:flex;align-items:center;gap:7px;font-size:12px}.actions{display:flex;justify-content:flex-end;gap:7px;flex-wrap:wrap;margin-top:14px}.validation{color:var(--warning);font-size:11px;line-height:1.65;margin-top:7px;overflow-wrap:anywhere}
  .record-toolbar{display:flex;align-items:flex-start;justify-content:space-between;gap:9px;padding:7px 12px;border-bottom:1px solid var(--divider)}.record-toolbar>button{font-size:11px;padding:3px 7px;min-height:25px}.record-options{font-size:11px}.record-options summary,.advanced summary{cursor:pointer;color:var(--muted)}.record-options>button{font-size:11px}
  .records-table{overflow:auto}.records-table table{border-collapse:collapse;width:100%;min-width:560px;table-layout:fixed;font-size:12px;text-align:left}.records-table th{font:10px var(--mx-mono);letter-spacing:.04em;color:var(--muted);font-weight:400}.records-table td,.records-table th{padding:7px 12px;border-bottom:1px solid var(--divider);vertical-align:top;overflow-wrap:anywhere}.records-table tr:last-child td{border-bottom:0}.records-table th:nth-child(1){width:19%}.records-table th:nth-child(2){width:13%}.records-table th:nth-child(3){width:13%}.records-table th:nth-child(4){width:25%}.records-table th:nth-child(5){width:30%}.records-table time{font:11px/1.5 var(--mx-mono);color:var(--muted)}.record-title{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.records-table small{display:block;color:var(--muted);font-size:11px;overflow-wrap:anywhere}.records-table .validation{color:var(--warning)}.records-table td:last-child button{display:block;padding:2px 5px;min-height:22px;font-size:11px;margin-top:4px}.empty{text-align:center;color:var(--muted);padding:15px!important}
  .advanced{font-size:12px;border-top:1px solid var(--divider);padding-top:10px}.advanced p{margin-top:7px}.preview-select{margin:9px 0}.preview-select label{display:flex;align-items:center;gap:9px}.message{padding:10px;border-left:2px solid var(--accent);background:var(--layer);font-size:12px}.message p{margin-top:5px}
  @container notifications (max-width:460px){.channel-row{grid-template-columns:24px minmax(0,1fr) auto 30px}.channel-actions{grid-column:2/-1;justify-content:flex-end}}
</style>
