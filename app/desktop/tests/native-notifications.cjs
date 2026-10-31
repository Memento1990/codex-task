async (page) => {
  const results=[];const check=(name,value)=>{if(!value)throw new Error(name);results.push(name);};
  await page.getByRole('button',{name:'更多操作',exact:true}).click();await page.getByRole('button',{name:'设置',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'看板设置'});
  check('通知默认开启',await dialog.getByRole('checkbox',{name:'消息浮窗通知'}).isChecked());
  check('通知默认三秒',await dialog.getByRole('spinbutton',{name:'通知停留 秒'}).inputValue()==='3');
  const ready=page.context().waitForEvent('page');await dialog.getByRole('button',{name:'测试通知',exact:true}).click();const toast=await ready;
  await toast.getByText('消息通知预览',{exact:true}).waitFor();check('真实原生通知窗可显示',true);
  check('通知背景透明',await toast.evaluate(()=>getComputedStyle(document.documentElement).backgroundColor==='rgba(0, 0, 0, 0)'));
  await toast.locator('.toast-close').click();await toast.waitForFunction(()=>document.querySelectorAll('.toast').length===0);check('通知可手动关闭',true);
  const send=async(id,seconds=30)=>page.evaluate(({id,seconds})=>window.__TAURI_INTERNALS__.invoke('host_action',{action:'toast',value:{task_id:'notice-'+id,title:'合成消息 '+id,lines:['清单状态已更新','仅用于隔离验收'],tab:0,seconds,theme:'light'}}),{id,seconds});
  for(let i=1;i<=5;i++)await send(i);
  await toast.waitForFunction(()=>document.querySelectorAll('.toast').length===4);
  check('最多四条并保留最新',await toast.getByText('合成消息 1',{exact:true}).count()===0&&await toast.getByText('合成消息 5',{exact:true}).isVisible());
  await send(5);check('同一任务消息合并',await toast.locator('.toast').count()===4);
  await toast.screenshot({path:'test-results/native/native-notifications.png',scale:'css'});
  for(let i=0;i<4;i++)await toast.locator('.toast-close').first().click();
  await send('pause',1);await toast.getByText('合成消息 pause',{exact:true}).waitFor();
  await toast.locator('.toast-open').hover();await toast.waitForTimeout(1200);
  check('悬停暂停倒计时',await toast.locator('.toast').count()===1);
  await toast.mouse.move(380,-5);await toast.waitForFunction(()=>document.querySelectorAll('.toast').length===0,{},{timeout:5000});check('离开后继续并自动消失',true);
  await send('disabled');await toast.getByText('合成消息 disabled',{exact:true}).waitFor();
  await dialog.getByRole('checkbox',{name:'消息浮窗通知'}).uncheck();await dialog.getByRole('button',{name:'保存',exact:true}).click();
  await toast.waitForFunction(()=>document.querySelectorAll('.toast').length===0);check('关闭通知立即清除浮窗',true);
  await page.getByRole('button',{name:'更多操作',exact:true}).click();await page.getByRole('button',{name:'设置',exact:true}).click();await dialog.getByRole('checkbox',{name:'消息浮窗通知'}).check();await dialog.getByRole('button',{name:'保存',exact:true}).click();
  check('通知设置恢复默认',true);
  return {passed:results.length,checks:results};
}
