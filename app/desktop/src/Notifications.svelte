<script lang="ts">
  import { onMount } from 'svelte';
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
  let pending = $state(false), error = $state(''), feedback = $state('');
  let tab = $state<'channels' | 'records'>('channels'), failedOnly = $state(false);
  let editing = $state<NotificationChannelId | null>(null);
  let webhook = $state(''), keyword = $state(''), signingSecret = $state('');
  let channelEnabled = $state(true), clearSigningSecret = $state(false);
  let previewEvent = $state<NotificationEvent>('blocker_opened');
  type Confirmation = { kind: 'save' | 'delete' | 'test' | 'retry' | 'clear'; id: string };
  let confirmation = $state<Confirmation | null>(null);
  let revision = 0, disposed = false, polling = false;
  let locked = $derived(pending || blocked || !native || !snapshot);
  let records = $derived(filterNotificationRecords(snapshot?.records ?? [], failedOnly));
  let preview = $derived(notificationPreview(previewEvent));
  let editedChannel = $derived(snapshot?.channels.find(channel => channel.id === editing));
  let canSaveChannel = $derived(!!editing && ((!webhook.trim() && !!editedChannel?.configured) || validNotificationWebhook(editing, webhook))
    && !(clearSigningSecret && !!signingSecret.trim()));
  $effect(() => { onbusychange(pending); });
  $effect(() => { if (!visible) { clearEditor(); confirmation = null; } });

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
    if (!native || !visible || pending || polling) return;
    const observed = revision; polling = true;
    try {
      const next = await call<NotificationSnapshot>('notification_snapshot');
      if (!disposed && observed === revision && !pending) { snapshot = next; }
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
  function confirmationFocus(node: HTMLDivElement) {
    queueMicrotask(() => { node.scrollIntoView({ block: 'nearest', behavior: 'instant' }); node.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true }); });
  }
  function escapeConfirmation(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); if (!pending) confirmation = null; }
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
  <div class="overview">
    <div><strong>重要进展，不必守在屏幕前。</strong><p>集中管理外部渠道与事件。桌面浮窗保持独立。</p></div>
    <label class="master"><input type="checkbox" checked={snapshot?.enabled ?? false} disabled={locked} onchange={event => { const checked = event.currentTarget.checked; event.currentTarget.checked = snapshot?.enabled ?? false; saveGlobal(checked); }}>外部通知</label>
  </div>
  {#if !native}
    <p class="notice" role="status">当前为非桌面环境，通知不可使用。请在桌面应用中配置；这里不会模拟连接或投递。</p>
  {:else if !snapshot && !error}<p class="hint" role="status">正在读取通知设置…</p>{/if}
  {#if snapshot && !snapshot.enabled}<p class="notice">外部通知已暂停。桌面浮窗不受影响；重新启用后不会补发暂停期间的事件。</p>{/if}
  <div class="tabs" role="group" aria-label="通知视图">
    <button type="button" class:active={tab === 'channels'} aria-pressed={tab === 'channels'} onclick={() => { tab = 'channels'; confirmation = null; }}>渠道与订阅</button>
    <button type="button" class:active={tab === 'records'} aria-pressed={tab === 'records'} onclick={() => { tab = 'records'; confirmation = null; }}>发送记录{snapshot ? ` · ${snapshot.records.length}` : ''}</button>
    <button type="button" class="refresh" disabled={!native || pending || blocked} onclick={() => request('notification_snapshot', {}, '通知状态已刷新')}>刷新</button>
  </div>
  {#if error}<p class="notice" role="alert">{error}</p>{/if}
  {#if snapshot?.service_error}<p class="notice" role="alert">本地通知服务异常，可能无法投递。请检查本地服务状态后刷新；此处不显示原始错误或凭据。</p>{/if}
  {#if feedback}<p class="feedback" role="status">{feedback}</p>{/if}
  {#if tab === 'channels'}
    <div class="section-title"><h2>通知渠道</h2><span>可同时启用多个渠道</span></div>
    <div class="channels">
      {#each (['wechat', 'wecom', 'feishu'] as NotificationChannelId[]) as id}
        {@const channel = snapshot?.channels.find(item => item.id === id)}
        <article class="channel" class:unavailable={id === 'wechat'}>
          <div class="channel-top"><span class="glyph" aria-hidden="true">{id === 'wechat' ? '微' : id === 'wecom' ? '企' : '飞'}</span><h3>{notificationChannelNames[id]}</h3></div>
          <p class="hint">{id === 'wechat' ? '个人微信 · 协议尚未选定' : '群机器人 · Webhook'}</p>
          <p class="channel-state">{id === 'wechat' ? '暂不可用 · 待接入协议' : channel ? notificationChannelState(channel) : '状态未读取'}</p>
          {#if id === 'wechat'}<p class="hint">不提供扫码、二维码或在线状态。建议先使用企微或飞书。</p>
          {:else}<div class="actions"><button type="button" disabled={locked || !channel || !!editing} onclick={() => channel && editChannel(channel)}>{channel?.configured ? '管理配置' : '配置 Webhook'}</button><button type="button" disabled={locked || !channel?.configured || !channel.enabled || !snapshot?.enabled} onclick={() => ask('test', id)}>发送测试</button></div>{/if}
        </article>
      {/each}
    </div>
    {#if editing}
      <section class="editor" aria-label="渠道配置">
        <div class="section-title"><h2>{notificationChannelNames[editing]} · 连接配置</h2><button type="button" disabled={pending} onclick={() => { clearEditor(); confirmation = null; }}>取消编辑</button></div>
        <p class="hint">凭据仅用于本机投递，不会回填或回显。空白表示保留已有凭据；更换 Webhook 不会自动清除签名密钥。取消编辑或关闭设置会丢弃未保存输入；切换发送记录会保留当前输入。</p>
        <fieldset disabled={pending || blocked || !!confirmation}>
          <label class="check"><input type="checkbox" bind:checked={channelEnabled}>启用此渠道</label>
          <label class="field">Webhook 地址<input type="password" bind:value={webhook} autocomplete="new-password" spellcheck="false" placeholder={editedChannel?.configured ? '已有地址已保存；留空保留，输入新地址替换' : editing === 'wecom' ? 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=…' : 'https://open.feishu.cn/open-apis/bot/v2/hook/…'}><small>仅接受对应平台的 HTTPS 机器人地址，不支持自定义代理或其他域名。</small></label>
          {#if webhook.trim() && !validNotificationWebhook(editing, webhook)}<p class="validation" role="alert">地址不符合所选平台要求。请检查域名和完整机器人路径。</p>{/if}
          <label class="field">安全关键词（选填）<input bind:value={keyword} maxlength="200" autocomplete="off" placeholder="平台要求的安全关键词"><small>将加入通知文本，请勿填写密钥。</small></label>
          {#if editing === 'feishu'}
            <label class="field">签名密钥（选填）<input type="password" bind:value={signingSecret} disabled={clearSigningSecret} autocomplete="new-password" spellcheck="false" placeholder={editedChannel?.has_signing_secret ? '已有密钥已保存；留空保留' : '如机器人启用了签名校验，请填写'}></label>
            <label class="check"><input type="checkbox" bind:checked={clearSigningSecret} onchange={() => { signingSecret = ''; }}>明确删除已保存的签名密钥</label>
            <p class="hint">仅在平台也关闭签名校验时删除，否则后续投递会失败。</p>
          {/if}
          <div class="actions"><button type="button" class="primary" disabled={!canSaveChannel} onclick={() => ask('save')}>保存配置…</button>{#if editedChannel?.configured}<button type="button" onclick={() => ask('delete', editing!)}>删除配置…</button>{/if}</div>
        </fieldset>
      </section>
    {/if}
    <section class="subscriptions"><div class="section-title"><h2>哪些事件需要通知？</h2><span>修改即时保存</span></div>
      {#each notificationEvents as event}<label class="check event"><input type="checkbox" checked={snapshot?.subscriptions[event.id] ?? false} disabled={locked} onchange={change => { const checked = change.currentTarget.checked; change.currentTarget.checked = snapshot?.subscriptions[event.id] ?? false; saveGlobal(snapshot!.enabled, event.id, checked); }}><span><strong>{event.name}</strong><small>{event.detail}</small></span></label>{/each}
      <p class="hint">所选事件发送至所有已启用且已配置的渠道。</p>
    </section>
    <section class="preview" aria-label="通知内容预览"><div class="section-title"><h2>通知预览</h2><label class="preview-select">事件<select bind:value={previewEvent}>{#each notificationEvents as event}<option value={event.id}>{event.name}</option>{/each}</select></label></div><div class="message"><strong>{preview.title}</strong><p>{preview.body}</p></div><p class="hint">示例内容，不会发送。实际通知按渠道格式呈现，包含所设安全关键词。</p></section>
    <details class="policy"><summary>发送策略与隐私</summary><p>仅本地通知进程运行且网络可用时才可能送达。退出、休眠或断网会影响投递，请勿将它用于紧急告警。</p><p>默认不发送本地路径、代码或日志；任务标题由用户提供，本身可能含敏感信息，请在命名和选定接收群时留意。</p><p>已配置不代表测试通过；平台受理不代表对方已读。失败可能自动重试，手动重试也可能重复送达。</p></details>
  {:else}
    <div class="section-title records-heading"><h2>最近发送</h2><label class="check"><input type="checkbox" bind:checked={failedOnly}>仅看失败</label><button type="button" disabled={locked || !snapshot?.records.length} onclick={() => ask('clear')}>清空记录…</button></div>
    <p class="hint">异步投递中每 2 秒刷新。平台受理不代表已读；失败可能自动重试。</p>
    {#if !records.length}<div class="empty">{failedOnly ? '没有失败记录' : '还没有发送记录'}<p>{!native ? '请在桌面应用中使用真实通知。' : '配置并启用渠道后，可发送测试查看最终结果。'}</p></div>
    {:else}<div class="records">{#each records as record (record.id)}
      {@const state = notificationRecordState(record.status)}
      <article class="record"><div class="record-head"><strong>{record.title || '通知'}</strong><span class="badge" class:success={state.tone === 'success'} class:warning={state.tone === 'warning'}>{state.label}</span></div><p class="hint">{notificationChannelNames[record.channel] ?? '通知渠道'} · {notificationEventName(record.event)} · 尝试 {record.attempts} 次</p><time class="hint">{notificationTime(record.created_at)}</time>{#if record.error}<p class="validation">{safeNotificationError(record.error)}</p>{/if}{#if record.status === 'failed'}<div class="actions"><button type="button" disabled={locked || !snapshot?.enabled} onclick={() => ask('retry', record.id)}>手动重试…</button></div>{/if}</article>
    {/each}</div>{/if}
  {/if}
  {#if confirmation}
    <div class="confirmation" use:confirmationFocus role="group" aria-label="确认通知操作">
      <strong>{confirmation.kind === 'save' ? '保存渠道配置？' : confirmation.kind === 'delete' ? '删除此渠道的配置？' : confirmation.kind === 'test' ? '真实发送测试通知？' : confirmation.kind === 'retry' ? '确认重新投递？' : '清空所有发送记录？'}</strong>
      <p>{confirmation.kind === 'save' ? (clearSigningSecret ? '将删除已保存的飞书签名密钥。保存不会发送测试，也不表示连接已经验证。' : '将应用当前配置；空白凭据保留已有值。保存后请单独发送测试，不会自动验证连接。') : confirmation.kind === 'delete' ? '将移除此渠道的本机配置和凭据，停止后续投递。已发出的消息不能撤回。' : confirmation.kind === 'test' ? '这不是模拟：将向此渠道已配置的真实接收群发送一条测试通知。请确认接收对象正确。平台受理不代表已读。' : confirmation.kind === 'retry' ? '上次请求可能已被平台受理，网络错误并不能证明未送达。手动重试可能造成重复消息，仍要发送吗？' : '仅删除本机已结束的记录，待发记录保留；不能撤回平台消息，正在投递的操作也不会因此取消。此操作不可撤销。'}</p>
      <div class="actions"><button type="button" disabled={pending} onkeydown={escapeConfirmation} onclick={() => confirmation = null}>取消</button><button type="button" class="primary" disabled={locked} onkeydown={escapeConfirmation} onclick={confirmAction}>{pending ? '正在处理…' : '确认'}</button></div>
    </div>
  {/if}
</section>
{/if}

<style>
  .notifications{min-width:0;color:var(--text);line-height:1.6;container-type:inline-size}
  .notifications h2,.notifications h3{margin:0;font-size:12px;font-weight:600}.notifications h3{font-size:13px}
  .overview{display:flex;align-items:flex-start;justify-content:space-between;gap:12px}.overview>div{min-width:0}.overview strong{font-size:14px}.overview p,.hint{color:var(--muted);font-size:11px;line-height:1.65}.overview p{margin-top:4px}.master{display:flex;gap:5px;align-items:center;flex:none;white-space:nowrap}
  .notice,.confirmation{border:1px solid var(--divider);border-left:3px solid var(--warning);background:var(--warning-bg);padding:10px 11px;border-radius:4px;margin:10px 0;overflow-wrap:anywhere}.notice{color:var(--warning)}.feedback{padding:8px 0;color:var(--success);overflow-wrap:anywhere}
  .tabs{display:flex;align-items:center;gap:5px;border-bottom:1px solid var(--divider);margin:13px 0}.tabs button{border-radius:4px 4px 0 0;border-bottom:2px solid transparent;padding:7px 9px}.tabs .active{color:var(--accent);border-bottom-color:var(--accent);background:var(--selected)}.tabs .refresh{margin-left:auto;padding-left:6px;padding-right:6px}
  .section-title{display:flex;align-items:center;justify-content:space-between;gap:8px;margin-bottom:9px;flex-wrap:wrap}.section-title>span{font-size:11px;color:var(--muted)}.channels{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:8px}.channel{min-width:0;border:1px solid var(--divider);border-radius:5px;padding:11px;background:var(--layer)}.channel-top{display:flex;align-items:center;gap:8px}.glyph{display:grid;place-items:center;width:27px;height:27px;background:var(--selected);border:1px solid var(--divider);border-radius:5px;color:var(--accent);font-weight:600;flex:none}.unavailable .glyph{color:var(--muted);background:var(--canvas)}.channel>.hint{margin-top:6px}.channel-state{font-size:11px;margin-top:9px;overflow-wrap:anywhere}.actions{display:flex;gap:5px;flex-wrap:wrap;margin-top:10px}.actions>button{border-color:var(--stroke)}.actions>button.primary{border-color:var(--accent)}
  .editor{padding:12px;border:1px solid var(--accent);border-radius:5px;background:var(--layer);margin-top:12px}.editor fieldset{border:0;padding:0;margin:10px 0 0;min-width:0}.field{display:grid;gap:5px;margin:11px 0}.field input{width:100%;min-width:0}.field small{color:var(--muted);line-height:1.6}.check{display:flex;align-items:flex-start;gap:7px}.check input{flex:none;margin:3px 0 0}.validation{font-size:11px;color:var(--warning);margin-top:6px;overflow-wrap:anywhere}.subscriptions{padding-top:16px;margin-top:14px;border-top:1px solid var(--divider)}.event{padding:6px 0}.event small{display:block;color:var(--muted)}.subscriptions>.hint{margin-top:6px}.preview{background:var(--canvas);border:1px solid var(--divider);border-radius:5px;padding:12px;margin-top:15px}.preview-select{display:flex;align-items:center;gap:7px;font-size:11px;color:var(--muted)}.preview-select select{max-width:140px;padding:3px 5px}.message{padding:10px;background:var(--surface);border-left:2px solid var(--accent);border-radius:2px;margin-bottom:8px}.message p{margin-top:5px;font-size:11px}.policy{margin-top:14px;font-size:11px;color:var(--muted)}.policy summary{cursor:pointer;color:var(--text);padding:4px 0}.policy p{margin-top:6px}.policy summary:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
  .records-heading{justify-content:flex-start}.records-heading>h2{margin-right:auto}.empty{text-align:center;padding:28px 10px;background:var(--canvas);border:1px dashed var(--stroke);border-radius:5px;margin-top:12px}.empty p{color:var(--muted);font-size:11px;margin-top:5px}.records{display:grid;gap:8px;margin-top:10px}.record{padding:11px;border:1px solid var(--divider);border-radius:5px;background:var(--layer);min-width:0}.record-head{display:flex;gap:8px;align-items:flex-start;justify-content:space-between;flex-wrap:wrap}.record-head strong{overflow-wrap:anywhere;min-width:0;flex:1 1 130px}.badge{font-size:10px;color:var(--muted);border:1px solid var(--divider);padding:1px 5px;border-radius:3px}.badge.success{color:var(--success)}.badge.warning{color:var(--warning);background:var(--warning-bg)}.record .hint{margin-top:4px}.record time{display:block}.confirmation p{margin-top:6px}.confirmation .actions{justify-content:flex-end}
  @container(max-width:460px){.channels{grid-template-columns:1fr}.channel{display:grid;grid-template-columns:minmax(0,1fr) auto;column-gap:8px}.channel-top,.channel-state,.channel>.hint{grid-column:1}.channel .actions{grid-column:2;grid-row:1 / 4;align-self:center;flex-direction:column;margin-top:0}.unavailable>.hint:last-child{grid-column:1 / -1}.overview{flex-wrap:wrap;gap:7px}.master{margin-left:auto}.editor{padding:10px}.section-title{gap:6px}.records-heading .check{font-size:11px}}
</style>
