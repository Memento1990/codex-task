use std::{io::Write, path::PathBuf, process::{Command, Stdio}, sync::{Arc, Mutex}};
use std::os::windows::process::CommandExt;
use serde_json::{json, Value};
use tauri::{Emitter, Manager, menu::{Menu, MenuItem}, tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState}, WebviewUrl, WebviewWindowBuilder, LogicalSize, LogicalPosition};
use tauri_plugin_opener::OpenerExt;
use floatboard_core::desktop::Engine;

struct State { engine: Arc<Mutex<Engine>>, root: PathBuf }

fn show_main(app:&tauri::AppHandle) { if let Some(w)=app.get_webview_window("main") { let _=w.show(); let _=w.unminimize(); let _=w.set_focus(); } }

#[tauri::command]
async fn bridge(app:tauri::AppHandle, window:tauri::WebviewWindow, state:tauri::State<'_,State>, method:String, mut params:Value)->Result<Value,String> {
    let allowed = ["snapshot","preferences","preview","connect","connect_request","disconnect","window_load","window_save"];
    if !allowed.contains(&method.as_str()) || window.label()!="main" && method!="snapshot" { return Err("此窗口不允许该操作".into()); }
    if window.label()!="main" { params["notify"]=json!(false); }
    let worker=state.engine.clone();
    let name=method.clone();
    let result=tauri::async_runtime::spawn_blocking(move||worker.lock().map_err(|_|"本地状态异常".to_string())?.call(&name,params)).await.map_err(|_|"本地操作已中断")??;
    if method=="preferences" {
        if let Some(w)=app.get_webview_window("main") { w.set_skip_taskbar(!result["show_taskbar"].as_bool().unwrap_or(false)).map_err(|e|e.to_string())?; }
        let theme=if result["theme"]=="dark"{tauri::Theme::Dark}else{tauri::Theme::Light};
        for (_,w) in app.webview_windows(){let _=w.set_theme(Some(theme));}
        if !result["notifications"].as_bool().unwrap_or(true) { if let Some(w)=app.get_webview_window("toast") { let _=w.emit("host-event",json!({"type":"clear"})); let _=w.hide(); } }
        let _=app.emit("host-event",json!({"type":"options","options":result}));
    }
    Ok(result)
}

fn completed(app:&tauri::AppHandle,value:Value)->Result<(),String> {
    if let Some(w)=app.get_webview_window("completed") { let _=w.show(); let _=w.set_focus(); w.emit("host-event",json!({"type":"scope","value":value})).map_err(|e|e.to_string())?; }
    else {
        let script=format!("window.__FLOATBOARD_VIEW__={};",json!({"view":"completed","value":value}));
        let theme=app.get_webview_window("main").and_then(|w|w.theme().ok());
        WebviewWindowBuilder::new(app,"completed",WebviewUrl::App("index.html".into())).title("已完成记录").inner_size(760.,520.).min_inner_size(540.,400.).skip_taskbar(true).theme(theme).initialization_script(&script).build().map_err(|e|e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
// Windows 创建 WebView 走异步命令，避免阻塞消息循环。
async fn host_action(app:tauri::AppHandle, window:tauri::WebviewWindow, state:tauri::State<'_,State>, action:String, value:Value)->Result<Value,String> {
    let label=window.label();
    if label=="toast" && !["toast_size","open_notice","hide"].contains(&action.as_str()) || label=="completed" && !["hide","copy","open_reference"].contains(&action.as_str()) { return Err("窗口操作不允许".into()); }
    match action.as_str() {
        "hide" => { if label=="main" { if let Some(w)=app.get_webview_window("completed"){let _=w.hide();} } window.hide().map_err(|e|e.to_string())?; },
        "quit" => app.exit(0),
        "pin" => { window.set_always_on_top(value.as_bool().unwrap_or(true)).map_err(|e|e.to_string())?; },
        "completed" => completed(&app,value)?,
        "copy" => { let text=value.as_str().unwrap_or(""); if text.len()>10000 {return Err("文本过长".into());} let mut child=Command::new("C:\\Windows\\System32\\clip.exe").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000).spawn().map_err(|e|e.to_string())?; if let Some(mut input)=child.stdin.take() { let bytes:Vec<u8>=text.encode_utf16().flat_map(u16::to_le_bytes).collect(); input.write_all(&[&[0xff,0xfe],bytes.as_slice()].concat()).map_err(|e|e.to_string())?; } let _=child.wait(); },
        "open_reference" => {
            let text=value.as_str().unwrap_or("");
            let valid_url=tauri::Url::parse(text).ok().filter(|u|["http","https"].contains(&u.scheme()) && u.host_str().is_some() && u.username().is_empty() && u.password().is_none());
            if let Some(url)=valid_url { app.opener().open_url(url.as_str(),None::<&str>).map_err(|e|e.to_string())?; }
            else { let path=PathBuf::from(text); let suffix=path.extension().and_then(|s|s.to_str()).unwrap_or("").to_lowercase(); if text.starts_with("\\\\") || !path.is_absolute() || !path.is_file() || !["md","txt","json","log","csv","png","jpg","jpeg","webp","pdf","html"].contains(&suffix.as_str()) {return Err("不支持此证据地址".into());} app.opener().open_path(text,None::<&str>).map_err(|e|e.to_string())?; }
        },
        "data_folder" => app.opener().open_path(state.root.to_string_lossy().to_string(),None::<&str>).map_err(|e|e.to_string())?,
        "toast" => {
            let existed=app.get_webview_window("toast").is_some();
            let script=format!("window.__FLOATBOARD_VIEW__={};",json!({"view":"toast","notice":value}));
            let w=if let Some(w)=app.get_webview_window("toast"){w}else{WebviewWindowBuilder::new(&app,"toast",WebviewUrl::App("index.html".into())).title("任务通知").inner_size(370.,160.).transparent(true).decorations(false).resizable(false).always_on_top(true).skip_taskbar(true).focused(false).focusable(false).visible(false).initialization_script(&script).build().map_err(|e|e.to_string())?};
            if existed { w.emit("host-event",json!({"type":"notice","value":value})).map_err(|e|e.to_string())?; }
        },
        "toast_size" => {
            let monitor=app.get_webview_window("main").and_then(|w|w.current_monitor().ok().flatten()).or_else(||window.primary_monitor().ok().flatten()).ok_or("屏幕不可用")?;
            let work=monitor.work_area();let scale=monitor.scale_factor();
            let height=value.as_f64().unwrap_or(160.).max(1.).min(work.size.height as f64/scale-24.);
            window.set_size(LogicalSize::new(370.,height)).map_err(|e|e.to_string())?;
            window.set_position(LogicalPosition::new((work.position.x as f64+work.size.width as f64)/scale-382.,(work.position.y as f64+work.size.height as f64)/scale-height-12.)).map_err(|e|e.to_string())?;
            window.show().map_err(|e|e.to_string())?;
        },
        "open_notice" => { show_main(&app); if value["completed"]==true {completed(&app,value.clone())?;} else {let _=app.emit_to("main","host-event",json!({"type":"select","value":value}));} },
        _=>return Err("未知窗口操作".into())
    }
    Ok(json!(true))
}

pub fn run() {
    let root=floatboard_core::data_root();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app,_,_|show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .setup(move|app|{
    let current=std::env::current_exe().expect("程序路径不可用");
    let _=floatboard_core::runtime::spawn_worker(&root);
    let mut adapter=Engine::new(root.clone(),current);
    let stored=adapter.call("window_load",json!({})).unwrap_or(json!({}));
    let options=floatboard_core::desktop::load_options(&root);
    let worker=Arc::new(Mutex::new(adapter));
    app.manage(State{engine:worker.clone(),root:root.clone()});

            let width=stored["width"].as_f64().unwrap_or(772.).max(392.);
            let height=stored["height"].as_f64().unwrap_or(570.).max(570.);
            let theme=if options["theme"]=="dark"{tauri::Theme::Dark}else{tauri::Theme::Light};
            let w=WebviewWindowBuilder::new(app,"main",WebviewUrl::App("index.html".into())).title("Codex · 卡点调度台").inner_size(width,height).min_inner_size(392.,570.).resizable(true).skip_taskbar(!options["show_taskbar"].as_bool().unwrap_or(false)).theme(Some(theme)).always_on_top(stored["pinned"].as_bool().unwrap_or(true)).build()?;
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
            if w.label()=="main" && matches!(event,tauri::WindowEvent::Resized(_)|tauri::WindowEvent::Moved(_)) {
                if w.is_minimized().unwrap_or(false){return;}
                if let (Ok(size),Ok(position),Ok(scale))=(w.inner_size(),w.outer_position(),w.scale_factor()) {let state=w.state::<State>();let params=json!({"width":size.width as f64/scale,"height":size.height as f64/scale,"x":position.x as f64/scale,"y":position.y as f64/scale});let engine=state.engine.clone();tauri::async_runtime::spawn_blocking(move||{if let Ok(mut adapter)=engine.lock(){let _=adapter.call("window_save",params);}});}
            }
        })
        .invoke_handler(tauri::generate_handler![bridge,host_action])
        .run(tauri::generate_context!()).expect("看板运行失败");
}
