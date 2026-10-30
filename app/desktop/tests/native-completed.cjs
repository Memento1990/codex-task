async (page) => {
  const results=[];const check=(name,value)=>{if(!value)throw new Error(name);results.push(name);};
  const main=page.context().pages().find(p=>p!==page);
  await page.getByText('已完成记录',{exact:true}).first().waitFor();
  check('完成任务二级窗独立显示',await page.locator('.task-row').count()===13);
  check('主窗保留原选择',await main.getByRole('heading',{name:'Windows 交互验收 · 合成',exact:true}).isVisible());
  check('完成清单保留备注',await page.getByText('最小 392 × 570；默认 772 × 570。',{exact:true}).isVisible());
  const list=page.locator('.task-list');
  await list.evaluate(el=>{el.scrollTop=el.scrollHeight;});
  check('二级任务列表独立滚动',await list.evaluate(el=>el.scrollTop>0));
  const last=page.locator('.task-row').last();await last.click();
  await page.getByRole('heading',{name:'接口回归与结果归档 · 01',exact:true}).waitFor();
  await page.getByRole('button',{name:'证据',exact:true}).click();
  check('完成证据可查看',await page.locator('.evidence').count()>0);
  await page.getByRole('button',{name:'动态',exact:true}).click();
  check('完成动态可查看',await page.locator('.activity').count()>0);
  await page.screenshot({path:'docs/evidence/FEAT-008/native-completed.png',scale:'css'});
  await page.getByRole('button',{name:'关闭',exact:true}).click();
  await main.getByRole('button',{name:'已完成清单 1 查看 ↗',exact:true}).click();
  await page.getByText('编译检查',{exact:true}).waitFor();
  check('完成清单二级入口可定位',await page.locator('.step-row').count()===1);
  check('完成清单模式不常驻任务列表',await page.locator('.task-sidebar').count()===0);
  check('两个入口复用同一个二级窗',page.context().pages().length===2);
  check('主窗未完成内容保留',await main.getByText('桌面交互验证',{exact:true}).isVisible());
  await page.getByRole('button',{name:'关闭',exact:true}).click();
  return {passed:results.length,checks:results};
}
