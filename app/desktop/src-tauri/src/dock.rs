//! Main-window dock lifecycle. Only an explicit native title-bar drag can arm
//! automatic collapse; resize/programmatic moves/startup never arm it.
use std::time::{Duration,Instant};
use serde_json::{json,Value};
use tauri::{Emitter,Manager,PhysicalPosition,PhysicalSize,LogicalSize,WebviewWindow};
#[link(name="user32")]
extern "system" {fn GetAsyncKeyState(key:i32)->i16;fn GetCursorPos(point:*mut Cursor)->i32;}
#[repr(C)]#[derive(Clone,Copy,Default)]struct Cursor{x:i32,y:i32}
#[derive(Clone,Copy)]struct BallDrag{offset:Cursor,work:Rect,threshold:i64}
fn detached(ball:Rect,work:Rect,side:Side,threshold:i64)->bool{match side{Side::Left=>(ball.x as i64-work.x as i64).abs()>threshold,Side::Right=>(ball.right()-work.right()).abs()>threshold}}
fn expanded_position(ball:Rect,size:PhysicalSize<u32>,work:Rect,side:Side)->PhysicalPosition<i32>{let x=if side==Side::Right{ball.right()-size.width as i64}else{ball.x as i64};PhysicalPosition::new(x.max(work.x as i64).min(work.right()-size.width.min(work.w) as i64) as i32,(ball.y as i64).max(work.y as i64).min(work.bottom()-size.height.min(work.h) as i64) as i32)}
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum Side{Left,Right}
impl Side{fn name(self)->&'static str{match self{Self::Left=>"left",Self::Right=>"right"}}}
#[derive(Clone,Copy,Debug)]
struct Rect{x:i32,y:i32,w:u32,h:u32}
impl Rect{fn right(self)->i64{self.x as i64+self.w as i64}fn bottom(self)->i64{self.y as i64+self.h as i64}}
fn edge(panel:Rect,work:Rect,monitors:&[Rect],threshold:i64)->Option<Side>{
 let center=work.x as i64+work.w as i64/2;let middle=work.y as i64+work.h as i64/2;
 let exposed=|x:i64|!monitors.iter().any(|m|{let own=center>=m.x as i64&&center<m.right()&&middle>=m.y as i64&&middle<m.bottom();!own&&x>=m.x as i64&&x<m.right()&&(panel.y as i64)<m.bottom()&&panel.bottom()>m.y as i64});
 if (panel.x as i64-work.x as i64).abs()<=threshold&&exposed(work.x as i64-2){Some(Side::Left)}
 else if (panel.right()-work.right()).abs()<=threshold&&exposed(work.right()+1){Some(Side::Right)}else{None}
}
#[derive(Clone)]
struct Panel{position:PhysicalPosition<i32>,size:PhysicalSize<u32>,resizable:bool,pinned:bool,skip_taskbar:bool}
pub struct Dock{panel:Option<Panel>,side:Side,hovered:bool,enabled:bool,skip_taskbar:bool,until:Option<Instant>,drag:u64,compact_width:Option<u32>,ball_drag:Option<BallDrag>}
impl Dock{
 pub fn new(skip_taskbar:bool)->Self{Self{panel:None,side:Side::Right,hovered:false,enabled:true,skip_taskbar,until:None,drag:0,compact_width:None,ball_drag:None}}
 pub fn value(&self)->Value{json!({"type":"dock_state","collapsed":self.panel.is_some(),"side":self.side.name(),"hovered":self.hovered,"dragging":self.ball_drag.is_some()})}
 pub fn skip_save(&self)->bool{self.ball_drag.is_some()||self.panel.is_some()||self.compact_width.is_some()||self.until.is_some_and(|t|Instant::now()<t)}
 fn quiet(&mut self){self.until=Some(Instant::now()+Duration::from_millis(500));}
 fn emit(&self,w:&WebviewWindow){let _=w.emit("host-event",self.value());}
 fn layout(&self,w:&WebviewWindow)->Result<(),String>{
  let monitor=w.current_monitor().map_err(|e|e.to_string())?.ok_or("屏幕不可用")?;let work=monitor.work_area();let scale=monitor.scale_factor();
  let width=if self.hovered{280.}else{76.};let height=76.;let pixels=(width*scale).round() as u32;let hp=(height*scale).round() as u32;
  let y=self.panel.as_ref().map(|p|p.position.y).unwrap_or(work.position.y).max(work.position.y).min((work.position.y as i64+work.size.height.saturating_sub(hp) as i64) as i32);
  let x=if self.side==Side::Left{work.position.x}else{(work.position.x as i64+work.size.width.saturating_sub(pixels) as i64) as i32};
  w.set_size(LogicalSize::new(width,height)).map_err(|e|e.to_string())?;w.set_position(PhysicalPosition::new(x,y)).map_err(|e|e.to_string())?;Ok(())
 }
 pub fn collapse(&mut self,w:&WebviewWindow,side:Side)->Result<(),String>{
  if !self.enabled||self.panel.is_some()||w.is_maximized().unwrap_or(false)||w.is_minimized().unwrap_or(false){return Ok(());}
  let panel=Panel{position:w.outer_position().map_err(|e|e.to_string())?,size:w.inner_size().map_err(|e|e.to_string())?,resizable:w.is_resizable().map_err(|e|e.to_string())?,pinned:w.is_always_on_top().map_err(|e|e.to_string())?,skip_taskbar:self.skip_taskbar};
  self.panel=Some(panel);self.side=side;self.hovered=false;self.quiet();
  let result=(||{w.set_min_size(Some(LogicalSize::new(76.,76.))).map_err(|e|e.to_string())?;w.set_resizable(false).map_err(|e|e.to_string())?;w.set_always_on_top(true).map_err(|e|e.to_string())?;w.set_skip_taskbar(true).map_err(|e|e.to_string())?;w.set_shadow(false).map_err(|e|e.to_string())?;self.layout(w)})();
  if let Err(error)=result{let _=self.restore(w);return Err(error);}self.emit(w);Ok(())
 }
 pub fn restore(&mut self,w:&WebviewWindow)->Result<(),String>{
  if self.ball_drag.take().is_some(){self.drag=self.drag.wrapping_add(1);self.emit(w);}
  let Some(panel)=self.panel.clone()else{return Ok(());};self.quiet();
  // Windows frame changes alter the client rect: restore styles before the
  // captured client size, otherwise the WebView shrinks by the restored frame.
  w.set_resizable(panel.resizable).map_err(|e|e.to_string())?;
  w.set_shadow(true).map_err(|e|e.to_string())?;
  w.set_always_on_top(panel.pinned).map_err(|e|e.to_string())?;
  w.set_skip_taskbar(panel.skip_taskbar).map_err(|e|e.to_string())?;
  w.set_min_size(Some(LogicalSize::new(392.,570.))).map_err(|e|e.to_string())?;
  w.set_size(panel.size).map_err(|e|e.to_string())?;
  w.set_position(panel.position).map_err(|e|e.to_string())?;
  self.panel=None;self.hovered=false;self.emit(w);Ok(())
 }
 pub fn action(&mut self,w:&WebviewWindow,action:&str,value:&Value)->Result<Value,String>{
  match action{
   "dock_state"=>{},"dock_restore"=>self.restore(w)?,
   "dock_set_enabled"=>{self.enabled=value.as_bool().ok_or("dock enabled须为布尔值")?;self.drag=self.drag.wrapping_add(1);if !self.enabled{self.restore(w)?;}},
   "dock_hover"=>{let hover=value.as_bool().ok_or("dock hover须为布尔值")?;if self.ball_drag.is_none()&&self.panel.is_some()&&hover!=self.hovered{self.hovered=hover;self.quiet();self.layout(w)?;self.emit(w);}},
   "dock_collapse"=>{let side=value.as_str().or_else(||value["side"].as_str()).unwrap_or("right");let side=match side{"left"=>Side::Left,"right"=>Side::Right,_=>return Err("贴边方向无效".into())};self.collapse(w,side)?;},
   "details_collapse"=>{let compact=value.as_bool().ok_or("details collapse须为布尔值")?;if compact==self.compact_width.is_some(){return Ok(self.value());}self.restore(w)?;if compact&&self.compact_width.is_none(){let size=w.inner_size().map_err(|e|e.to_string())?;self.compact_width=Some(size.width);self.quiet();let scale=w.scale_factor().map_err(|e|e.to_string())?;w.set_size(PhysicalSize::new((392.*scale).round() as u32,size.height)).map_err(|e|e.to_string())?;}else if !compact{if let Some(width)=self.compact_width.take(){self.quiet();let size=w.inner_size().map_err(|e|e.to_string())?;w.set_size(PhysicalSize::new(width,size.height)).map_err(|e|e.to_string())?;}}},
   _=>return Err("未知贴边操作".into())
  }Ok(self.value())
 }
 fn drag_tick(&mut self,w:&WebviewWindow,token:u64,cursor:Option<Cursor>,down:bool)->Result<bool,String>{
  if self.drag!=token||self.ball_drag.is_none(){return Ok(false);}
  let result=(||{
   if let Some(cursor)=cursor{let mut drag=self.ball_drag.unwrap();let current=w.outer_size().map_err(|e|e.to_string())?;
    let position=PhysicalPosition::new(cursor.x.saturating_sub(drag.offset.x),cursor.y.saturating_sub(drag.offset.y));
    let ball=Rect{x:position.x,y:position.y,w:current.width,h:current.height};
    if self.panel.is_some(){
     if detached(ball,drag.work,self.side,drag.threshold){
      let size=self.panel.as_ref().unwrap().size;let position=expanded_position(ball,size,drag.work,self.side);
      self.panel.as_mut().unwrap().position=position;
      // Internal restoration is still the same gesture, not a cancellation.
      self.ball_drag=None;self.restore(w)?;self.ball_drag=Some(drag);
      drag.offset=Cursor{x:cursor.x.saturating_sub(position.x),y:cursor.y.saturating_sub(position.y)};self.ball_drag=Some(drag);self.emit(w);
     }else{
      let x=if self.side==Side::Left{drag.work.x}else{(drag.work.right()-ball.w as i64) as i32};
      let y=(position.y as i64).max(drag.work.y as i64).min(drag.work.bottom()-ball.h.min(drag.work.h) as i64) as i32;
      w.set_position(PhysicalPosition::new(x,y)).map_err(|e|e.to_string())?;
      if let Some(panel)=self.panel.as_mut(){panel.position.y=y;}
     }
    }else{w.set_position(position).map_err(|e|e.to_string())?;}
   }
   Ok(cursor.is_some()&&down)
  })();
  if !matches!(result.as_ref(),Ok(true)){self.ball_drag=None;self.quiet();self.emit(w);}result
 }
 pub fn preferences(&mut self,w:&WebviewWindow,skip_taskbar:bool)->Result<(),String>{self.restore(w)?;self.skip_taskbar=skip_taskbar;w.set_skip_taskbar(skip_taskbar).map_err(|e|e.to_string())}
}
// Never hold Dock's mutex on an async worker while synchronously dispatching
// window operations to the UI thread. A tray callback on that thread would
// otherwise wait for the mutex while the worker waits for the UI thread.
pub async fn on_main<T:Send+'static>(app:&tauri::AppHandle,work:impl FnOnce()->Result<T,String>+Send+'static)->Result<T,String>{
 let(tx,rx)=std::sync::mpsc::sync_channel(1);
 app.run_on_main_thread(move||{let _=tx.send(work());}).map_err(|e|e.to_string())?;
 tauri::async_runtime::spawn_blocking(move||rx.recv().map_err(|_|"窗口操作已中断".to_string())?).await.map_err(|_|"窗口操作已中断".to_string())?
}
pub fn restore(app:&tauri::AppHandle){if let Some(w)=app.get_webview_window("main"){if let Some(state)=app.try_state::<super::State>(){if let Ok(mut dock)=state.dock.lock(){let _=dock.restore(&w);}}}}
/// Starts on the UI thread; background work only samples the mouse. A bounded
/// acknowledgement channel permits exactly one outstanding UI tick.
pub fn start_ball_drag(w:&WebviewWindow)->Result<Value,String>{
 let mut cursor=Cursor::default();if unsafe{GetCursorPos(&mut cursor)}==0||unsafe{GetAsyncKeyState(1)}>=0{return Err("请按住头像拖动".into());}
 let state=w.state::<super::State>();let mut dock=state.dock.lock().map_err(|_|"贴边状态异常")?;
 if dock.panel.is_none()||!dock.enabled||dock.ball_drag.is_some(){return Ok(dock.value());}
 let position=w.outer_position().map_err(|e|e.to_string())?;let size=w.inner_size().map_err(|e|e.to_string())?;
 let monitor=w.current_monitor().map_err(|e|e.to_string())?.ok_or("屏幕不可用")?;let work=monitor.work_area();let pixels=(76.*monitor.scale_factor()).round() as i32;
 let ball_x=if dock.side==Side::Right{position.x.saturating_add(size.width as i32).saturating_sub(pixels)}else{position.x};
 dock.hovered=false;dock.quiet();dock.layout(w)?;
 dock.drag=dock.drag.wrapping_add(1);let token=dock.drag;
 dock.ball_drag=Some(BallDrag{offset:Cursor{x:cursor.x.saturating_sub(ball_x),y:cursor.y.saturating_sub(position.y)},work:Rect{x:work.position.x,y:work.position.y,w:work.size.width,h:work.size.height},threshold:(18.*monitor.scale_factor()).round() as i64});dock.emit(w);let value=dock.value();drop(dock);
 let window=w.clone();std::thread::spawn(move||{let began=Instant::now();loop{
  let mut point=Cursor::default();let valid=unsafe{GetCursorPos(&mut point)}!=0;let down=unsafe{GetAsyncKeyState(1)}<0&&began.elapsed()<Duration::from_secs(120);
  let (tx,rx)=std::sync::mpsc::sync_channel(1);let tick_window=window.clone();
  if window.app_handle().run_on_main_thread(move||{let state=tick_window.state::<super::State>();let keep=state.dock.lock().map(|mut d|d.drag_tick(&tick_window,token,valid.then_some(point),down).unwrap_or(false)).unwrap_or(false);let _=tx.send(keep);}).is_err(){return;}
  // No polling or window mutation while the UI callback is pending.
  if !rx.recv().unwrap_or(false){return;}std::thread::sleep(Duration::from_millis(16));
 }});Ok(value)
}
pub fn start_drag(w:&WebviewWindow)->Result<(),String>{
 // A release can beat a queued WebView command: a click must never start
 // Windows' modal move loop after the button has already been released.
 if unsafe{GetAsyncKeyState(1)}>=0{return Ok(());}
 let state=w.state::<super::State>();let mut dock=state.dock.lock().map_err(|_|"贴边状态异常")?;
 if dock.panel.is_some(){return Ok(());}dock.drag=dock.drag.wrapping_add(1);let token=dock.drag;let enabled=dock.enabled;let start=w.outer_position().map_err(|e|e.to_string())?;drop(dock);
 if enabled{let window=w.clone();std::thread::spawn(move||{
  // Poll global button state off the UI thread; native modal move loops can
  // otherwise prevent queued WebView work from running until after release.
  let began=Instant::now();let mut held=false;
  loop{let down=unsafe{GetAsyncKeyState(1)}<0;held|=down;if !down&&(held||began.elapsed()>Duration::from_millis(200)){break;}if began.elapsed()>Duration::from_secs(120){return;}std::thread::sleep(Duration::from_millis(20));}
  if !held{return;}
  let app=window.app_handle().clone();let _=app.run_on_main_thread(move||{
   let state=window.state::<super::State>();let Ok(mut dock)=state.dock.lock()else{return;};if dock.drag!=token||!dock.enabled||dock.panel.is_some(){return;}
   let(Ok(position),Ok(size),Ok(Some(monitor)),Ok(monitors))=(window.outer_position(),window.outer_size(),window.current_monitor(),window.available_monitors())else{return;};
   if (position.x as i64-start.x as i64).abs()+(position.y as i64-start.y as i64).abs()<4{return;}
   let work=monitor.work_area();let rect=Rect{x:position.x,y:position.y,w:size.width,h:size.height};let bounds:Vec<_>=monitors.iter().map(|m|Rect{x:m.position().x,y:m.position().y,w:m.size().width,h:m.size().height}).collect();
   if let Some(side)=edge(rect,Rect{x:work.position.x,y:work.position.y,w:work.size.width,h:work.size.height},&bounds,(18.*monitor.scale_factor()).round() as i64){let _=dock.collapse(&window,side);}
  });
 });}
 w.start_dragging().map_err(|e|e.to_string())
}
#[cfg(test)]mod tests{use super::*;
#[test]fn ball_detaches_only_beyond_side_threshold(){let work=Rect{x:-1920,y:0,w:1920,h:1080};let left=Rect{x:-1902,y:50,w:76,h:76};assert!(!detached(left,work,Side::Left,18));assert!(detached(Rect{x:-1901,..left},work,Side::Left,18));let right=Rect{x:-94,y:800,w:76,h:76};assert!(!detached(right,work,Side::Right,18));assert!(detached(Rect{x:-95,..right},work,Side::Right,18));}
#[test]fn detach_expands_near_ball_not_stale_panel_bounds(){let work=Rect{x:0,y:0,w:1920,h:1080};let size=PhysicalSize::new(780,570);assert_eq!(expanded_position(Rect{x:1800,y:700,w:76,h:76},size,work,Side::Right),PhysicalPosition::new(1096,510));assert_eq!(expanded_position(Rect{x:30,y:50,w:76,h:76},size,work,Side::Left),PhysicalPosition::new(30,50));}
#[test]fn only_external_edges_not_monitor_seams(){let a=Rect{x:0,y:0,w:1920,h:1080};let b=Rect{x:1920,y:0,w:1920,h:1080};let p=Rect{x:1520,y:100,w:400,h:570};assert_eq!(edge(p,a,&[a,b],18),None);assert_eq!(edge(p,a,&[a],18),Some(Side::Right));assert_eq!(edge(Rect{x:1,..p},a,&[a,b],18),Some(Side::Left));}#[test]fn negative_monitor_and_off_edge_are_supported(){let a=Rect{x:-1920,y:-200,w:1920,h:1080};assert_eq!(edge(Rect{x:-1910,y:0,w:400,h:570},a,&[a],18),Some(Side::Left));assert_eq!(edge(Rect{x:-1800,y:0,w:400,h:570},a,&[a],18),None);}}
