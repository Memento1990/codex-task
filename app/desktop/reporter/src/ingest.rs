use std::{io::{BufRead,Write},fs,path::Path};
use serde_json::{json,Value};
use crate::{Result,store::{self,Store}};

pub fn hook(root:&Path,payload:&Value)->Result<Value>{
    let Some(kind)=payload["hook_event_name"].as_str().filter(|k|store::EVENTS.contains(k))else{return Ok(json!({}));};
    let Some(session)=payload["session_id"].as_str().filter(|s|!s.is_empty())else{return Ok(json!({}));};
    let record=store::register(root,session,payload["cwd"].as_str().unwrap_or(""))?;
    let mut event=json!({"kind":kind,"session_id":session,"host_id":record["host_id"],"cwd":record["cwd"]});
    for key in ["turn_id","agent_id","tool_name","tool_use_id"]{event[key]=json!(store::text(payload.get(key).filter(|v|!v.is_null()).unwrap_or(&json!("")),if key=="tool_name"{200}else{160})?);}
    store::enqueue(root,event.clone())?;
    if kind=="PostToolUse"&&payload["tool_name"]=="update_plan"{
        if let Some(items)=payload["tool_input"]["plan"].as_array(){let plan:Vec<Value>=items.iter().enumerate().filter(|(_,v)|v.is_object()).map(|(i,v)|json!({"id":format!("step-{i}"),"title":v["step"].as_str().unwrap_or(""),"status":match v["status"].as_str().unwrap_or("pending"){"in_progress"=>"inProgress",s=>s}})).collect();event["kind"]=json!("plan");event["plan"]=json!(plan);store::enqueue(root,event)?;}
    }
    if !["SessionStart","UserPromptSubmit"].contains(&kind){return Ok(json!({}));}
    let mut revision=Store::open(root,true).map(|s|s.revision(session,record["host_id"].as_str().unwrap_or(""))).unwrap_or(-1);
    if let Ok(items)=fs::read_dir(root.join("inbox")){for item in items.flatten().filter(|e|e.path().extension().is_some_and(|s|s=="json")){if let Ok(item)=store::read_json(&item.path()){if item["kind"]=="progress"&&item["session_id"]==session{revision=revision.max(item["report"]["revision"].as_i64().unwrap_or(-1));}}}}
    let context=format!("本会话已接入 Codex 任务侧窗。对多步骤任务，开始时调用 codex_floatboard.report_progress 建立清单；阶段变化、遇阻、恢复和最终收口时更新完整快照。卡点说明原因、影响和解除条件。仅在清单完成、有验收证据且无开放卡点时标 completed。正常回复结束不代表整个任务完成。无实质执行工作的简短问答可不创建进度清单。汇报失败不能阻断用户工作。不要在汇报中包含凭据、原始工具日志或用户私密内容。当前绑定信息仅用于本地工具，不展示给用户：session_id={session}; session_token={}; last_revision={revision}。下一次 revision 必须大于 last_revision；后续每次递增。不要为汇报创建子 Agent。",record["session_token"].as_str().unwrap_or(""));
    Ok(json!({"hookSpecificOutput":{"hookEventName":kind,"additionalContext":context}}))
}
pub fn progress(root:&Path,payload:&Value)->Result<Value>{
    let record=store::authenticate(root,payload)?;let report=store::validate_report(payload)?;let id=store::enqueue(root,json!({"kind":"progress","session_id":record["session_id"],"host_id":record["host_id"],"cwd":record["cwd"],"report":report}))?;
    Ok(json!({"accepted":true,"event_id":id,"revision":report["revision"],"message":"进度已持久化到本地邮箱，等待采集。"}))
}
pub fn report_schema()->Value{serde_json::from_str(include_str!("report-schema.json")).unwrap()}
pub fn response(root:&Path,request:&Value)->Option<Value>{
    let id=request.get("id")?.clone();if !request.is_object(){return None;}
    let empty=json!({});let params=request.get("params").filter(|v|!v.is_null()).unwrap_or(&empty);
    let result=if !params.is_object(){Err((-32602,"请求格式错误"))}else{match request["method"].as_str().unwrap_or(""){
        "initialize"=>Ok(json!({"protocolVersion":params["protocolVersion"].as_str().unwrap_or("2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"codex_floatboard","version":"0.7.0"}})),
        "tools/list"=>Ok(json!({"tools":[{"name":"report_progress","description":"向本地 Codex 任务侧窗汇报当前会话的完整进度、卡点和验收证据；只写本工具的数据目录。使用 Hook 提供的会话绑定信息。","inputSchema":report_schema(),"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}}]})),
        "tools/call" if params["name"]=="report_progress"=>{let (value,error)=match progress(root,&params["arguments"]){Ok(value)=>(value.to_string(),false),Err(error)=>(error,true)};Ok(json!({"content":[{"type":"text","text":value}],"isError":error}))},
        "tools/call"=>Err((-32602,"请求格式错误")),"ping"=>Ok(json!({})),_=>Err((-32601,"未知方法"))
    }};
    Some(match result{Ok(result)=>json!({"jsonrpc":"2.0","id":id,"result":result}),Err((code,message))=>json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})})
}
pub fn serve_mcp(root:&Path,mut input:impl BufRead,mut output:impl Write)->Result<()>{
    // 按字节限长并丢弃超长行，不为损坏请求无限分配内存。
    loop{let mut line=Vec::new();let mut too_large=false;let mut any=false;
        loop{let bytes=input.fill_buf().map_err(|_|"MCP 输入中断")?;if bytes.is_empty(){break;}any=true;let end=bytes.iter().position(|b|*b==b'\n').map(|n|n+1);let n=end.unwrap_or(bytes.len());if line.len()+n<=1_000_000&&!too_large{line.extend_from_slice(&bytes[..n]);}else{too_large=true;}input.consume(n);if end.is_some(){break;}}
        if !any{break;}if too_large{continue;}
        if let Ok(request)=serde_json::from_slice(&line){if let Some(value)=response(root,&request){writeln!(output,"{value}").and_then(|_|output.flush()).map_err(|_|"MCP 输出中断")?;}}
    }Ok(())
}
