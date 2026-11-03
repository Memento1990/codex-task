use std::{io::Write, path::PathBuf, process::{Command, Stdio}, sync::{Arc, Mutex}};
use std::os::windows::process::CommandExt;
use serde_json::{json, Value};
use tauri::{Emitter, Manager, menu::{Menu, MenuItem}, tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState}, WebviewUrl, WebviewWindowBuilder, LogicalSize, LogicalPosition};
use tauri_plugin_opener::OpenerExt;
use floatboard_core::desktop::Engine;
mod dock;

#[derive(Default)]
struct ToastQueue { ready: bool, pending: Vec<Value> }
struct State { engine: Arc<Mutex<Engine>>, toasts: Mutex<ToastQueue>, updates: Arc<floatboard_core::update::UpdateService>, dock:Mutex<dock::Dock> }

fn development_mode()->bool{cfg!(debug_assertions)||std::env::var("FLOATBOARD_DEVELOPMENT").as_deref()==Ok("1")||std::env::current_exe().is_ok_and(|path|path.components().any(|part|part.as_os_str()=="target"||part.as_os_str()=="targets"))}

#[tauri::command]
async fn update_action(app:tauri::AppHandle, window:tauri::WebviewWindow, state:tauri::State<'_,State>, action:String, params:Value)->Result<Value,String>{
    if window.label()!="main"{return Err("此窗口不能更新应用".into());}
    if development_mode() && action!="snapshot"{return Err("开发调试版禁用正式更新，请发布GitHub Release后在正式版更新".into());}
    if action=="install"{
        if params["confirmed"]!=true{return Err("请确认重启看板后更新".into());}
        let root={let engine=state.engine.lock().map_err(|_|"本地状态异常")?;if engine.data_location()["restart_required"]==true{return Err("数据目录已迁移，请先重启看板再更新".into());}engine.root.clone()};
        let service=state.updates.clone();let failed=service.clone();
        let result=tauri::async_runtime::spawn_blocking(move||floatboard_core::update_install::prepare(&service,&root)).await.map_err(|_|"更新准备中断")?;
        if let Err(error)=result{failed.install_failed(&error);return Err(error);}
        app.exit(0);return Ok(json!({"state":"installing"}));
    }
    let service=state.updates.clone();
    if action=="check"||action=="download"{let snapshot=service.snapshot();tauri::async_runtime::spawn_blocking(move||{let _=if action=="check"{service.check()}else{service.download()};});return Ok(snapshot);}
    tauri::async_runtime::spawn_blocking(move||match action.as_str(){"snapshot"=>Ok(service.snapshot()),"cancel"=>service.cancel(),"preferences"=>service.preferences(&params),_=>Err("未知更新操作".into())}).await.map_err(|_|"更新操作中断")?
}

fn show_main(app:&tauri::AppHandle) { dock::restore(app); if let Some(w)=app.get_webview_window("main") { let _=w.show(); let _=w.unminimize(); let _=w.set_focus(); } }

#[tauri::command]
async fn bridge(app:tauri::AppHandle, window:tauri::WebviewWindow, state:tauri::State<'_,State>, method:String, mut params:Value)->Result<Value,String> {
    let allowed = ["snapshot","preferences","preview","connect","connect_request","disconnect","window_load","window_save","data_location","migrate_data"];
    if !allowed.contains(&method.as_str()) || window.label()!="main" && method!="snapshot" { return Err("此窗口不允许该操作".into()); }
    if window.label()!="main" { params["notify"]=json!(false); }
    let worker=state.engine.clone();
    let name=method.clone();
    let result=tauri::async_runtime::spawn_blocking(move||worker.lock().map_err(|_|"本地状态异常".to_string())?.call(&name,params)).await.map_err(|_|"本地操作已中断")??;
    if method=="preferences" {
        let ui=app.clone();let skip=!result["show_taskbar"].as_bool().unwrap_or(false);
        dock::on_main(&app,move||{if let Some(w)=ui.get_webview_window("main"){ui.state::<State>().dock.lock().map_err(|_|"贴边状态异常")?.preferences(&w,skip)?;}Ok(())}).await?;
        let theme=if result["theme"]=="dark"{tauri::Theme::Dark}else{tauri::Theme::Light};
        for (_,w) in app.webview_windows(){let _=w.set_theme(Some(theme));}
        if !result["notifications"].as_bool().unwrap_or(true) { if let Some(w)=app.get_webview_window("toast") { let _=w.emit("host-event",json!({"type":"clear"})); let _=w.hide(); } }
        let _=app.emit("host-event",json!({"type":"options","options":result}));
    }
    if method=="snapshot" && window.label()=="main"{let _=floatboard_core::update_install::acknowledge();}
    Ok(result)
}

fn completed(app:&tauri::AppHandle,value:Value)->Result<(),String> {
    if let Some(w)=app.get_webview_window("completed") { let _=w.show(); let _=w.unminimize(); let _=w.set_focus(); w.emit("host-event",json!({"type":"scope","value":value})).map_err(|e|e.to_string())?; }
    else {
        let script=format!("window.__FLOATBOARD_VIEW__={};",json!({"view":"completed","value":value}));
        let main=app.get_webview_window("main").ok_or("主窗口不可用")?;
        let theme=main.theme().ok();
        // Ownership preserves the relationship; matching the owner's topmost
        // band also keeps history above it across Windows pin/unpin changes.
        let w=WebviewWindowBuilder::new(app,"completed",WebviewUrl::App("index.html".into())).owner(&main).map_err(|e|e.to_string())?.always_on_top(main.is_always_on_top().map_err(|e|e.to_string())?).title("已完成记录").inner_size(760.,520.).min_inner_size(540.,400.).decorations(false).skip_taskbar(true).theme(theme).initialization_script(&script).build().map_err(|e|e.to_string())?;
        w.set_focus().map_err(|e|e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
// Windows 创建 WebView 走异步命令，避免阻塞消息循环。
async fn host_action(app:tauri::AppHandle, window:tauri::WebviewWindow, state:tauri::State<'_,State>, action:String, value:Value)->Result<Value,String> {
    let label=window.label();
    if label=="toast" && !["toast_size","toast_ready","open_notice","hide"].contains(&action.as_str()) || label=="completed" && !["hide","copy","open_reference","drag","minimize","toggle_maximize","window_state"].contains(&action.as_str()) { return Err("窗口操作不允许".into()); }
    match action.as_str() {
        "hide" => { if label=="main" { if let Some(w)=app.get_webview_window("completed"){let _=w.hide();} } window.hide().map_err(|e|e.to_string())?; },
        "show" => {let ui=app.clone();dock::on_main(&app,move||{show_main(&ui);Ok(())}).await?;},
        "quit" => app.exit(0),
        "dock_state"|"dock_restore"|"dock_hover"|"dock_set_enabled"|"dock_collapse"|"details_collapse" => { if label!="main"{return Err("此窗口不支持贴边".into());}let ui=app.clone();return dock::on_main(&app,move||ui.state::<State>().dock.lock().map_err(|_|"贴边状态异常")?.action(&window,&action,&value)).await; },
        "dock_drag" => {if label!="main"{return Err("此窗口不支持头像拖动".into());}return dock::on_main(&app,move||dock::start_ball_drag(&window)).await;},
        "drag" => {if label=="main"{dock::on_main(&app,move||dock::start_drag(&window)).await?;}else{window.start_dragging().map_err(|e|e.to_string())?;}},
        "minimize" => window.minimize().map_err(|e|e.to_string())?,
        "window_state" => return Ok(json!({"maximized":window.is_maximized().map_err(|e|e.to_string())?})),
        "toggle_maximize" => { if window.is_maximized().map_err(|e|e.to_string())? { window.unmaximize() } else { window.maximize() }.map_err(|e|e.to_string())?; },
        "restart" => {
            let worker=state.engine.clone();
            let root=tauri::async_runtime::spawn_blocking(move||worker.lock().map_err(|_|"本地状态异常".to_string())?.call("data_location",json!({}))).await.map_err(|_|"本地操作已中断")??;
            let path=root["current"].as_str().ok_or("数据目录不可用")?;
            let current=std::env::current_exe().map_err(|e|e.to_string())?;
            // 等待旧 GUI 退出后再启动，防止单实例插件把重启请求转交给旧实例。
            let script=format!("Wait-Process -Id {} -ErrorAction SilentlyContinue; & '{}' --data-dir '{}'",std::process::id(),current.to_string_lossy().replace('\'',"''"),path.replace('\'',"''"));
            Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe").args(["-NoProfile","-NonInteractive","-WindowStyle","Hidden","-Command",&script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|e|e.to_string())?;
            app.exit(0);
        },
        "pin" => {let ui=app.clone();dock::on_main(&app,move||{if window.label()=="main"{dock::restore(&ui);}let pinned=value.as_bool().unwrap_or(true);window.set_always_on_top(pinned).map_err(|e|e.to_string())?;if window.label()=="main"{if let Some(history)=ui.get_webview_window("completed"){history.set_always_on_top(pinned).map_err(|e|e.to_string())?;if history.is_visible().unwrap_or(false){history.set_focus().map_err(|e|e.to_string())?;}}}Ok(())}).await?;},
        "completed" => {let _=window.emit("host-event",json!({"type":"window_blur"}));completed(&app,value)?;},
        "copy" => { let text=value.as_str().unwrap_or(""); if text.len()>10000 {return Err("文本过长".into());} let mut child=Command::new("C:\\Windows\\System32\\clip.exe").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|e|e.to_string())?; if let Some(mut input)=child.stdin.take() { let bytes:Vec<u8>=text.encode_utf16().flat_map(u16::to_le_bytes).collect(); input.write_all(&[&[0xff,0xfe],bytes.as_slice()].concat()).map_err(|e|e.to_string())?; } let _=child.wait(); },
        "open_reference" => {
            let text=value.as_str().unwrap_or("");
            let valid_url=tauri::Url::parse(text).ok().filter(|u|["http","https"].contains(&u.scheme()) && u.host_str().is_some() && u.username().is_empty() && u.password().is_none());
            if let Some(url)=valid_url { app.opener().open_url(url.as_str(),None::<&str>).map_err(|e|e.to_string())?; }
            else { let path=PathBuf::from(text); let suffix=path.extension().and_then(|s|s.to_str()).unwrap_or("").to_lowercase(); if text.starts_with("\\\\") || !path.is_absolute() || !path.is_file() || !["md","txt","json","log","csv","png","jpg","jpeg","webp","pdf","html"].contains(&suffix.as_str()) {return Err("不支持此证据地址".into());} app.opener().open_path(text,None::<&str>).map_err(|e|e.to_string())?; }
        },
        "data_folder" => { let root=state.engine.lock().map_err(|_|"本地状态异常")?.root.clone(); app.opener().open_path(root.to_string_lossy().to_string(),None::<&str>).map_err(|e|e.to_string())?; },
        "toast" => {
            let mut queue=state.toasts.lock().map_err(|_|"通知状态异常")?;
            if queue.ready { if let Some(w)=app.get_webview_window("toast") { w.emit("host-event",json!({"type":"notice","value":value})).map_err(|e|e.to_string())?; } }
            else {
                // Keep blocker alerts even while the toast WebView is starting.
                // Coalesce per task and preserve sticky until explicitly dismissed.
                let mut value=value;
                if let Some(previous)=queue.pending.iter().find(|notice|notice["task_id"]==value["task_id"]){if previous["sticky"]==true{value["sticky"]=json!(true);}}
                queue.pending.retain(|notice|notice["task_id"]!=value["task_id"]);
                queue.pending.push(value);
                while queue.pending.iter().filter(|notice|notice["sticky"]!=true).count()>4 {
                    if let Some(index)=queue.pending.iter().position(|notice|notice["sticky"]!=true){queue.pending.remove(index);}
                }
                if app.get_webview_window("toast").is_none() {
                    WebviewWindowBuilder::new(&app,"toast",WebviewUrl::App("index.html".into())).title("任务通知").inner_size(370.,160.).transparent(true).decorations(false).resizable(false).always_on_top(true).skip_taskbar(true).focused(false).focusable(false).visible(false).initialization_script("window.__FLOATBOARD_VIEW__={view:'toast'};").build().map_err(|e|e.to_string())?;
                }
            }
        },
        "toast_ready" => {
            let mut queue=state.toasts.lock().map_err(|_|"通知状态异常")?;
            queue.ready=true;
            for notice in queue.pending.drain(..) { window.emit("host-event",json!({"type":"notice","value":notice})).map_err(|e|e.to_string())?; }
        },
        "toast_size" => {
            let monitor=app.get_webview_window("main").and_then(|w|w.current_monitor().ok().flatten()).or_else(||window.primary_monitor().ok().flatten()).ok_or("屏幕不可用")?;
            let work=monitor.work_area();let scale=monitor.scale_factor();
            let height=value.as_f64().unwrap_or(160.).max(1.).min(work.size.height as f64/scale-24.);
            window.set_size(LogicalSize::new(370.,height)).map_err(|e|e.to_string())?;
            window.set_position(LogicalPosition::new((work.position.x as f64+work.size.width as f64)/scale-382.,(work.position.y as f64+work.size.height as f64)/scale-height-12.)).map_err(|e|e.to_string())?;
            window.show().map_err(|e|e.to_string())?;
        },
        "open_notice" => { let ui=app.clone();dock::on_main(&app,move||{show_main(&ui);Ok(())}).await?; if value["completed"]==true {completed(&app,value.clone())?;} else {let _=app.emit_to("main","host-event",json!({"type":"select","value":value}));} },
        _=>return Err("未知窗口操作".into())
    }
    Ok(json!(true))
}

pub fn run() {
    if floatboard_core::update_install::startup_gate().is_err(){return;}
    let development=development_mode();
    if development && !(std::env::args().any(|arg|arg=="--isolated-instance") && std::env::args().any(|arg|arg=="--data-dir") && ["CODEX_HOME","LOCALAPPDATA","WEBVIEW2_USER_DATA_FOLDER"].iter().all(|key|std::env::var_os(key).is_some())){eprintln!("开发构建必须通过dev-isolated.ps1启动，禁止使用正式数据目录");return;}
    let root=floatboard_core::data_root();
    // Explicit isolated acceptance runs must not activate the user's live GUI.
    // Normal launches retain the single-instance behavior.
    let isolated=std::env::args().any(|arg|arg=="--isolated-instance") && std::env::args().any(|arg|arg=="--data-dir") && std::env::var_os("CODEX_HOME").is_some();
    let builder=tauri::Builder::default();
    let builder=if isolated{builder}else{builder.plugin(tauri_plugin_single_instance::init(|app,_,_|show_main(app)))};
    builder.plugin(tauri_plugin_opener::init())
        .setup(move|app|{
    let current=std::env::current_exe().expect("程序路径不可用");
    let _=floatboard_core::runtime::spawn_worker(&root);
    let mut adapter=Engine::new(root.clone(),current);
    let stored=adapter.call("window_load",json!({})).unwrap_or(json!({}));
    let options=floatboard_core::desktop::load_options(&root);
    let worker=Arc::new(Mutex::new(adapter));
    let updates=floatboard_core::update::UpdateService::new(root.clone());
    let scheduled=updates.clone();std::thread::spawn(move||{std::thread::sleep(std::time::Duration::from_secs(20));loop{if !development && scheduled.snapshot()["preferences"]["auto_check"]==true{let _=scheduled.check();}std::thread::sleep(std::time::Duration::from_secs(6*60*60));}});
    app.manage(State{engine:worker.clone(),toasts:Mutex::new(ToastQueue::default()),updates,dock:Mutex::new(dock::Dock::new(!options["show_taskbar"].as_bool().unwrap_or(false)))});

            let width=stored["width"].as_f64().unwrap_or(772.).max(392.);
            let height=stored["height"].as_f64().unwrap_or(570.).max(570.);
            let theme=if options["theme"]=="dark"{tauri::Theme::Dark}else{tauri::Theme::Light};
            let w=WebviewWindowBuilder::new(app,"main",WebviewUrl::App("index.html".into())).title(if development{"[DEV] Codex · 卡点调度台"}else{"Codex · 卡点调度台"}).initialization_script(format!("window.__FLOATBOARD_DEVELOPMENT__={};",development)).inner_size(width,height).min_inner_size(392.,570.).transparent(true).decorations(false).resizable(true).skip_taskbar(!options["show_taskbar"].as_bool().unwrap_or(false)).theme(Some(theme)).always_on_top(stored["pinned"].as_bool().unwrap_or(true)).build()?;
            if let Some(monitor)=w.primary_monitor()? { let work=monitor.work_area();let scale=monitor.scale_factor();let max_w=work.size.width as f64/scale;let max_h=work.size.height as f64/scale;
                let outer=w.outer_size()?;let inner=w.inner_size()?;
                let frame_w=(outer.width.saturating_sub(inner.width)) as f64/scale;
                let frame_h=(outer.height.saturating_sub(inner.height)) as f64/scale;
                let width=width.min(max_w-frame_w);let height=height.min(max_h-frame_h);
                let _=w.set_size(LogicalSize::new(width,height));
                let left=work.position.x as f64/scale;let top=work.position.y as f64/scale;
                let x=stored["x"].as_f64().unwrap_or(left+max_w-width-frame_w-20.).max(left).min(left+max_w-width-frame_w);
                let y=stored["y"].as_f64().unwrap_or(top+60.).max(top).min(top+max_h-height-frame_h);
                let _=w.set_position(LogicalPosition::new(x,y));
            }
            let show=MenuItem::with_id(app,"show","显示卡点调度台",true,None::<&str>)?;
            let settings=MenuItem::with_id(app,"settings","设置",true,None::<&str>)?;
            let quit=MenuItem::with_id(app,"quit","退出看板（采集继续）",true,None::<&str>)?;
            let menu=Menu::with_items(app,&[&show,&settings,&quit])?;
            TrayIconBuilder::new().icon(app.default_window_icon().unwrap().clone()).tooltip("Codex · 卡点调度台").menu(&menu).show_menu_on_left_click(false)
                .on_menu_event(|app,event|match event.id.as_ref(){"show"=>show_main(app),"settings"=>{show_main(app);let _=app.emit_to("main","host-event",json!({"type":"settings"}));},"quit"=>app.exit(0),_=>{}})
                .on_tray_icon_event(|tray,event|if matches!(event,TrayIconEvent::Click{button:MouseButton::Left,button_state:MouseButtonState::Up,..}){show_main(tray.app_handle());}).build(app)?;
            w.show()?;
            w.unminimize()?;
            Ok(())
        })
        .on_window_event(|w,event|{
            if let tauri::WindowEvent::CloseRequested{api,..}=event { api.prevent_close();let _=w.hide();if w.label()=="main"{if let Some(c)=w.app_handle().get_webview_window("completed"){let _=c.hide();}} }
            if matches!(event,tauri::WindowEvent::Focused(false)) {let _=w.emit("host-event",json!({"type":"window_blur"}));}
            if matches!(event,tauri::WindowEvent::Resized(_)) {
                let _=w.emit("host-event",json!({"type":"window_state","maximized":w.is_maximized().unwrap_or(false)}));
            }
            if w.label()=="main" && matches!(event,tauri::WindowEvent::Resized(_)|tauri::WindowEvent::Moved(_)) {
                {let state=w.state::<State>();if state.dock.try_lock().map(|d|d.skip_save()).unwrap_or(true){return;}}
                let maximized=w.is_maximized().unwrap_or(false);
                // 只保存正常窗口尺寸，最大化/最小化不覆盖用户的还原尺寸。
                if w.is_minimized().unwrap_or(false)||maximized{return;}
                if let (Ok(size),Ok(position),Ok(scale))=(w.inner_size(),w.outer_position(),w.scale_factor()) {let state=w.state::<State>();let params=json!({"width":size.width as f64/scale,"height":size.height as f64/scale,"x":position.x as f64/scale,"y":position.y as f64/scale});let engine=state.engine.clone();tauri::async_runtime::spawn_blocking(move||{if let Ok(mut adapter)=engine.lock(){let _=adapter.call("window_save",params);}});}
            }
        })
        .invoke_handler(tauri::generate_handler![bridge,host_action,update_action])
        .run(tauri::generate_context!()).expect("看板运行失败");
}
