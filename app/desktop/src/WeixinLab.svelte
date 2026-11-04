<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import QRCode from 'qrcode';
  import { native } from './host';
  import { weixinLabRemaining, weixinLabStatus, weixinLabStep, weixinLabTime, weixinQrPayload, weixinTestStatus, validWeixinVerifyCode, type WeixinLabSnapshot } from './weixin-lab-model';
  // App owns modal title, scrolling and fixed close footer. Only foreground IPC locks it.
  let { visible = true, blocked = false, onbusychange = (_value: boolean) => {} }:
    { visible?: boolean; blocked?: boolean; onbusychange?: (value: boolean) => void } = $props();
  const development = native && !!(window as unknown as { __FLOATBOARD_DEVELOPMENT__?: boolean }).__FLOATBOARD_DEVELOPMENT__;
  let snapshot = $state<WeixinLabSnapshot | null>(null);
  let pending = $state(false), error = $state(''), feedback = $state(''), code = $state('');
  let confirmation = $state<'test' | 'disconnect' | null>(null);
  let now = $state(Date.now()), qrImage = $state(''), qrError = $state('');
  let disposed = false, polling = false, revision = 0;
  let presentation = $derived(weixinLabStatus(snapshot));
  let step = $derived(weixinLabStep(snapshot));
  let locked = $derived(pending || blocked || !development || !snapshot);
  let qrPayload = $derived(weixinQrPayload(snapshot?.qrcode_img_content));
  let loginActive = $derived(!!snapshot && ['starting_login', 'waiting_scan', 'scanned', 'need_verifycode'].includes(snapshot.status));
  let expiry = $derived(weixinLabRemaining(snapshot?.qr_expires_at, now));
  let bindExpiry = $derived(weixinLabRemaining(snapshot?.binding_expires_at, now));
  $effect(() => { onbusychange(pending); });
  $effect(() => { if (!visible) { confirmation = null; code = ''; } });
  $effect(() => {
    const payload = qrPayload; let cancelled = false; qrImage = ''; qrError = '';
    if (payload) void QRCode.toDataURL(payload, { width: 400, margin: 4, errorCorrectionLevel: 'M' }).then(image => {
      if (!cancelled && !disposed) qrImage = image;
    }).catch(() => { if (!cancelled && !disposed) qrError = '二维码无法在本机生成，请刷新二维码后重试。'; });
    return () => { cancelled = true; };
  });
  async function request(action: string, params: Record<string, unknown> = {}, message = '') {
    if (locked) return;
    ++revision; pending = true; error = ''; feedback = '';
    try {
      const next = await invoke<WeixinLabSnapshot>('weixin_lab_action', { action, params });
      if (disposed) return;
      snapshot = next; feedback = message; confirmation = null; code = '';
    } catch { if (!disposed) error = '操作未完成，请检查隔离实验室服务或网络后重试。身份凭据不会显示在这里。'; }
    finally { if (!disposed) pending = false; }
  }
  async function poll() {
    if (!development || !visible || pending || polling) return;
    const observed = revision; polling = true;
    try {
      const next = await invoke<WeixinLabSnapshot>('weixin_lab_action', { action: 'snapshot', params: {} });
      if (!disposed && visible && observed === revision && !pending) snapshot = next;
    } catch { if (!disposed && observed === revision) error = '暂时无法读取实验室状态，请确认隔离开发实例正在运行。'; }
    finally { polling = false; }
  }
  function focusConfirmation(node: HTMLDivElement) { queueMicrotask(() => node.querySelector<HTMLButtonElement>('button')?.focus()); }
  function escapeConfirmation(event: KeyboardEvent) { if (event.key === 'Escape' && confirmation) { event.preventDefault(); event.stopPropagation(); if (!pending) confirmation = null; } }
  onMount(() => {
    disposed = false; let timer: ReturnType<typeof setTimeout>;
    async function loop() { now = Date.now(); await poll(); if (!disposed) timer = setTimeout(loop, 1000); }
    void loop(); return () => { disposed = true; ++revision; clearTimeout(timer); onbusychange(false); };
  });
</script>

{#if visible}
<section class="weixin-lab" aria-label="微信连接实验室">
  <header class="intro"><div><strong>微信连接实验室</strong><span class="tag">DEV · 隔离验证</span></div><p>只验证扫码、绑定与文本测试。单账号、单收件人，不接任务事件，也不执行微信指令。</p></header>
  <ol class="steps" aria-label="连接步骤">{#each ['登录', '绑定', '测试'] as label, index}<li class:active={step === index} class:done={step > index} aria-current={step === index ? 'step' : undefined}><span>{index + 1}</span>{label}</li>{/each}</ol>
  {#if !development}<div class="notice" role="status"><strong>仅隔离开发实例可用</strong><p>此页面不会在普通浏览器或正式安装版建立微信连接。</p></div>
  {:else}
    <div class="status" role="status"><strong>{presentation.title}</strong><p>{presentation.detail}</p>{#if snapshot?.busy}<p class="hint">后台操作进行中；关闭页面不会结束实验室连接。</p>{/if}</div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if snapshot?.error}<p class="error" role="status">服务报告连接异常，请检查网络、会话状态或重新连接。</p>{/if}
    {#if feedback}<p class="feedback" role="status">{feedback}</p>{/if}
    {#if snapshot}
      {#if snapshot.status === 'restored'}
        <div class="notice"><strong>恢复前需要你的同意</strong><p>恢复后将轮询此实验室账号的新消息。不会读取 OpenClaw 配置，不会自动发送测试。</p><button class="primary" disabled={locked} onclick={() => request('resume', {}, '已请求恢复连接')}>恢复连接</button></div>
      {:else if loginActive || snapshot.status === 'expired'}
        <div class="login-panel">
          <div class="qr" aria-label="微信登录二维码">
            {#if qrImage && snapshot.status !== 'expired' && expiry !== '已过期'}<img src={qrImage} alt="使用微信扫描此登录二维码" width="200" height="200">
            {:else}<div class="qr-placeholder"><strong>{snapshot.status === 'expired' || expiry === '已过期' ? '二维码已过期' : snapshot.status === 'starting_login' ? '正在获取二维码…' : '二维码暂不可用'}</strong><span>{qrError || '仅展示服务返回并在本机生成的真实二维码'}</span></div>{/if}
          </div>
          <div class="login-copy"><strong>用微信扫码连接</strong><p>登录凭据仅保存在隔离实验室，由 Windows 当前用户加密保护。</p>{#if expiry}<p class="hint">二维码有效期：{expiry}</p>{/if}<p class="hint">请勿把二维码或验证码分享给他人。若手机提示过期，可随时刷新。</p><div class="actions"><button class="primary" disabled={locked || snapshot.busy} onclick={() => request('start_login')}>刷新二维码</button><button disabled={locked} onclick={() => request('cancel_login', {}, '登录已取消')}>取消登录</button></div></div>
        </div>
        {#if snapshot.status === 'need_verifycode'}<form class="verify" onsubmit={event => { event.preventDefault(); if (validWeixinVerifyCode(code)) void request('verify_code', { code: code.trim() }, '验证码已提交'); }}><label for="weixin-verify-code">手机显示的验证码</label><div class="actions"><input id="weixin-verify-code" type="text" inputmode="numeric" autocomplete="one-time-code" maxlength="16" bind:value={code} disabled={locked}><button class="primary" disabled={locked || !validWeixinVerifyCode(code)}>提交验证码</button></div><p class="hint">验证码不会保留在页面中。</p></form>{/if}
      {:else if snapshot.status === 'binding' || snapshot.status === 'logged_in'}
        <div class="binding"><strong>在微信中发送一次性绑定码</strong><p>请从希望接收通知的会话向机器人发送完整内容。仅核对通过的消息会绑定，不会采用“谁先发就绑定谁”。</p>{#if snapshot.binding_code && bindExpiry !== '已过期'}<code class="bind-code">{snapshot.binding_code}</code>{#if bindExpiry}<p class="hint">剩余有效期：{bindExpiry}</p>{/if}{:else}<p class="hint">{bindExpiry === '已过期' ? '绑定码已过期，请重新生成。' : '绑定码尚未生成。'}</p>{/if}<button disabled={locked || !!snapshot.busy} onclick={() => request('start_binding', {}, '已请求生成新的绑定码')}>{snapshot.binding_code ? '重新生成绑定码' : '生成绑定码'}</button></div>
      {:else if snapshot.status === 'bound' || snapshot.status === 'sending'}
        <div class="recipient"><div><strong>已绑定收件人</strong><span>{snapshot.recipient_masked || '身份已绑定 · 不显示原始标识'}</span></div><dl><div><dt>最近接收</dt><dd>{weixinLabTime(snapshot.last_received_at)}</dd></div><div><dt>最近发送</dt><dd>{weixinLabTime(snapshot.last_sent_at)}</dd></div></dl></div>
        <div class="test"><div><strong>发送一条文本测试</strong><p>测试只发送实验室说明，不包含任务标题、工作目录或历史消息。</p><span class="hint" role="status">{weixinTestStatus(snapshot.last_test_status)}</span></div><button class="primary" disabled={locked || snapshot.last_test_status === 'pending' || !!snapshot.busy} onclick={() => confirmation = 'test'}>发送测试通知</button></div>
      {:else if snapshot.status === 'paused'}<button class="primary" disabled={locked} onclick={() => request('resume', {}, '已请求恢复连接')}>恢复连接</button>
      {:else}<button class="primary" disabled={locked || snapshot.busy} onclick={() => snapshot?.logged_in ? request('resume', {}, '已请求恢复连接') : request('start_login')}>{snapshot.logged_in ? '恢复连接' : snapshot.status === 'error' ? '重新连接微信' : '连接微信'}</button>{/if}
      {#if !['disconnected', 'starting_login', 'waiting_scan', 'scanned', 'need_verifycode', 'expired'].includes(snapshot.status)}<div class="management"><button disabled={locked} onclick={() => confirmation = 'disconnect'}>断开并删除实验室凭据</button><span class="hint">不影响企微、飞书或现有任务通知设置。</span></div>{/if}
    {/if}
    {#if confirmation}<div class="confirmation" use:focusConfirmation role="group" aria-label={confirmation === 'test' ? '发送测试确认' : '断开连接确认'}><strong>{confirmation === 'test' ? '向绑定的微信发送真实消息？' : '断开并删除实验室连接？'}</strong><p>{confirmation === 'test' ? '确认后将真实发送一条文本测试。平台受理不等于已读，网络超时后重复测试可能造成重复消息。' : '将停止本实验室轮询并删除保存的凭据和绑定信息。再次使用需要扫码，不会退出或修改 OpenClaw。'}</p><div class="actions"><button disabled={pending} onkeydown={escapeConfirmation} onclick={() => confirmation = null}>取消</button><button class="primary" disabled={locked} onkeydown={escapeConfirmation} onclick={() => confirmation === 'test' ? request('send_test', { confirmed: true }, '已提交文本测试，请在微信核对') : request('disconnect', {}, '实验室连接已断开')}>{confirmation === 'test' ? '确认发送' : '确认断开'}</button></div></div>{/if}
  {/if}
  <footer class="limitations"><strong>实验能力边界</strong><p>扫码成功不代表长期主动推送可用。会话上下文可能过期；尚未验证长期空闲发送、频率限制及平台政策，不承诺固定有效窗口。</p><p>关闭页面不会删除连接。停止轮询请使用「断开并删除实验室凭据」。</p></footer>
</section>
{/if}

<style>
  .weixin-lab{display:grid;gap:14px;min-width:0;font-size:12px;line-height:1.6;container-type:inline-size}.weixin-lab p{margin:5px 0 0}.weixin-lab strong{font-weight:600}.intro>div{display:flex;align-items:center;gap:8px;flex-wrap:wrap}.intro>div>strong{font-size:15px}.intro p,.hint,.limitations p,.status p{color:var(--muted)}.tag{border:1px solid var(--stroke);border-radius:3px;padding:1px 6px;font-size:10px;color:var(--muted)}.steps{list-style:none;display:flex;padding:0;margin:0;gap:8px}.steps li{display:flex;gap:7px;align-items:center;flex:1;background:var(--layer);padding:9px;border:1px solid var(--divider);border-radius:4px;color:var(--muted)}.steps li>span{display:grid;place-items:center;width:21px;height:21px;border:1px solid var(--stroke);border-radius:50%;font-size:11px}.steps .active{color:var(--accent);border-color:var(--accent);background:var(--selected)}.steps .done>span{color:var(--success);border-color:var(--success)}.status,.notice,.binding,.recipient,.test,.confirmation{padding:12px;border:1px solid var(--divider);border-radius:4px;background:var(--layer)}.notice{border-left:3px solid var(--accent)}.notice button,.binding button{margin-top:10px}.actions{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.weixin-lab button{min-height:30px}.weixin-lab button:disabled{cursor:default}.login-panel{display:grid;grid-template-columns:216px minmax(0,1fr);gap:16px;align-items:center}.qr{width:216px;height:216px;display:grid;place-items:center;background:#fff;border:1px solid var(--divider);border-radius:4px;overflow:hidden}.qr img{display:block;max-width:100%;height:auto}.qr-placeholder{padding:16px;text-align:center;color:#616161;display:grid;gap:8px}.qr-placeholder span{font-size:11px}.verify{display:grid;gap:7px}.verify input{width:140px;min-height:30px;box-sizing:border-box;letter-spacing:3px}.bind-code{display:block;margin-top:12px;padding:12px;font-size:23px;letter-spacing:2px;text-align:center;color:var(--accent);background:var(--selected);border:1px dashed var(--accent);border-radius:4px;overflow-wrap:anywhere;user-select:all}.recipient>div{display:grid;gap:3px}.recipient>div>span{color:var(--muted);overflow-wrap:anywhere}.recipient dl{margin:10px 0 0;padding-top:8px;border-top:1px solid var(--divider);display:grid;gap:4px}.recipient dl>div{display:flex;justify-content:space-between;gap:8px;flex-wrap:wrap}.recipient dt{color:var(--muted)}.recipient dd{margin:0}.test{display:flex;align-items:center;justify-content:space-between;gap:12px}.test>div{min-width:0}.test>button{flex-shrink:0}.test .hint{display:block;margin-top:6px}.management{display:grid;justify-items:start;gap:5px;padding-top:8px;border-top:1px solid var(--divider)}.confirmation{border-color:var(--accent);background:var(--selected)}.confirmation .actions{justify-content:flex-end;margin-top:10px}.error{color:var(--warning);border-left:3px solid var(--warning);padding-left:9px;overflow-wrap:anywhere}.feedback{color:var(--success)}.limitations{padding-top:10px;border-top:1px solid var(--divider);font-size:11px}.limitations>strong{color:var(--muted)}@container(max-width:430px){.login-panel{grid-template-columns:1fr}.qr{justify-self:center}.login-copy{text-align:center}.test{align-items:flex-start;flex-direction:column}.steps{gap:5px}.steps li{padding:8px 6px}.steps li>span{width:18px;height:18px}}
</style>
