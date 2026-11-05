/* 仅供原型的状态与行为；不使用真实 bridge、连接配置或网络投递。 */
(function () {
  const F = window.FB, K = window.KIT, esc = K.esc;
  const seed = JSON.stringify(F);
  const ui = { query:'', filter:'active', historyQuery:'', page:1, size:20, drawer:null, tab:'plan', dialog:null, palette:false, commandIndex:0, commandQuery:'', scenario:'baseline', notices:[], pinned:false };
  let config, storageKey, toastTimer, updateTimer, pointer, lastFocus, generation=0, suppressOrbClickUntil=0;
  const later = (fn,ms) => { const owner=generation; return setTimeout(()=>{if(owner===generation)fn();},ms); };
  const task = id => F.tasks.find(t => t.id === id);
  const selected = () => task(config?.selected()) || F.tasks.find(t => !['completed','cancelled','unreported'].includes(t.status)) || F.tasks[0];
  const button = (label, action, extra='', cls='') => `<button class="m-btn ${cls}" data-mock="${action}" ${extra}>${label}</button>`;
  const input = (name, value='', placeholder='', type='text') => `<input class="m-input" name="${name}" value="${esc(value)}" placeholder="${esc(placeholder)}" aria-label="${esc(placeholder||name)}" type="${type}">`;
  const badge = value => `<span class="m-badge">${esc(value)}</span>`;
  const busy = value => ['checking','downloading','installing','running','rebuilding','verifying'].includes(value);
  const time = value => value ? K.fmtDateTime(value) : '时间未记录';
  function save() { try { sessionStorage.setItem(storageKey,JSON.stringify({data:F,scenario:ui.scenario})); } catch (_) { /* 禁用存储时，当前页面仍可演示。 */ } }
  function changed() { save(); config.render(); }
  function toast(text) {
    document.getElementById('mock-toast')?.remove();
    const el=document.createElement('div'); el.id='mock-toast'; el.className='m-toast'; el.role='status'; el.textContent=text; document.body.append(el);
    clearTimeout(toastTimer); toastTimer=setTimeout(()=>el.remove(),2600);
  }
  function nav(screen) { ui.drawer=null; ui.dialog=null; ui.palette=false; config.state.screen=screen; history.replaceState(null,'','#'+screen); config.render(); }
  function filtered(options={}) {
    const q=ui.query.trim().toLowerCase();
    return F.tasks.filter(t => {
      const tone=K.tone(t);
      const match=ui.filter==='all' || ui.filter==='active' && !['completed','cancelled','unreported'].includes(t.status) || ui.filter==='active' && options.includeCompleted && t.status==='completed' || ui.filter==='blocked' && tone==='blocked' || ui.filter==='waiting' && t.status==='waiting' || t.status===ui.filter;
      return match && (!q || [t.title,t.tag,t.cwd,t.sessionId,...t.blockers.map(b=>b.title)].join(' ').toLowerCase().includes(q));
    });
  }
  function toolbar() {
    return `<div class="m-toolbar">${input('task-search',ui.query,'搜索任务、卡点、项目')}<select class="m-select" name="task-filter" aria-label="任务状态">${[['active','进行中'],['all','全部'],['blocked','有卡点'],['waiting','待确认'],['completed','已完成'],['unreported','未汇报'],['cancelled','已取消']].map(([v,l])=>`<option value="${v}" ${ui.filter===v?'selected':''}>${l}</option>`).join('')}</select><span class="m-count">${filtered().length} 项</span>${button('⌕','palette','aria-label="搜索命令"')}</div>`;
  }
  function notice(title,taskId,sticky=false) {
    if (!F.options.notifications) return;
    const n={id:'notice-'+Date.now(),title,taskId,sticky}; ui.notices.push(n); renderNotices();
    if (!sticky) setTimeout(()=>{ui.notices=ui.notices.filter(x=>x!==n);renderNotices();},F.options.notificationSeconds*1000);
  }
  function notify(event,t,title) {
    const label={blocker_opened:'卡点出现',blocker_resolved:'卡点解除',task_completed:'任务完成'}[event];
    notice(title,t.id,event==='blocker_opened');
    if (!F.notifications.subscriptions[event]) return;
    for(const ch of F.notifications.channels) F.notifications.records.unshift({id:'record-'+Date.now()+'-'+ch.id,at:F.now(),atAgo:0,channel:ch.id,event:label,title,status:ch.enabled&&ch.configured?(ui.scenario==='failure'?'failed':'sent'):'skipped',attempts:ch.enabled?1:0});
    F.notifications.records=F.notifications.records.slice(0,30);
  }
  function renderNotices() {
    let el=document.getElementById('mock-notices');
    if(!el){el=document.createElement('div');el.id='mock-notices';el.className='m-notices';document.body.append(el);}
    el.innerHTML=ui.notices.map(n=>`<div class="m-notice ${n.sticky?'sticky':''}"><header><b>${esc(n.title)}</b><button data-mock="dismiss-notice" data-id="${n.id}" aria-label="关闭通知">×</button></header>${task(n.taskId)?button('查看任务','detail',`data-task="${n.taskId}"`):''} <small class="m-muted">${n.sticky?'卡点通知 · 手动关闭':'普通通知 · 自动关闭'}</small></div>`).join('');
  }
  function shell(title,body,footer='',wide=false) {
    return `<section class="m-window ${wide?'history':''}" data-screen-label="${title}"><header class="m-header"><strong>${title}</strong><small>MOCK · 独立工具窗</small><button class="m-close" data-mock="close" aria-label="关闭窗口">×</button></header>${body}<footer class="m-footer">${footer || '<span>更改仅保存于本原型会话</span>'}${button('返回主窗','home')}</footer></section>`;
  }
  function settings() {
    const o=F.options;
    const row=(label,key,hint='')=>`<div class="m-row"><label for="opt-${key}">${label}${hint?`<small>${hint}</small>`:''}</label><input id="opt-${key}" class="m-switch" type="checkbox" data-option="${key}" ${o[key]?'checked':''}></div>`;
    return shell('看板设置',`<div class="m-body"><section class="m-section"><h3>外观与启动</h3>${row('开机自启','autoStart')}${row('任务栏显示','showTaskbar')}${row('桌面浮窗通知','notifications')}<div class="m-row"><label for="theme">主题</label><select id="theme" class="m-select" data-option="theme"><option value="dark" ${o.theme==='dark'?'selected':''}>暗色</option><option value="light" ${o.theme==='light'?'selected':''}>浅色</option></select></div><div class="m-row"><label>普通通知停留秒数</label><input class="m-input" type="number" min="1" max="60" name="notice-seconds" value="${o.notificationSeconds}"></div><div class="m-row"><label>头像球摘要</label><select class="m-select" data-option="dockSummaryMode">${[['hover','悬停显示'],['always','固定显示'],['never','不显示']].map(([v,l])=>`<option value="${v}" ${o.dockSummaryMode===v?'selected':''}>${l}</option>`).join('')}</select></div><div class="m-actions" style="margin-top:12px">${button('预览普通通知','test-notice')}${button('预览卡点通知','test-blocker-notice')}</div></section><section class="m-section"><h3>数据目录</h3><p class="m-code">${esc(F.dataLocation.current)}</p><div class="m-actions">${button('打开目录','directory')}${button('迁移目录…','migrate','', 'primary')}</div>${F.dataLocation.restartRequired?'<p class="m-note">目录切换已模拟完成，原目录保留。</p>':''}</section><section class="m-section"><h3>工具</h3><div class="m-actions">${button('Codex 接入','go','data-screen-target="integration"')}${button('消息通知','go','data-screen-target="notifications"')}${button('版本更新','go','data-screen-target="updates"')}${button('修复记录','go','data-screen-target="repair"')}</div></section></div>`);
  }
  function integration() {
    const c=F.connection, states=['missing','configured','mcp_connected','session_received','sync_verified','recovery_required'], n=c.state==='recovery_required'?-1:states.indexOf(c.state);
    const action=n===0?button('连接 Codex…','connect','','primary'):n===1?button('模拟组件回执','connection-next','','primary'):n===2?button('模拟会话事件','connection-next','','primary'):n===3?button('模拟任务汇报','connection-next','','primary'):n===4?button('完成','home','','primary'):c.state==='recovery_required'?button('查看并恢复…','recover','','primary'):button('查看问题详情','diagnostics','','primary');
    return shell('Codex 接入',`<div class="m-body"><div class="m-stepper">${['配置写入','连接回执','会话与同步'].map((l,i)=>`<span class="${n>i?'done':''}">${i+1}. ${l}</span>`).join('')}</div><div class="m-note">${connectionLabel()}${n===4?' · 历史回执不代表此刻实时在线。':''}</div><div class="m-actions">${action}${button('刷新状态','connection-refresh')}${button('查看配置','connection-config')}</div><section class="m-section" style="margin-top:18px"><h3>分层证据</h3><table class="m-table"><tbody>${[['配置写入',n>=1?'已写入':'未写入'],['Codex 授权','工具不能直接读取'],['组件回执',n>=2?time(c.mcpConnectedAt):'尚未收到 · 非心跳'],['会话事件',n>=3?time(c.sessionReceivedAt):'尚未收到'],['任务同步',n>=4?time(c.syncVerifiedAt):'尚未验证']].map(r=>`<tr><td>${r[0]}</td><td>${r[1]}</td></tr>`).join('')}</tbody></table></section><section class="m-section"><h3>当前 CODEX_HOME</h3><div class="m-code">${esc(c.codexHome)}</div></section><div class="m-actions">${button('复制同步指令','copy-prompt')}${button('查看诊断','diagnostics')}${n>0?button('断开连接…','disconnect','','danger'):''}</div><div class="m-row"><label>演示接入状态</label><select name="connection-state" class="m-select">${[...states,'conflict','partial','error'].map(s=>`<option ${s===c.state?'selected':''}>${s}</option>`).join('')}</select></div></div>`,'<span>配置变更只模拟本工具拥有的条目</span>');
  }
  function notifications() {
    const n=F.notifications;
    return shell('消息通知',`<div class="m-body"><section class="m-section"><h3>通知渠道</h3>${n.channels.map(c=>`<div class="m-row"><label>${c.name}<small>${c.configured?'已配置 · '+(c.enabled?'已启用':'已停用'):'未配置'}${c.lastTest?' · '+(c.lastTest==='sent'?'平台已受理':'测试失败'):''}</small></label><input class="m-switch" type="checkbox" data-channel="${c.id}" ${c.enabled?'checked':''}>${button(c.id==='wechat'?'绑定 / 管理':'配置','channel',`data-id="${c.id}"`)}${button('测试','test-channel',`data-id="${c.id}" ${!c.configured?'disabled':''}`)}</div>`).join('')}</section><section class="m-section"><h3>事件订阅</h3>${[['blocker_opened','卡点出现'],['blocker_resolved','卡点解除'],['task_completed','任务完成']].map(([v,l])=>`<div class="m-row"><label>${l}</label><input class="m-switch" type="checkbox" data-subscription="${v}" ${n.subscriptions[v]?'checked':''}></div>`).join('')}</section><section class="m-section"><h3>发送记录 · 最近 30 条</h3><div class="m-table-wrap"><table class="m-table"><thead><tr><th>时间 / 渠道</th><th>事件 / 标题</th><th>状态</th></tr></thead><tbody>${n.records.map(r=>`<tr><td>${K.ago(r.at||F.now()-r.atAgo,F.now())}<br><small>${{wechat:'微信',wecom:'企微',feishu:'飞书'}[r.channel]}</small></td><td>${esc(r.event)}<br><span class="m-muted">${esc(r.title)}</span></td><td>${r.status==='sent'?'平台已受理':r.status==='failed'?'投递失败':'未投递'}${r.status==='failed'?button('重试','retry-record',`data-id="${r.id}"`):''}</td></tr>`).join('')}</tbody></table></div></section></div>`,'<span>平台受理不代表对方已读；模拟中不会发送消息</span>');
  }
  function updates() {
    const u=F.updates;
    const label={idle:'检查可用更新',checking:'正在检查',available:'发现可用更新',downloading:'正在下载',ready:'更新已准备就绪',installing:'正在安装',up_to_date:'已是最新版本',error:'更新失败'}[u.state]||u.state;
    const a=u.state==='ready'?button('安装并重启…','install','','primary'):u.state==='available'?button('下载更新','download','','primary'):busy(u.state)?button('取消','cancel-update'):button(u.state==='error'?'重新检查':'检查更新','check-update','','primary');
    return shell('版本更新',`<div class="m-body"><section class="m-section"><h3>当前版本 ${esc(u.currentVersion)}</h3><div class="m-note">${label}${['available','ready','downloading'].includes(u.state)?' · '+u.latestVersion:''}</div>${u.error?`<p class="m-error">${esc(u.error)}</p>`:''}<div class="m-actions">${a}${button('发布说明','release-notes')}</div>${u.state==='downloading'?`<div class="m-progress"><i style="width:${u.progress||0}%"></i></div><span>${u.progress||0}% · 模拟下载</span>`:''}</section><section class="m-section"><h3>更新偏好</h3>${[['autoCheck','自动检查'],['autoDownload','自动下载']].map(([key,label])=>`<div class="m-row"><label>${label}</label><input class="m-switch" type="checkbox" data-update-option="${key}" ${u[key]?'checked':''}></div>`).join('')}</section><p class="m-note">下载完成后仍需确认安装。此处重启仅重置模拟窗口，不启动安装器。</p></div>`);
  }
  function repair() {
    const r=F.repair;
    return shell('修复任务记录',`<div class="m-body"><div class="m-note ${r.status==='error'?'danger':''}">${{not_started:'尚未开始',running:'正在暂停模拟写入',rebuilding:'正在从模拟快照重建',verifying:'正在验证记录',completed:'模拟修复成功 · 记录与设置已保留',error:'模拟校验失败 · 原始记录保留'}[r.status]||r.status}</div><section class="m-section"><h3>原始记录目录</h3><p class="m-code">${esc(F.dataLocation.current)}</p></section><div class="m-actions">${busy(r.status)?'<span>处理中…</span>':button(r.status==='error'?'重新修复…':'预览修复…','repair','','primary')}${button('查看修复结果','repair-result',r.status==='not_started'?'disabled':'')}</div><p class="m-muted">暂停写入 → 一致性快照重建 → 重启验证。所有步骤均模拟，保留原始数据，不结束 Codex。</p></div>`);
  }
  function completed() {
    const list=F.tasks.filter(t=>t.status==='completed' && (!ui.historyQuery||[t.title,t.cwd].join(' ').toLowerCase().includes(ui.historyQuery.toLowerCase())));
    const pages=Math.max(1,Math.ceil(list.length/ui.size)); ui.page=Math.min(ui.page,pages);
    return shell('完成记录',`<div class="m-toolbar">${input('history-search',ui.historyQuery,'搜索完成任务')}<span class="m-count">${list.length} 项</span></div><div class="m-table-wrap"><table class="m-table"><thead><tr><th>任务</th><th>步骤</th><th>耗时</th><th>证据</th><th>结束</th></tr></thead><tbody>${list.slice((ui.page-1)*ui.size,ui.page*ui.size).map(t=>`<tr data-mock="detail" data-task="${t.id}" tabindex="0"><td>${esc(t.title)}<br><small class="m-muted">${esc(t.cwd)}</small></td><td>${K.doneSteps(t)}/${t.plan.length}</td><td>${K.fmtCompact(K.elapsed(t))}</td><td>${t.evidence.length}</td><td>${time(t.endedAt)}</td></tr>`).join('')||'<tr><td colspan="5" class="m-empty">暂无完成记录</td></tr>'}</tbody></table></div><div class="m-footer m-pagination"><span>共 ${list.length} 条</span><select class="m-select" name="history-size">${[20,50,100].map(n=>`<option ${n===ui.size?'selected':''}>${n}</option>`).join('')}</select>${button('上一页','page-prev',ui.page===1?'disabled':'')}<span>${ui.page}/${pages}</span>${button('下一页','page-next',ui.page===pages?'disabled':'')}</div>`,'<span>点击记录查看清单、卡点与证据</span>',true);
  }
  function utility(screen) { return ({settings,integration,notifications,updates,repair,completed}[screen])?.() || (screen==='closed'?`<div class="m-closed">模拟窗口已关闭<br>${button('重新打开','home')}</div>`:null); }
  function openTask(id,blockerId) { const t=task(id); if(!t)return; config.select(id,blockerId);ui.palette=false;ui.drawer=id;ui.tab=blockerId?'blockers':'plan';ui.highlight=blockerId;lastFocus=document.activeElement;config.render();document.querySelector('.m-drawer button')?.focus();if(blockerId)document.querySelector('.m-drawer .m-highlight')?.scrollIntoView({block:'center'}); }
  function selectTask(id,blockerId) { ui.query='';ui.filter=task(id)?.status==='completed'?'completed':'all'; config.select(id,blockerId);config.state.screen=config.state.screen==='narrow'?'narrow':'main';config.render();if(blockerId)setTimeout(()=>document.querySelector(`[data-blocker-id="${CSS.escape(blockerId)}"]`)?.scrollIntoView({block:'center'}),0); }
  function detail() {
    const t=task(ui.drawer);if(!t)return '';
    let body='';
    if(ui.tab==='plan') body=t.plan.map(s=>`<div class="m-step"><header><strong>${K.STEPMARK[s.status]||'·'} ${esc(s.title)}</strong><span class="m-muted">${K.STATUS[s.status]}</span></header>${s.note?`<p class="m-muted">${esc(s.note)}</p>`:''}<div class="m-code">${s.startedAt?time(s.startedAt):'起点未记录'} → ${s.endedAt?time(s.endedAt):s.startedAt?'进行中':'未开始'}${s.timing?.partial?' · 部分记录':''}</div></div>`).join('')||'<div class="m-empty">等待结构化清单</div>';
    if(ui.tab==='blockers') body=t.blockers.map(b=>`<section class="m-blocker ${b.open?'open':''} ${b.id===ui.highlight?'m-highlight':''}" data-blocker-id="${b.id}"><header><strong>${esc(b.title)}</strong>${badge(b.open?'未解除':'已解除')}</header><dl><dt>原因</dt><dd>${esc(b.reason)}</dd><dt>影响</dt><dd>${esc(b.impact||'未提供')}</dd><dt>处理方</dt><dd>${esc(b.owner)}</dd><dt>解除条件</dt><dd>${esc(b.resolveWhen)}</dd></dl><div class="m-actions"><span class="m-muted">${b.open?(b.openedAt?K.fmtCompact(F.now()-b.openedAt):'开放时间未记录'):time(b.closedAt)}</span>${b.open?button('模拟解除…','resolve',`data-task="${t.id}" data-blocker="${b.id}"`):''}</div></section>`).join('')||'<div class="m-empty">没有卡点</div>';
    if(ui.tab==='evidence') body=`<div class="m-actions" style="margin-bottom:12px">${button('添加模拟证据','add-evidence',`data-task="${t.id}"`)}</div>`+t.evidence.map(e=>`<div class="m-proof"><header><strong>${esc(e.label)}</strong><small class="m-muted">首次记录 ${e.at?time(e.at):'时间未知'}</small></header><p class="m-code">${esc(e.reference)}</p>${button('打开证据预览','evidence',`data-task="${t.id}" data-evidence="${e.id}"`)}</div>`).join('');
    return `<div class="m-scrim" data-mock="close-drawer"><aside class="m-drawer" role="dialog" aria-modal="true" aria-label="任务详情"><header class="m-header"><strong>任务详情</strong>${badge(K.STATUS[t.status]||t.status)}<button class="m-close" data-mock="close-drawer" aria-label="关闭详情">×</button></header><div style="padding:14px 15px 6px"><h2>${esc(t.title)}</h2><div class="m-actions">${badge(K.doneSteps(t)+'/'+t.plan.length)}<span class="m-muted">${K.fmtElapsed(K.elapsed(t))}${t.timing?.partial?' · 部分记录':''}</span>${button('复制会话 ID','copy-session',`data-task="${t.id}"`)}</div><p class="m-code">${esc(t.cwd)}</p></div><nav class="m-tabs">${[['plan','清单'],['blockers','卡点 '+K.openBlockers(t).length],['evidence','证据 '+t.evidence.length]].map(([v,l])=>`<button data-mock="tab" data-tab="${v}" class="${ui.tab===v?'on':''}">${l}</button>`).join('')}</nav><div class="m-body">${body}</div><footer class="m-footer"><span>MOCK · 仅改变演示任务</span>${!['completed','cancelled','unreported'].includes(t.status)?button('推进任务','advance',`data-task="${t.id}"`,'primary'):''}${t.status==='completed'?button('重新打开任务','reopen',`data-task="${t.id}"`):''}</footer></aside></div>`;
  }
  function showDialog(d) { ui.palette=false;ui.dialog=d;lastFocus=document.activeElement;renderLayer();setTimeout(()=>document.querySelector('.m-dialog input,.m-dialog textarea,.m-dialog button')?.focus(),0); }
  function confirm(title,body,execute) { showDialog({kind:'confirm',title,body,execute,stage:0}); }
  function dialog() {
    const d=ui.dialog;if(!d)return '';
    let body=d.body||'', footer=button('关闭','dismiss-dialog');
    if(d.kind==='confirm') {body=`<div class="m-stepper"><span class="done">1. 预览</span><span class="${d.stage>=1?'done':''}">2. Dry run</span><span class="${d.stage>=2?'done':''}">3. 确认执行</span></div>${body}${d.stage>=1?'<div class="m-note">Mock 检查通过：只变更本原型状态，不操作本机文件或配置。</div>':''}`;footer=button('取消','dismiss-dialog')+button(d.stage===0?'运行 Dry run':d.stage===1?'继续确认':'确认执行','confirm-next','','primary');}
    if(d.kind==='evidence') {const e=task(d.taskId)?.evidence.find(e=>e.id===d.evidenceId);body=e?`<p class="m-code">${esc(e.reference)}</p><div class="m-note">模拟证据内容 · 不访问真实路径或外部站点</div><pre class="m-code">任务：${esc(task(d.taskId).title)}\n证据：${esc(e.label)}\n首次记录：${time(e.at)}\n验证结果：Mock PASS\n数据来源：${e.timeSource==='unknown'?'时间未知':'模拟首次汇报'}\n说明：该预览不构成真实验收证据。</pre>`:'';footer=button('复制引用','copy-reference')+button('关闭','dismiss-dialog');}
    if(d.kind==='add-evidence') {body=`<label>证据名称${input('proof-label','','如：回归报告')}</label><label>引用路径 / URL${input('proof-reference','','mock://proof/report')}</label>`;footer=button('取消','dismiss-dialog')+button('添加证据','save-evidence','','primary');}
    if(d.kind==='resolve') {const b=task(d.taskId).blockers.find(b=>b.id===d.blockerId);body=`<div class="m-note">解除条件：${esc(b.resolveWhen)}</div><label>模拟处理结果<textarea class="m-textarea" name="resolution" placeholder="填写满足解除条件的结果"></textarea></label>`;footer=button('取消','dismiss-dialog')+button('确认解除并记录证据','save-resolution','','primary');}
    if(d.kind==='migrate') {body=`<p class="m-code">当前：${esc(F.dataLocation.current)}</p><label>目标完整目录${input('migration-path','','D:\\MockFloatboard\\data')}</label><p class="m-muted">原目录保留。仅模拟路径切换。</p>`;footer=button('取消','dismiss-dialog')+button('预览迁移','migration-preview','','primary');}
    if(d.kind==='channel') {const c=F.notifications.channels.find(c=>c.id===d.id);body=c.id==='wechat'?`<div class="m-note">${c.configured?'模拟账号已绑定':'扫描状态仅用于演示，不包含真实登录二维码'}</div>${!c.configured?'<div class="m-qr"><b>MOCK</b></div>':''}<p class="m-code">${c.configured?'接收方：m***@mock':'等待扫描 → 已扫描 → 已绑定'}</p>`:`<label>Webhook URL${input('webhook',c.webhook||'','https://example.invalid/mock-hook')}</label><label>关键词（可选）${input('keyword',c.keyword||'')}</label>`;footer=button('取消','dismiss-dialog')+(c.id==='wechat'?c.configured?button('解除绑定…','unbind-channel','','danger'):button(d.scanned?'确认模拟绑定':'模拟扫码','bind-channel','','primary'):button('保存配置','save-channel','','primary'));}
    return `<div class="m-scrim" style="z-index:140" data-mock="dismiss-dialog"><section class="m-dialog" role="dialog" aria-modal="true" aria-label="${esc(d.title)}"><header class="m-header"><h2>${esc(d.title)}</h2><button class="m-close" data-mock="dismiss-dialog" aria-label="关闭对话框">×</button></header><div class="m-body">${body}<div class="m-error" role="alert"></div></div><footer class="m-footer">${footer}</footer></section></div>`;
  }
  function commands() {
    const entries=[...F.tasks.map(t=>({label:t.title,sub:[t.tag,t.cwd,t.sessionId].join(' · '),run:()=>openTask(t.id),kind:'任务'})),...Object.entries({main:'返回主窗',completed:'完成记录',settings:'看板设置',integration:'Codex 接入',notifications:'消息通知',updates:'版本更新',repair:'修复任务记录'}).map(([screen,label])=>({label,sub:'导航',run:()=>nav(screen),kind:'功能'})),...['advance','copy-session','dock','theme','pin','close'].map(a=>({label:{advance:'推进当前任务','copy-session':'复制会话 ID',dock:'收起为头像球',theme:'切换主题',pin:'切换置顶',close:'关闭模拟窗口'}[a],sub:'当前选中任务',run:()=>action(a,{}),kind:'动作'}))];
    const q=ui.commandQuery.trim().toLowerCase();return entries.filter(e=>[e.label,e.sub].join(' ').toLowerCase().includes(q));
  }
  function commandList() {const list=commands();ui.commandIndex=Math.min(Math.max(0,ui.commandIndex),Math.max(0,list.length-1));return list.map((c,i)=>`<button class="m-command ${i===ui.commandIndex?'on':''}" data-mock="command" data-index="${i}"><span>${esc(c.label)}<small>${esc(c.sub)}</small></span><kbd>${c.kind}</kbd></button>`).join('')||'<div class="m-empty">没有匹配的任务或命令</div>';}
  function palette() {return `<div class="m-scrim" data-mock="close-palette"><section class="m-dialog" role="dialog" aria-modal="true" aria-label="命令搜索"><div class="m-header" style="padding:12px">${input('command-search',ui.commandQuery,'搜索任务、项目、命令…')}<button data-mock="close-palette">Esc</button></div><div class="m-command-list">${commandList()}</div><footer class="m-footer">↑↓ 选择 · Enter 执行 · Esc 关闭</footer></section></div>`;}
  function openPalette() {ui.dialog=null;ui.palette=true;ui.commandQuery='';ui.commandIndex=0;lastFocus=document.activeElement;renderLayer();document.querySelector('[name="command-search"]')?.focus();}
  function renderLayer() {let el=document.getElementById('mock-layer');if(!el){el=document.createElement('div');el.id='mock-layer';document.body.append(el);}el.innerHTML=detail()+(ui.palette?palette():'')+dialog();}
  function dock(variant) {
    const t=selected(), active=F.tasks.filter(t=>!['completed','cancelled','unreported'].includes(t.status)), blockers=F.tasks.reduce((n,t)=>n+K.openBlockers(t).length,0), p=t?K.pct(t):0;
    const summary=variant==='c'?active.map(x=>`<div class="m-row" style="padding:5px 0"><span>${esc(x.tag)}</span><span>${K.doneSteps(x)}/${x.plan.length} ${K.openBlockers(x).length?'!'+K.openBlockers(x).length:''}</span></div>`).join(''):variant==='b'?K.actionItems(F.tasks).slice(0,3).map(x=>`<div class="m-row" style="padding:5px 0">${esc(x.blocker?.title||'等待确认')}<small>${esc(x.blocker?.owner||'你')}</small></div>`).join(''):`<div class="m-muted">${esc(t?.currentStep||'暂无进行中的任务')}</div>`;
    const title=t?.title||'暂无任务', ring=variant==='c'?active.map((x,i)=>`<circle cx="28" cy="28" r="23" fill="none" stroke="${K.openBlockers(x).length?'var(--red,var(--rose,#f2705e))':'var(--blue,var(--sky,#6fa8dc))'}" stroke-width="3" stroke-dasharray="${140/Math.max(active.length,1)-4} 145" stroke-dashoffset="${-i*145/Math.max(active.length,1)}"/>`).join(''):`<circle cx="28" cy="28" r="23" fill="none" stroke="var(--m-accent)" stroke-width="3" stroke-dasharray="${p*145} 145"/>`;
    return `<div class="m-desktop"><small>MOCK 桌面 · 拖动头像吸附左右边缘</small><div class="m-orb-wrap ${F.options.dockSummaryMode==='always'?'always':F.options.dockSummaryMode==='never'?'never':''}" style="${F.dockPosition?.left?'left:12px;right:auto;':''}top:${F.dockPosition?.top||46}%"><button class="m-orb" data-mock="restore" aria-label="恢复主窗"><svg viewBox="0 0 56 56"><circle cx="28" cy="28" r="23" fill="none" stroke="var(--m-line)" stroke-width="3"/>${ring}</svg><b>${variant==='c'?active.length:t?K.doneSteps(t)+'/'+t.plan.length:'—'}</b>${blockers?`<em>${blockers}</em>`:''}</button><div class="m-orb-summary"><strong>${variant==='c'?'并发任务 '+active.length:title}</strong>${summary||'<span class="m-muted">没有待介入事项</span>'}<p class="m-muted">${blockers} 个未解除卡点 · 点击恢复</p></div></div></div>`;
  }
  function connectionLabel() {return {missing:'尚未配置',configured:'配置完成 · 连接待确认',mcp_connected:'组件回执已收到',session_received:'会话事件已收到',sync_verified:'接入与同步已验证',recovery_required:'旧连接待恢复',conflict:'连接配置冲突',partial:'配置需要检查',error:'连接验证失败'}[F.connection.state]||'连接待验证';}
  function scenario(name) {
    generation++;clearInterval(updateTimer); const clean=JSON.parse(seed); for(const key of Object.keys(F))if(typeof F[key]!=='function')delete F[key];for(const key of Object.keys(clean))F[key]=clean[key];ui.scenario=name;ui.query='';ui.filter='active';ui.drawer=null;ui.dialog=null;ui.palette=false;ui.notices=[];ui.page=1;
    if(name==='waiting') {F.tasks[1].status='waiting';F.tasks[1].runtime='waiting_approval';F.tasks[1].waitingSince=F.now()-240;F.tasks[1].currentStep='等待确认回归范围';}
    if(name==='partial') {const t=F.tasks[0];t.startedAt=null;t.timing={seconds:480,since:F.now()-60,partial:true};t.plan[0].startedAt=null;t.plan[0].endedAt=null;t.plan[0].timing={seconds:100,since:null,partial:true};t.plan[0].segments=[];t.evidence[0].at=null;t.evidence[0].timeSource='unknown';t.blockers[0].openedAt=null;}
    if(name==='empty')F.tasks=[];
    if(name==='failure'){F.connection.state='error';F.updates.state='error';F.updates.error='模拟网络不可用';F.repair.status='error';}
    if(name==='states') {const t=F.tasks[1];t.status='cancelled';t.runtime='interrupted';t.endedAt=F.now();t.plan.forEach(s=>{if(s.startedAt&&!s.endedAt)s.endedAt=F.now();});}
    if(name==='history'){const base=F.tasks.find(t=>t.status==='completed');for(let i=1;i<=53;i++){const t=JSON.parse(JSON.stringify(base));t.id='history-'+i;t.title='归档任务 '+String(i).padStart(2,'0')+' · 回归记录';t.sessionId='mock-history-'+i;t.plan.forEach((s,n)=>s.id=t.id+'-s'+n);t.evidence.forEach((e,n)=>e.id=t.id+'-e'+n);F.tasks.push(t);}}
    if(name==='clean')for(const t of F.tasks){for(const b of t.blockers){b.open=false;b.closedAt=F.now();}for(const s of t.plan)if(s.status==='blocked'){settleStep(s,F.now());s.status='pending';}if(t.status==='waiting'){t.status='running';t.runtime='active';t.waitingSince=null;}}
    config.select(F.tasks[0]?.id||'');changed();renderNotices();
  }
  function settleStep(s,now) {
    const seconds=K.elapsed(s)??0;
    s.timing={seconds,since:null,partial:s.timing?.partial||false};
    for(const part of s.segments||[])if(!part.endedAt)part.endedAt=now;
  }
  function advance(t) {
    if(!t)return toast('暂无可推进任务');
    if(['completed','cancelled','unreported'].includes(t.status))return toast('当前任务不可推进');
    if(K.openBlockers(t).length){openTask(t.id,K.openBlockers(t)[0].id);return toast('先满足解除条件并解除卡点');}
    if(t.status==='waiting'){confirm('确认继续任务',`<p>${esc(t.currentStep||t.title)}</p>`,()=>{t.status='running';t.runtime='active';t.waitingSince=null;changed();});return;}
    const now=F.now(), current=t.plan.find(s=>s.status==='inProgress'||s.status==='blocked');
    if(current){settleStep(current,now);current.status='completed';current.endedAt=now;}
    const next=t.plan.find(s=>s.status!=='completed');
    if(next){next.status='inProgress';next.startedAt=next.startedAt||now;next.segments=next.segments||[];next.segments.push({startedAt:now,endedAt:null});next.timing={seconds:next.timing?.seconds||0,since:now,partial:next.timing?.partial||false};t.currentStep=next.title;toast('当前步骤已推进');}
    else if(!t.evidence.length){t.status='waiting';t.currentStep='等待验收证据';t.waitingSince=now;openTask(t.id);ui.tab='evidence';toast('完成前需要至少一项验收证据');}
    else {t.status='completed';t.runtime='idle';t.endedAt=now;t.currentStep='已完成';if(t.timing){t.timing.seconds+=(t.timing.since?now-t.timing.since:0);t.timing.since=null;}notify('task_completed',t,t.title+' · 已完成');toast('任务已进入完成记录');}
    changed();
  }
  function error(message) {const el=document.querySelector('.m-dialog .m-error');if(el)el.textContent=message;}
  function field(name) {return document.querySelector(`.m-dialog [name="${name}"]`)?.value.trim()||'';}
  async function copy(text) {try{await navigator.clipboard.writeText(text);toast('已复制');}catch(_){showDialog({title:'复制内容',body:`<textarea class="m-textarea" readonly>${esc(text)}</textarea>`});}}
  function action(a,d) {
    const t=task(d.task)||selected();
    if(a==='go')return nav(d.screenTarget);
    if(a==='home'||a==='restore')return nav(config.state.previousMain||'main');
    if(a==='detail')return openTask(d.task,d.blocker);
    if(a==='focus')return selectTask(d.task,d.blocker);
    if(a==='palette')return openPalette();
    if(a==='close-palette'){ui.palette=false;renderLayer();return lastFocus?.focus();}
    if(a==='command'){const c=commands()[Number(d.index)];ui.palette=false;c?.run();return renderLayer();}
    if(a==='tab'){ui.tab=d.tab;return renderLayer();}
    if(a==='close-drawer'){ui.drawer=null;renderLayer();return lastFocus?.focus();}
    if(a==='dismiss-dialog'){ui.dialog=null;renderLayer();return lastFocus?.focus();}
    if(a==='pin'){ui.pinned=!ui.pinned;config.render();return toast(ui.pinned?'模拟置顶已开启':'模拟置顶已关闭');}
    if(a==='theme'){F.options.theme=F.options.theme==='dark'?'light':'dark';return changed();}
    if(a==='dock'||a==='minimize'){if(['main','narrow'].includes(config.state.screen))config.state.previousMain=config.state.screen;return nav('dock');}
    if(a==='maximize'){document.querySelector('.win')?.classList.toggle('m-maximized');return;}
    if(a==='close')return nav(['main','narrow','dock'].includes(config.state.screen)?'closed':'main');
    if(a==='copy-session')return copy(t?.sessionId||'暂无会话');
    if(a==='advance')return advance(t);
    if(a==='reopen'){const seconds=K.elapsed(t)||0;t.status='running';t.runtime='active';t.endedAt=null;const s=t.plan.at(-1);if(s){settleStep(s,F.now());s.status='pending';s.endedAt=null;}t.currentStep=s?.title||'等待清单';t.timing={seconds,since:F.now(),partial:t.timing?.partial||false};changed();return;}
    if(a==='evidence')return showDialog({kind:'evidence',title:'证据预览',taskId:d.task,evidenceId:d.evidence});
    if(a==='copy-reference')return copy(task(ui.dialog.taskId).evidence.find(e=>e.id===ui.dialog.evidenceId).reference);
    if(a==='add-evidence')return showDialog({kind:'add-evidence',title:'添加模拟验收证据',taskId:d.task});
    if(a==='save-evidence'){const label=field('proof-label'),reference=field('proof-reference');if(!label||!reference)return error('名称和引用均为必填');if(!/^(mock:\/\/|https?:\/\/|[A-Za-z]:[\\/])/.test(reference))return error('请输入 mock://、http(s) URL 或 Windows 完整路径');task(ui.dialog.taskId).evidence.push({id:'proof-'+Date.now(),label,reference,at:F.now(),timeSource:'first_reported'});ui.dialog=null;ui.tab='evidence';changed();return;}
    if(a==='resolve')return showDialog({kind:'resolve',title:'模拟解除卡点',taskId:d.task,blockerId:d.blocker});
    if(a==='save-resolution'){const result=field('resolution');if(!result)return error('请填写满足解除条件的模拟结果');const x=task(ui.dialog.taskId),b=x.blockers.find(b=>b.id===ui.dialog.blockerId);b.open=false;b.closedAt=F.now();x.evidence.push({id:'resolve-'+Date.now(),label:'解除验证 · '+b.title,reference:'mock://resolution/'+b.id,at:F.now(),timeSource:'first_reported',note:result});if(!K.openBlockers(x).length)for(const s of x.plan)if(s.status==='blocked'){settleStep(s,F.now());s.status='pending';}ui.dialog=null;notify('blocker_resolved',x,b.title+' · 已解除');changed();return;}
    if(a==='test-notice'||a==='test-blocker-notice'){if(!F.options.notifications)return toast('桌面浮窗已关闭，请先开启');return notice('通知预览 · '+(a==='test-notice'?'普通通知':'卡点需介入'),selected()?.id,a!=='test-notice');}
    if(a==='dismiss-notice'){ui.notices=ui.notices.filter(n=>n.id!==d.id);return renderNotices();}
    if(a==='directory')return showDialog({title:'模拟数据目录',body:`<pre class="m-code">${esc(F.dataLocation.current)}\n├─ tasks.db\n├─ backup/\n└─ control/</pre>`});
    if(a==='migrate')return showDialog({kind:'migrate',title:'迁移数据目录'});
    if(a==='migration-preview'){const path=field('migration-path');if(!/^[A-Za-z]:[\\/][^<>:"|?*]+$/.test(path))return error('请输入有效的 Windows 完整目录');if(path.toLowerCase()===F.dataLocation.current.toLowerCase())return error('目标目录应与当前目录不同');return confirm('预览目录迁移',`<p class="m-code">${esc(F.dataLocation.current)}\n→ ${esc(path)}</p><p>离线复制并切换路径，保留旧目录。仅模拟。</p>`,()=>{F.dataLocation.current=path;F.dataLocation.restartRequired=true;changed();toast('模拟迁移完成');});}
    if(a==='confirm-next'){const dlg=ui.dialog;if(dlg.stage<2){dlg.stage++;return renderLayer();}ui.dialog=null;dlg.execute();return renderLayer();}
    if(a==='connect'||a==='recover')return confirm(a==='connect'?'预览连接配置':'预览恢复配置','<pre class="m-code">+ mock_floatboard 连接\n+ mock_session_hook\n保留所有非本工具配置</pre>',()=>{F.connection.state='configured';F.connection.configState='ready';changed();});
    if(a==='disconnect')return confirm('预览断开连接','<p>仅移除模拟的本工具配置；任务数据保留。</p>',()=>{F.connection.state='missing';F.connection.mcpConnected=false;changed();});
    if(a==='connection-next'){const c=F.connection;c.state={configured:'mcp_connected',mcp_connected:'session_received',session_received:'sync_verified'}[c.state]||c.state;if(c.state==='mcp_connected'){c.mcpConnected=true;c.mcpConnectedAt=F.now();}if(c.state==='session_received')c.sessionReceivedAt=F.now();if(c.state==='sync_verified')c.syncVerifiedAt=F.now();return changed();}
    if(a==='connection-refresh')return toast(connectionLabel()+' · 刷新不补造回执');
    if(a==='connection-config'||a==='diagnostics')return showDialog({title:a==='diagnostics'?'模拟接入诊断':'模拟连接配置',body:`<pre class="m-code">${esc(JSON.stringify({state:F.connection.state,codexHome:F.connection.codexHome,ownedEntry:'mock_floatboard',configState:F.connection.configState,evidenceSource:'mock'},null,2))}</pre>`});
    if(a==='copy-prompt')return copy('请将本会话当前任务同步到任务侧窗，仅使用本会话有效绑定，汇报当前完整快照；不补造历史、计时或证据。');
    if(a==='channel')return showDialog({kind:'channel',id:d.id,title:'配置 '+F.notifications.channels.find(c=>c.id===d.id).name});
    if(a==='save-channel'){const url=field('webhook');try{const u=new URL(url);if(u.protocol!=='https:')throw new Error();}catch(_){return error('请输入有效的 HTTPS Webhook URL');}const c=F.notifications.channels.find(c=>c.id===ui.dialog.id);c.webhook=url;c.keyword=field('keyword');c.configured=true;c.enabled=true;ui.dialog=null;changed();toast('模拟配置已保存，不向该 URL 发送请求');return;}
    if(a==='bind-channel'){if(!ui.dialog.scanned){ui.dialog.scanned=true;return renderLayer();}const c=F.notifications.channels.find(c=>c.id===ui.dialog.id);c.configured=true;c.enabled=true;ui.dialog=null;changed();return;}
    if(a==='unbind-channel'){const id=ui.dialog.id;return confirm('解除模拟绑定','<p>停用当前模拟接收方，发送记录保留。</p>',()=>{const c=F.notifications.channels.find(c=>c.id===id);c.configured=false;c.enabled=false;changed();});}
    if(a==='test-channel'){const c=F.notifications.channels.find(c=>c.id===d.id);if(!c?.configured)return toast('请先配置渠道');c.lastTest=ui.scenario==='failure'?'failed':'sent';F.notifications.records.unshift({id:'test-'+Date.now(),at:F.now(),channel:c.id,event:'测试消息',title:'模拟渠道测试',status:c.lastTest,attempts:1});F.notifications.records=F.notifications.records.slice(0,30);changed();return toast(c.lastTest==='sent'?'模拟平台已受理':'模拟投递失败，可在记录中重试');}
    if(a==='retry-record'){const r=F.notifications.records.find(r=>r.id===d.id),c=F.notifications.channels.find(c=>c.id===r.channel);if(!c.configured||!c.enabled)return toast('先配置并启用该渠道');r.status='sent';r.at=F.now();r.attempts++;changed();return toast('模拟重试已受理');}
    if(a==='check-update'){clearInterval(updateTimer);F.updates.state='checking';F.updates.error=null;changed();updateTimer=setTimeout(()=>{F.updates.state=ui.scenario==='failure'?'error':F.updates.currentVersion===F.updates.latestVersion?'up_to_date':'available';if(F.updates.state==='error')F.updates.error='模拟网络不可用 · 可切换常规场景重试';changed();if(F.updates.state==='available'&&F.updates.autoDownload)action('download',{});},550);return;}
    if(a==='download'){F.updates.state='downloading';F.updates.progress=0;changed();clearInterval(updateTimer);updateTimer=setInterval(()=>{F.updates.progress+=20;if(ui.scenario==='failure'){clearInterval(updateTimer);F.updates.state='error';F.updates.error='模拟下载中断';}else if(F.updates.progress>=100){clearInterval(updateTimer);F.updates.state='ready';}changed();},220);return;}
    if(a==='cancel-update'){clearInterval(updateTimer);F.updates.state='available';F.updates.progress=0;return changed();}
    if(a==='install')return confirm('预览安装与重启',`<p>${F.updates.currentVersion} → ${F.updates.latestVersion}</p><p>保留任务，只模拟看板重启。</p>`,()=>{F.updates.state='installing';changed();later(()=>{F.updates.currentVersion=F.updates.latestVersion;F.updates.state='up_to_date';changed();toast('模拟安装完成，任务已保留');},650);});
    if(a==='release-notes')return showDialog({title:'模拟发布说明 · '+F.updates.latestVersion,body:'<ul><li>改进多任务总览</li><li>完善卡点详情与通知</li><li>保留任务与本地设置</li></ul><p class="m-muted">原型示例版本，不代表已发布版本。</p>'});
    if(a==='repair')return confirm('预览修复任务记录',`<p class="m-code">${esc(F.dataLocation.current)}</p><p>暂停写入 → 从一致性快照重建 → 模拟重启验证。原始记录保留。</p>`,()=>{F.repair.status='running';changed();later(()=>{F.repair.status='rebuilding';changed();later(()=>{F.repair.status='verifying';changed();later(()=>{F.repair.status=ui.scenario==='failure'?'error':'completed';F.repair.checked=F.tasks.length;changed();},350);},350);},350);});
    if(a==='repair-result')return showDialog({title:'模拟修复结果',body:`<pre class="m-code">状态：${esc(F.repair.status)}\n任务数：${F.repair.checked??F.tasks.length}\n原始记录：保留\n真实文件：未改动\nCodex 会话：未操作</pre>`});
    if(a==='page-prev'){ui.page=Math.max(1,ui.page-1);return config.render();}
    if(a==='page-next'){ui.page++;return config.render();}
    if(a==='reset')return confirm('重置原型数据','<p>清除当前方案的 Mock 操作，恢复初始演示。其他方案不受影响。</p>',()=>scenario('baseline'));
  }
  function afterRender() {
    document.documentElement.dataset.theme=F.options.theme;
    document.querySelector('.m-orb-wrap')?.classList.toggle('left',!!F.dockPosition?.left);
    document.querySelectorAll('button[title]:not([aria-label])').forEach(el=>el.setAttribute('aria-label',el.title));
    document.querySelectorAll('.m-row input,.m-row select').forEach(el=>{const label=el.closest('.m-row')?.querySelector('label')?.textContent;if(label&&!el.hasAttribute('aria-label'))el.setAttribute('aria-label',label);});
    document.querySelectorAll('[data-mock="pin"]').forEach(el=>{el.classList.toggle('on',ui.pinned);el.setAttribute('aria-pressed',String(ui.pinned));});
    let controls=document.getElementById('mock-controls');if(!controls){controls=document.createElement('div');controls.id='mock-controls';controls.className='m-controls';document.querySelector('.proto-tag')?.append(controls);}
    if(controls)controls.innerHTML=`<span>MOCK</span><select class="m-select" name="mock-scenario" aria-label="演示场景">${[['baseline','常规任务'],['clean','卡点已解除'],['waiting','待确认'],['partial','缺失 / 部分计时'],['failure','失败与重试'],['empty','空数据'],['states','取消 / 中断'],['history','55 条完成记录']].map(([v,l])=>`<option value="${v}" ${ui.scenario===v?'selected':''}>${l}</option>`).join('')}</select>${button('推进当前任务','advance')}${button('重置','reset')}`;
    renderLayer();renderNotices();
  }
  function install(options) {
    config=options;storageKey='floatboard-redesign-v2-mock-'+config.variant;
    try{const saved=JSON.parse(sessionStorage.getItem(storageKey)||'null');if(saved){Object.assign(F,saved.data);ui.scenario=saved.scenario||'baseline';if(busy(F.updates.state))F.updates.state='available';if(busy(F.repair.status))F.repair.status='not_started';}}catch(_){}
    document.addEventListener('click',e=>{
      const el=e.target.closest('[data-mock]');
      if(el){if(el.disabled)return;if(el.classList.contains('m-scrim')&&e.target!==el)return;e.preventDefault();e.stopImmediatePropagation();if(el.dataset.mock==='restore'&&Date.now()<suppressOrbClickUntil)return;return action(el.dataset.mock,el.dataset);}
      const s=e.target.closest('[data-screen]');if(s){e.preventDefault();e.stopImmediatePropagation();nav(s.dataset.screen);}
    },true);
    document.addEventListener('input',e=>{
      const el=e.target;
      if(el.name==='command-search'){ui.commandQuery=el.value;ui.commandIndex=0;document.querySelector('.m-command-list').innerHTML=commandList();}
      if(el.name==='task-search'||el.name==='history-search'){
        const pos=el.selectionStart,toolbar=el.closest('.m-toolbar');
        ui[el.name==='task-search'?'query':'historyQuery']=el.value;ui.page=1;config.render();
        const replacement=document.querySelector(`[name="${el.name}"]`)?.closest('.m-toolbar');
        if(toolbar&&replacement){const count=toolbar.querySelector('.m-count');if(count)count.textContent=replacement.querySelector('.m-count')?.textContent||'';replacement.replaceWith(toolbar);}
        el.focus();el.setSelectionRange(pos,pos);
      }
    });
    document.addEventListener('change',e=>{
      const el=e.target;
      if(el.name==='mock-scenario')return scenario(el.value);
      if(el.name==='task-filter'){ui.filter=el.value;return config.render();}
      if(el.name==='history-size'){ui.size=Number(el.value);ui.page=1;return config.render();}
      if(el.name==='connection-state'){F.connection.state=el.value;return changed();}
      if(el.name==='notice-seconds'){const n=Number(el.value);if(!Number.isInteger(n)||n<1||n>60){el.value=F.options.notificationSeconds;return toast('通知时长须为 1～60 秒的整数');}F.options.notificationSeconds=n;return save();}
      if(el.dataset.option){F.options[el.dataset.option]=el.type==='checkbox'?el.checked:el.value;return changed();}
      if(el.dataset.subscription){F.notifications.subscriptions[el.dataset.subscription]=el.checked;return save();}
      if(el.dataset.channel){const c=F.notifications.channels.find(c=>c.id===el.dataset.channel);if(el.checked&&!c.configured){el.checked=false;return action('channel',{id:c.id});}c.enabled=el.checked;return changed();}
      if(el.dataset.updateOption){F.updates[el.dataset.updateOption]=el.checked;return save();}
    });
    document.addEventListener('keydown',e=>{
      if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();return ui.palette?(ui.palette=false,renderLayer()):openPalette();}
      if(e.key==='Escape'){if(ui.dialog)action('dismiss-dialog',{});else if(ui.palette)action('close-palette',{});else if(ui.drawer)action('close-drawer',{});return;}
      if(ui.palette&&['ArrowDown','ArrowUp','Enter'].includes(e.key)){e.preventDefault();if(e.key==='Enter')return action('command',{index:ui.commandIndex});ui.commandIndex=(ui.commandIndex+(e.key==='ArrowDown'?1:-1)+commands().length)%Math.max(1,commands().length);document.querySelector('.m-command-list').innerHTML=commandList();document.querySelector('.m-command.on')?.scrollIntoView({block:'nearest'});}
      if(e.key==='Tab'&&(ui.dialog||ui.palette||ui.drawer)){const roots=[...document.querySelectorAll('.m-dialog,.m-drawer')],root=roots.at(-1),els=[...root.querySelectorAll('button:not(:disabled),input,select,textarea,[tabindex="0"]')].filter(x=>x.offsetParent!==null);const first=els[0],last=els.at(-1);if(!root.contains(document.activeElement)){e.preventDefault();first?.focus();}else if(e.shiftKey&&document.activeElement===first){e.preventDefault();last?.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}}
      if(e.key==='Enter'&&!ui.palette&&e.target.matches('tr[data-mock]'))e.target.click();
    });
    document.addEventListener('pointerdown',e=>{const orb=e.target.closest('.m-orb');if(!orb)return;const wrap=orb.parentElement,desk=wrap.parentElement;pointer={x:e.clientX,y:e.clientY,wrap,desk,moved:false};orb.setPointerCapture(e.pointerId);});
    document.addEventListener('pointermove',e=>{if(!pointer)return;const p=pointer,r=p.desk.getBoundingClientRect();if(Math.hypot(e.clientX-p.x,e.clientY-p.y)>5)p.moved=true;if(p.moved){p.wrap.style.left=Math.max(0,Math.min(r.width-56,e.clientX-r.left-28))+'px';p.wrap.style.right='auto';p.wrap.style.top=Math.max(0,Math.min(r.height-56,e.clientY-r.top-28))+'px';}});
    document.addEventListener('pointerup',e=>{if(!pointer)return;const p=pointer;pointer=null;if(!p.moved)return;const r=p.desk.getBoundingClientRect(),left=e.clientX<r.left+r.width/2;F.dockPosition={left,top:Math.max(4,Math.min(84,(e.clientY-r.top-28)/r.height*100))};p.wrap.style.left=left?'12px':'auto';p.wrap.style.right=left?'auto':'12px';p.wrap.style.top=F.dockPosition.top+'%';p.wrap.classList.toggle('left',left);suppressOrbClickUntil=Date.now()+350;save();});
    setInterval(()=>{document.querySelectorAll('[data-live-elapsed]').forEach(el=>{const t=task(el.dataset.liveElapsed);el.textContent=(t?.timing?.partial?'≥':'')+K.fmtCompact(K.elapsed(t||{}));});document.querySelectorAll('[data-live-blocker]').forEach(el=>{const b=F.tasks.flatMap(t=>t.blockers).find(b=>b.id===el.dataset.liveBlocker);el.textContent=b?.openedAt?K.fmtCompact(F.now()-b.openedAt):'时间未记录';});},1000);
  }
  window.MOCK={install,utility,afterRender,toolbar,tasks:filtered,includeCompleted:()=>['all','completed'].includes(ui.filter),openTask,selectTask,openPalette,dock,connectionLabel,toast};
})();
