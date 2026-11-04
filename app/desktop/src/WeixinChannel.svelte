<script lang="ts">
  import QRCode from 'qrcode';
  import { call } from './host';
  import { validWeixinVerifyCode, weixinQrPayload, weixinLabRemaining, weixinLabTime, type WeixinLabSnapshot } from './weixin-lab-model';
  import type { NotificationSnapshot } from './notification-model';
  let { snapshot, enabled=false, blocked=false, onupdate, onbusychange=(_busy:boolean)=>{} }:{snapshot?:WeixinLabSnapshot;enabled?:boolean;blocked?:boolean;onupdate:(s:NotificationSnapshot)=>void;onbusychange?:(busy:boolean)=>void}=$props();
  let pending=$state(false),error=$state(''),code=$state(''),qr=$state('');
  let confirm=$state<'disconnect'|'enable'|'test'|null>(null);
  let payload=$derived(weixinQrPayload(snapshot?.qrcode_img_content));
  let locked=$derived(blocked||pending||!snapshot);
  $effect(()=>{onbusychange(pending);});
  $effect(()=>{const value=payload;let cancelled=false;qr='';if(value)void QRCode.toDataURL(value,{width:320,margin:4}).then(image=>{if(!cancelled)qr=image;}).catch(()=>{if(!cancelled)error='二维码生成失败，请刷新重试。';});return()=>{cancelled=true;};});
  async function request(action:string,params:Record<string,unknown>={}){if(locked)return;pending=true;error='';try{onupdate(await call<NotificationSnapshot>('notification_weixin_action',{action,...params}));confirm=null;code='';}catch{error='微信操作未完成，请检查会话状态及网络后重试。';}finally{pending=false;}}
  async function channel(enable:boolean){if(locked)return;pending=true;error='';try{onupdate(await call<NotificationSnapshot>('notification_channel_save',{id:'wechat',enabled:enable}));confirm=null;}catch{error='请先完成扫码绑定，再开启微信任务通知。';}finally{pending=false;}}
  async function test(){if(locked)return;pending=true;error='';try{onupdate(await call<NotificationSnapshot>('notification_test',{id:'wechat',confirmed:true}));confirm=null;}catch{error='测试未提交，请先开启外部通知、完成绑定并启用微信渠道。';}finally{pending=false;}}
</script>
<section class="weixin-channel" aria-label="微信任务通知连接">
 <p class="intro">将任务完成、卡点等已订阅事件发送到你绑定的微信。凭据由当前 Windows 用户加密保存；不会读取 OpenClaw 或执行微信消息中的指令。</p>
 {#if error}<p role="alert" class="warning">{error}</p>{/if}
 {#if snapshot}
 <div class="connection-state" role="status"><strong>{snapshot.bound?'收件人已绑定':snapshot.logged_in?'已扫码 · 待绑定':'尚未绑定微信'}</strong><span>{snapshot.status==='paused'?'会话失效或暂停，请重新连接':snapshot.active?'连接处理中／保持会话中':'连接未在轮询'}</span></div>
 {#if snapshot.error}<p class="warning" role="status">连接异常；请尝试恢复。若会话已失效，请断开后重新扫码；不要反复重发状态未知的通知。</p>{/if}
 {#if ['starting_login','waiting_scan','scanned','need_verifycode','expired'].includes(snapshot.status)}
 <div class="qr-panel">{#if qr&&snapshot.status!=='expired'}<img src={qr} width="180" height="180" alt="使用接收通知的微信扫描二维码">{:else}<div class="qr-empty">{snapshot.status==='expired'?'二维码已过期':'正在等待真实二维码'}</div>{/if}<div><strong>用接收通知的微信扫码</strong><p>请勿分享二维码或验证码。二维码有效期：{weixinLabRemaining(snapshot.qr_expires_at,Date.now())}</p><div class="actions"><button disabled={locked||snapshot.busy} onclick={()=>request('start_login')}>刷新二维码</button><button disabled={locked} onclick={()=>request('cancel_login')}>取消登录</button></div></div></div>
 {#if snapshot.status==='need_verifycode'}<form onsubmit={e=>{e.preventDefault();void request('verify_code',{code:code.trim()});}}><label>手机验证码 <input bind:value={code} maxlength="16" inputmode="numeric" autocomplete="one-time-code" disabled={locked}></label><button disabled={locked||!validWeixinVerifyCode(code)}>提交验证码</button></form>{/if}
 {:else if !snapshot.logged_in}<button class="primary" disabled={locked||snapshot.busy} onclick={()=>request('start_login')}>扫码连接微信</button>
 {:else if !snapshot.bound}<div class="binding"><strong>确认收件人身份</strong><p>从扫码账号向机器人发送下方完整绑定码，5 分钟内有效。不会绑定其他用户的消息。</p>{#if snapshot.binding_code}<code>{snapshot.binding_code}</code>{/if}<button disabled={locked||snapshot.busy} onclick={()=>request('start_binding')}>{snapshot.binding_code?'重新生成绑定码':'生成绑定码'}</button></div>
 {:else}<dl><div><dt>收件人</dt><dd>{snapshot.recipient_masked||'已绑定的扫码用户'}</dd></div><div><dt>最近会话消息</dt><dd>{weixinLabTime(snapshot.last_received_at)}</dd></div><div><dt>最近发送</dt><dd>{weixinLabTime(snapshot.last_sent_at)}</dd></div></dl><div class="actions"><button class="primary" disabled={locked} onclick={()=>enabled?channel(false):confirm='enable'}>{enabled?'停用微信任务通知':'启用微信任务通知'}</button><button disabled={locked||!enabled} onclick={()=>confirm='test'}>发送微信测试</button><button disabled={locked||snapshot.busy} onclick={()=>request('resume')}>恢复连接</button></div>{/if}
 {#if snapshot.logged_in}<button class="disconnect" disabled={locked} onclick={()=>confirm='disconnect'}>断开并删除微信凭据</button>{/if}
 {:else}<p role="status">正在读取微信连接状态…</p>{/if}
 {#if confirm}<div class="confirmation" role="group" aria-label="微信操作确认"><strong>{confirm==='enable'?'允许将任务信息发送到微信？':confirm==='test'?'发送真实微信测试？':'断开并删除绑定？'}</strong><p>{confirm==='enable'?'启用后将自动发送订阅事件，包括任务标题、卡点标题和事件提示。启用后先建立当前任务基线，不补发基线之前的变化；重启后自动恢复已启用连接。外部通知总开关也须开启。':confirm==='test'?'将向绑定用户发送真实文本。平台受理不等于手机送达；状态未知时重试可能重复。':'停止连接并清除本机凭据，未投递微信记录将取消。已经发出的请求无法撤回。'}</p><div class="actions"><button disabled={pending} onclick={()=>confirm=null}>取消</button><button class="primary" disabled={locked} onclick={()=>confirm==='enable'?channel(true):confirm==='test'?test():request('disconnect',{confirmed:true})}>确认{confirm==='enable'?'启用':confirm==='test'?'发送':'断开'}</button></div></div>{/if}
 <p class="footnote">会话可能因平台策略或长期无消息而失效。开启通知不是永久送达保证；查看发送记录，状态未知不自动重复投递。</p>
</section>
<style>
.weixin-channel{display:grid;gap:12px;min-width:0;font-size:12px;line-height:1.65}.intro,.footnote,.connection-state span{color:var(--muted);margin:0}.connection-state{display:flex;justify-content:space-between;gap:10px;flex-wrap:wrap}.actions{display:flex;gap:8px;flex-wrap:wrap}.qr-panel{display:flex;gap:16px;align-items:center;flex-wrap:wrap}.qr-panel img{background:white;border:1px solid var(--divider)}.qr-panel>div{flex:1;min-width:160px}.qr-empty{width:180px;min-height:180px;display:grid;place-items:center;background:var(--layer)}.binding,.confirmation{padding:12px;background:var(--layer);border:1px solid var(--stroke);border-radius:5px}.binding code{display:block;overflow-wrap:anywhere;color:var(--accent);padding:10px 0;user-select:all}dl{margin:0;display:grid;gap:4px}dl>div{display:flex;gap:12px;flex-wrap:wrap}dt{color:var(--muted)}dd{margin:0}.warning{color:var(--warning)}.disconnect{justify-self:start}input{max-width:150px}form{display:flex;gap:8px;flex-wrap:wrap}
</style>
