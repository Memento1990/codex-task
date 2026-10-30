use std::collections::{HashMap,HashSet};
use rusqlite::Connection;
use serde::{Serialize,Deserialize};
use serde_json::{json,Value};
use crate::Result;

#[derive(Default,Clone,Serialize,Deserialize)]
pub struct Timer{started_at:Option<f64>,ended_at:Option<f64>,seconds:f64,since:Option<f64>,partial:bool}
impl Timer{
    fn advance(&mut self,active:bool,at:f64,finished:bool){
        if active&&self.since.is_none(){if self.started_at.is_none(){self.started_at=Some(at);}self.since=Some(at);self.ended_at=None;}
        else if !active&&self.since.is_some(){self.seconds+=(at-self.since.take().unwrap()).max(0.);self.ended_at=Some(at);}
        if finished&&self.started_at.is_none()&&self.ended_at.is_none(){self.partial=true;self.ended_at=Some(at);}
    }
}
struct State{revision:i64,title:String,status:String,last_at:f64,plan_at:f64,timing:Timer,steps:HashMap<String,Timer>,blockers:HashMap<String,Timer>}
impl Default for State{fn default()->Self{Self{revision:-1,title:String::new(),status:"unreported".into(),last_at:0.,plan_at:0.,timing:Timer::default(),steps:HashMap::new(),blockers:HashMap::new()}}}
#[derive(Default)]
pub struct TimingIndex{cursor:i64,states:HashMap<String,State>}
impl TimingIndex{
    pub fn update(&mut self,db:&Connection)->Result<()>{
        let high:i64=db.query_row("select coalesce(max(rowid),0) from events",[],|r|r.get(0)).map_err(|_|"计时记录读取失败")?;
        let mut stmt=db.prepare("select task_id,kind,at,payload from events where rowid>?1 and rowid<=?2 and kind in ('progress','plan') order by rowid").map_err(|_|"计时记录读取失败")?;
        let rows=stmt.query_map([self.cursor,high],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,f64>(2)?,r.get::<_,String>(3)?))).map_err(|_|"计时记录读取失败")?;
        for row in rows{
            let (id,kind,at,raw)=row.map_err(|_|"计时记录读取失败")?;let event:Value=serde_json::from_str(&raw).map_err(|_|"计时记录格式无效")?;
            if event["agent_id"].as_str().is_some_and(|s|!s.is_empty()){continue;}
            let state=self.states.entry(id).or_default();
            let report=if kind=="progress"{
                let report=&event["report"];let revision=report["revision"].as_i64().ok_or("计时修订无效")?;
                if revision<=state.revision{continue;}
                let status=report["status"].as_str().unwrap_or("unreported");let title=report["title"].as_str().unwrap_or("");
                if ["completed","cancelled"].contains(&state.status.as_str())&&!["completed","cancelled"].contains(&status)&&title!=state.title{state.timing=Timer::default();state.steps.clear();state.blockers.clear();}
                state.revision=revision;state.title=title.into();state.status=status.into();report.clone()
            }else{if state.revision>=0||at<state.plan_at{continue;}state.plan_at=at;json!({"status":"running","plan":event["plan"],"blockers":[]})};
            let at=at.max(state.last_at);state.last_at=at;let terminal=["completed","cancelled"].contains(&report["status"].as_str().unwrap_or(""));state.timing.advance(!terminal,at,terminal);
            for (key,timers) in [("plan",&mut state.steps),("blockers",&mut state.blockers)]{
                let mut present=HashSet::new();
                if let Some(items)=report[key].as_array(){for item in items{let id=item["id"].as_str().unwrap_or("").to_owned();present.insert(id.clone());let active=if key=="blockers"{item["open"]==true}else{["inProgress","blocked"].contains(&item["status"].as_str().unwrap_or(""))};let finished=if key=="blockers"{item["open"]==false}else{item["status"]=="completed"};timers.entry(id).or_default().advance(active&&!terminal,at,finished);}}
                for (id,timer) in timers.iter_mut(){if !present.contains(id){timer.advance(false,at,false);}}
            }
        }self.cursor=high;Ok(())
    }
    pub fn attach(&self,task:&mut Value){
        let state=self.states.get(task["id"].as_str().unwrap_or(""));task["timing"]=state.map(|s|json!(s.timing)).unwrap_or(Value::Null);
        for key in ["plan","blockers"]{if let Some(items)=task[key].as_array_mut(){for item in items{item["timing"]=state.and_then(|s|if key=="plan"{&s.steps}else{&s.blockers}.get(item["id"].as_str().unwrap_or(""))).map(|t|json!(t)).unwrap_or(Value::Null);}}}
    }
}
