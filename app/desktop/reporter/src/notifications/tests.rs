use super::*;

#[test]
fn corrupt_or_missing_task_store_does_not_block_outbox_or_erase_baseline(){
    for corrupt in [false,true] {
        let f=Fixture::new();f.configure();
        f.service.observe(&[task("running",1,json!([]))]).unwrap();
        let baseline=transaction(&f.root,|s|Ok(serde_json::to_value(&s.baseline).unwrap())).unwrap();
        if corrupt {fs::write(f.root.join("events.sqlite3"),b"corrupt SQLite fixture").unwrap();}
        f.service.call("notification_test",json!({"id":"wecom"})).unwrap();
        let mut sent=false;
        assert!(worker_cycle(&f.service.inner,|_,_|{sent=true;Outcome::Accepted}).is_err());
        assert!(sent,"task storage errors must not gate explicit test delivery");
        assert_eq!(f.snapshot()["records"][0]["status"],"accepted");
        transaction(&f.root,|s|{assert_eq!(serde_json::to_value(&s.baseline).unwrap(),baseline);assert!(!s.baseline_reset);Ok(())}).unwrap();
        assert!(!f.service.inner.baseline_needed.load(Ordering::Acquire));
        f.service.observe(&[task("completed",2,json!([]))]).unwrap();
        assert_eq!(f.snapshot()["records"][0]["event"],"task_completed");
    }
}

#[test]
fn corrupt_task_store_still_syncs_and_dispatches_weixin(){
    let f=Fixture::new();configure_weixin(&f,vec![Ok(json!({"ret":0}))]);
    fs::write(f.root.join("events.sqlite3"),b"corrupt SQLite fixture").unwrap();
    f.service.call("notification_test",json!({"id":"wechat"})).unwrap();
    f.service.inner.weixin.lock().unwrap().as_ref().unwrap().halt().unwrap();
    assert_eq!(weixin_snapshot(&f.service.inner)["active"],false);
    assert!(worker_cycle(&f.service.inner,|_,_|panic!("wrong transport")).is_err());
    assert_eq!(weixin_snapshot(&f.service.inner)["active"],true);
    assert_eq!(f.snapshot()["records"][0]["status"],"accepted");
}

#[test]
fn channel_activation_suppresses_pre_enable_changes_for_shared_baseline(){
    let f=Fixture::new();f.configure();f.service.observe(&[task("running",1,json!([]))]).unwrap();
    f.service.call("notification_channel_save",json!({"id":"wecom","enabled":false})).unwrap();
    // Completion happens before enable but has not yet been polled.
    let completed=task("completed",2,json!([]));
    f.service.call("notification_channel_save",json!({"id":"wecom","enabled":true})).unwrap();
    f.service.observe(&[completed]).unwrap();assert_eq!(f.snapshot()["records"],json!([]));
    // Same persisted reset contract applies to Weixin activation.
    f.service.inner.weixin.lock().unwrap().take();
    *f.service.inner.weixin.lock().unwrap()=Some(Arc::new(crate::weixin::notification_fixture(f.root.join("weixin-production"),vec![])));
    f.service.call("notification_channel_save",json!({"id":"wechat","enabled":true})).unwrap();
    f.service.observe(&[task("blocked",3,json!([{"id":"old","open":true}]))]).unwrap();
    assert_eq!(f.snapshot()["records"],json!([]));
    f.service.observe(&[task("completed",4,json!([]))]).unwrap();
    let records=f.snapshot()["records"].as_array().unwrap().clone();
    assert_eq!(records.len(),2);assert!(records.iter().all(|r|r["event"]=="task_completed"));
}

#[test]
fn shutdown_drains_observer_and_inflight_worker_cycle(){
    let f=Fixture::new();f.configure();f.service.observe(&[task("running",1,json!([]))]).unwrap();
    thread::scope(|scope|{
        let disk=lock(&f.root).unwrap();
        let observer=scope.spawn(||f.service.observe(&[task("completed",2,json!([]))]));
        wait_until(||f.service.inner.lifecycle.try_write().is_err());
        let shutdown=scope.spawn(||f.service.shutdown());
        wait_until(||f.service.inner.stopped.load(Ordering::Acquire));
        drop(disk);let _=observer.join().unwrap();shutdown.join().unwrap();
    });
    // A separate fixture models the worker's production lifecycle/activity guards.
    let f=Fixture::new();f.configure();f.service.call("notification_test",json!({"id":"wecom"})).unwrap();
    let (started,rx)=std::sync::mpsc::channel();let (release,wait)=std::sync::mpsc::channel();
    thread::scope(|scope|{
        let inner=&f.service.inner;
        let worker=scope.spawn(move ||{
            let _lifecycle=inner.lifecycle.read().unwrap();
            let _activity=crate::runtime::ProcessLock::acquire(&inner.root.join("notification-activity.lock")).unwrap();
            worker_cycle(inner,|_,_|{started.send(()).unwrap();wait.recv().unwrap();Outcome::Accepted})
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // Snapshot is allowed while dispatch owns a network request.
        assert_eq!(f.snapshot()["records"][0]["status"],"pending");
        let shutdown=scope.spawn(||f.service.shutdown());
        wait_until(||f.service.inner.stopped.load(Ordering::Acquire));
        assert!(f.service.inner.lifecycle.try_write().is_err());
        release.send(()).unwrap();assert!(worker.join().unwrap().is_err());shutdown.join().unwrap();
    });
    transaction(&f.root,|s|{assert_eq!(s.records[0].status,"accepted");Ok(())}).unwrap();
}

fn wait_until(mut condition:impl FnMut()->bool){let deadline=std::time::Instant::now()+Duration::from_secs(2);while !condition(){assert!(std::time::Instant::now()<deadline,"coordination timed out");thread::yield_now();}}

#[test]
fn shutdown_drains_waiting_foreground_rechecks_stop_and_releases_account(){
    let f=Fixture::new();f.snapshot();
    let retained_inner=Arc::clone(&f.service.inner);
    thread::scope(|scope|{
        let dispatch=f.service.inner.dispatch.lock().unwrap();
        let caller=scope.spawn(||f.service.call("notification_save",json!({"enabled":true})));
        wait_until(||f.service.inner.lifecycle.try_write().is_err());
        let shutdown=scope.spawn(||f.service.shutdown());
        wait_until(||f.service.inner.stopped.load(Ordering::Acquire));
        drop(dispatch);
        assert!(caller.join().unwrap().is_err());shutdown.join().unwrap();
    });
    assert!(retained_inner.weixin.lock().unwrap().is_none());
    let new_account=crate::weixin::Weixin::new(f.root.join("weixin-production")).unwrap();new_account.shutdown();drop(new_account);
    assert!(weixin(&retained_inner).is_err());
    assert!(f.service.call("notification_snapshot",json!({})).is_err());
    assert!(f.service.observe(&[]).is_err());
    transaction(&f.root,|s|{assert!(!s.enabled);Ok(())}).unwrap();
}

#[test]
fn shutdown_drains_snapshot_transaction_without_waiting_for_dispatch(){
    let f=Fixture::new();f.snapshot();
    thread::scope(|scope|{
        let disk=lock(&f.root).unwrap();
        let caller=scope.spawn(||f.service.call("notification_snapshot",json!({})));
        wait_until(||f.service.inner.lifecycle.try_write().is_err());
        let (done,rx)=std::sync::mpsc::channel();
        let service=&f.service;
        let shutdown=scope.spawn(move ||{service.shutdown();done.send(()).unwrap();});
        wait_until(||f.service.inner.stopped.load(Ordering::Acquire));
        assert!(rx.try_recv().is_err());
        drop(disk);caller.join().unwrap().unwrap();shutdown.join().unwrap();rx.recv().unwrap();
    });
    assert!(f.service.inner.weixin.lock().unwrap().is_none());
}

struct Fixture { root:PathBuf, service:NotificationService }
impl Fixture {
    fn new()->Self {
        let root=std::env::temp_dir().join(format!("floatboard-notifications-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let service=NotificationService{inner:Arc::new(Inner{root:root.clone(),lifecycle:RwLock::new(()),stopped:AtomicBool::new(false),baseline_needed:AtomicBool::new(true),error:Mutex::new(None),weixin:Mutex::new(None),dispatch:Mutex::new(())}),worker:Mutex::new(None)};
        Self{root,service}
    }
    fn configure(&self){
        self.service.call("notification_channel_save",json!({"id":"wecom","enabled":true,"webhook":"https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=fixture-key","keyword":"fixture"})).unwrap();
        self.service.call("notification_save",json!({"enabled":true})).unwrap();
    }
    fn snapshot(&self)->Value{self.service.call("notification_snapshot",json!({})).unwrap()}
}
impl Drop for Fixture{fn drop(&mut self){self.service.shutdown();let _=fs::remove_dir_all(&self.root);}}
fn task(status:&str,revision:i64,blockers:Value)->Value{json!({"id":"task-1","title":"fixture title","status":status,"revision":revision,"blockers":blockers,"runtime":"idle"})}
#[test]
fn url_allowlist_and_canonicalization() {
    assert!(validate_url("wecom","https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=fixture-key").is_ok());
    assert!(validate_url("feishu","https://open.feishu.cn/open-apis/bot/v2/hook/fixture-token").is_ok());
    for url in ["http://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x","https://qyapi.weixin.qq.com.evil.test/cgi-bin/webhook/send?key=x","https://user@qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x","https://qyapi.weixin.qq.com:8443/cgi-bin/webhook/send?key=x","https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x&other=y","https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=","https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x#fragment","https://qyapi.weixin.qq.com/other/../cgi-bin/webhook/send?key=x","https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=%78","https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x\n"] {assert!(validate_url("wecom",url).is_err(),"{url:?}");}
    for url in ["https://open.feishu.cn/open-apis/bot/v2/hook/","https://open.feishu.cn/open-apis/bot/v2/hook/a/b","https://open.feishu.cn/open-apis/bot/v2/hook/x?key=x","https://open.feishu.cn/open-apis/bot/v2/hook/%78"]{assert!(validate_url("feishu",url).is_err());}
}
#[test]
fn feishu_hmac_known_vector(){assert_eq!(feishu_signature("1700000000","test-secret"),"mbm4Y4oluIPQ00qlBIhX8vAZ0EKv3nw0LuTb91jPL84=");}
#[test]
fn provider_response_classification_and_signed_payload(){
    assert_eq!(classify_response("wecom",200,br#"{"errcode":0}"#),Outcome::Accepted);
    assert_eq!(classify_response("feishu",200,br#"{"code":0}"#),Outcome::Accepted);
    assert_eq!(classify_response("feishu",200,br#"{"StatusCode":0}"#),Outcome::Accepted);
    for code in [429,500,502,503]{assert!(matches!(classify_response("wecom",code,b""),Outcome::Temporary(_)));}
    for code in [301,302,400,401,403]{assert!(matches!(classify_response("wecom",code,b""),Outcome::Permanent(_)));}
    assert!(matches!(classify_response("wecom",200,br#"{"errcode":45009}"#),Outcome::Temporary(_)));
    assert!(matches!(classify_response("feishu",200,br#"{"code":99991400}"#),Outcome::Temporary(_)));
    for response in [b"{}".as_slice(),b"not json",br#"{"code":19021,"msg":"secret URL must not surface"}"#]{assert!(matches!(classify_response("feishu",200,response),Outcome::Permanent(_)));}
    let f=Fixture::new();f.configure();f.service.call("notification_test",json!({"id":"wecom"})).unwrap();
    transaction(&f.root,|s|{
        let record=&s.records[0];assert_eq!(payload(&s.channels[1],record,"1700000000")?["text"]["content"],"fixture\n任务侧窗 · 测试通知");
        let c=Channel{id:"feishu".into(),signing_secret:protect("test-secret")?,..Channel::default()};
        let p=payload(&c,record,"1700000000")?;assert_eq!(p["timestamp"],"1700000000");assert_eq!(p["sign"],"mbm4Y4oluIPQ00qlBIhX8vAZ0EKv3nw0LuTb91jPL84=");assert_eq!(p["msg_type"],"text");Ok(())
    }).unwrap();
}
#[test]
fn credentials_dpapi_preserved_and_never_exposed(){
    let f=Fixture::new();f.configure();
    let result=f.service.call("notification_channel_save",json!({"id":"feishu","enabled":true,"webhook":"https://open.feishu.cn/open-apis/bot/v2/hook/fixture-token","keyword":"keyword","signing_secret":"fixture-secret"})).unwrap();
    assert_eq!(result["channels"][2]["has_signing_secret"],true);
    f.service.call("notification_channel_save",json!({"id":"feishu","enabled":true,"webhook":"","keyword":"new","signing_secret":""})).unwrap();
    let raw=fs::read_to_string(f.root.join("notifications.json")).unwrap();
    for secret in ["fixture-secret","fixture-token","fixture-key","https://"]{assert!(!raw.contains(secret));assert!(!f.snapshot().to_string().contains(secret));}
    transaction(&f.root,|s|{assert_eq!(unprotect(&s.channels[2].signing_secret)?,"fixture-secret");Ok(())}).unwrap();
    let result=f.service.call("notification_channel_save",json!({"id":"feishu","enabled":true,"keyword":"","clear_signing_secret":true})).unwrap();assert_eq!(result["channels"][2]["has_signing_secret"],false);
}
#[test]
fn first_run_restart_enable_baseline_and_real_transitions(){
    let f=Fixture::new();f.configure();
    let running=task("running",1,json!([]));f.service.observe(&[running.clone()]).unwrap();
    assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),0);
    let opened=task("blocked",2,json!([{"id":"b","open":true,"title":"dependency"}]));f.service.observe(&[opened.clone()]).unwrap();
    assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),1);
    f.service.observe(&[opened.clone()]).unwrap();assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),1);
    // Disappearance is not an explicit resolution, and idle is never completion.
    f.service.call("notification_save",json!({"enabled":true,"subscriptions":{"blocker_resolved":true}})).unwrap();
    f.service.observe(&[task("running",3,json!([]))]).unwrap();assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),1);
    f.service.observe(&[opened.clone()]).unwrap();
    f.service.observe(&[task("running",4,json!([{"id":"b","open":false,"title":"dependency"}]))]).unwrap();
    assert!(f.snapshot()["records"].as_array().unwrap().iter().any(|r|r["event"]=="blocker_resolved"));
    let before=f.snapshot()["records"].as_array().unwrap().len();
    f.service.inner.baseline_needed.store(true,Ordering::Release);
    f.service.observe(&[task("completed",5,json!([]))]).unwrap();assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),before);
    f.service.call("notification_save",json!({"enabled":false})).unwrap();
    f.service.call("notification_save",json!({"enabled":true})).unwrap();
    f.service.observe(&[opened]).unwrap();assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),before);
}
#[test]
fn archives_unreported_excluded_and_new_task_completion_is_event(){
    let f=Fixture::new();f.configure();f.service.observe(&[]).unwrap();
    let mut archive=task("completed",1,json!([]));archive["archived"]=json!(true);
    f.service.observe(&[archive,task("unreported",1,json!([]))]).unwrap();assert_eq!(f.snapshot()["records"],json!([]));
    f.service.observe(&[task("completed",2,json!([]))]).unwrap();assert_eq!(f.snapshot()["records"][0]["event"],"task_completed");
}
#[test]
fn queue_retry_expiry_permanent_errors_and_acceptance(){
    let f=Fixture::new();f.configure();f.service.call("notification_test",json!({"id":"wecom"})).unwrap();
    deliver_one(&f.service.inner,|_,_|Outcome::Temporary("network unavailable")).unwrap();assert_eq!(f.snapshot()["records"][0]["attempts"],1);assert_eq!(f.snapshot()["records"][0]["status"],"pending");
    for _ in 0..2 {transaction(&f.root,|s|{s.records[0].next_at=0.;Ok(())}).unwrap();deliver_one(&f.service.inner,|_,_|Outcome::Temporary("network unavailable")).unwrap();}
    assert_eq!(f.snapshot()["records"][0]["status"],"failed");assert_eq!(f.snapshot()["records"][0]["attempts"],3);
    let id=f.snapshot()["records"][0]["id"].clone();f.service.call("notification_retry",json!({"id":id})).unwrap();
    deliver_one(&f.service.inner,|_,_|Outcome::Permanent("rejected")).unwrap();assert_eq!(f.snapshot()["records"][0]["attempts"],1);assert_eq!(f.snapshot()["records"][0]["status"],"failed");
    f.service.call("notification_test",json!({"id":"wecom"})).unwrap();deliver_one(&f.service.inner,|_,_|Outcome::Accepted).unwrap();assert_eq!(f.snapshot()["records"][0]["status"],"accepted");
    f.service.call("notification_test",json!({"id":"wecom"})).unwrap();transaction(&f.root,|s|{s.records.last_mut().unwrap().created_at=now()-EXPIRY-1.;Ok(())}).unwrap();
    deliver_one(&f.service.inner,|_,_|panic!("expired entry must not send")).unwrap();assert_eq!(f.snapshot()["records"][0]["status"],"expired");
}
#[test]
fn snapshot_does_not_wait_for_network_and_claim_is_exclusive(){
    let f=Fixture::new();f.configure();f.service.call("notification_test",json!({"id":"wecom"})).unwrap();
    let inner=Arc::clone(&f.service.inner);let (started,rx)=std::sync::mpsc::channel();let (release,wait)=std::sync::mpsc::channel();
    let worker=thread::spawn(move||deliver_one(&inner,|_,_|{started.send(()).unwrap();wait.recv().unwrap();Outcome::Accepted}));
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let start=std::time::Instant::now();assert_eq!(f.snapshot()["records"][0]["status"],"pending");assert!(start.elapsed()<Duration::from_millis(500));
    deliver_one(&f.service.inner,|_,_|panic!("leased record must not be sent twice")).unwrap();
    release.send(()).unwrap();worker.join().unwrap().unwrap();
}
#[test]
fn persisted_reset_reopen_and_crash_lease_resume(){
    let f=Fixture::new();f.configure();
    let opened=task("blocked",1,json!([{"id":"stable","open":true,"title":"fixture"}]));
    f.service.observe(&[opened.clone()]).unwrap();
    f.service.observe(&[task("running",2,json!([{"id":"stable","open":false}]))]).unwrap();
    f.service.observe(&[task("blocked",3,json!([{"id":"stable","open":true}]))]).unwrap();
    assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),1);
    // A different instance consumes the persisted enable reset, even if its own
    // in-memory first-observation flag was already consumed before the toggle.
    let other=Inner{root:f.root.clone(),lifecycle:RwLock::new(()),stopped:AtomicBool::new(false),baseline_needed:AtomicBool::new(false),error:Mutex::new(None),weixin:Mutex::new(None),dispatch:Mutex::new(())};
    f.service.call("notification_save",json!({"enabled":false})).unwrap();
    f.service.call("notification_save",json!({"enabled":true})).unwrap();
    observe(&other,&[task("completed",4,json!([]))]).unwrap();assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),1);
    f.service.call("notification_test",json!({"id":"wecom"})).unwrap();
    transaction(&f.root,|s|{let r=s.records.last_mut().unwrap();r.attempts=1;r.lease_until=now()+30.;Ok(())}).unwrap();
    deliver_one(&other,|_,_|panic!("live lease cannot send")).unwrap();
    transaction(&f.root,|s|{s.records.last_mut().unwrap().lease_until=now()-1.;Ok(())}).unwrap();
    deliver_one(&other,|_,r|{assert_eq!(r.attempts,2);Outcome::Accepted}).unwrap();assert_eq!(f.snapshot()["records"][0]["status"],"accepted");
}
fn configure_weixin(f:&Fixture,replies:Vec<Result<Value>>){let w=crate::weixin::notification_fixture(f.root.join("weixin-production"),replies);*f.service.inner.weixin.lock().unwrap()=Some(Arc::new(w));f.service.call("notification_channel_save",json!({"id":"wechat","enabled":true})).unwrap();f.service.call("notification_save",json!({"enabled":true})).unwrap();}
#[test]fn weixin_task_outbox_acceptance_and_ambiguous_no_retry(){let f=Fixture::new();configure_weixin(&f,vec![Ok(json!({"ret":0})),Err("timeout after dispatch".into())]);f.service.observe(&[task("running",1,json!([]))]).unwrap();f.service.observe(&[task("completed",2,json!([]))]).unwrap();deliver_one(&f.service.inner,|_,_|panic!("wrong transport")).unwrap();assert_eq!(f.snapshot()["records"][0]["status"],"accepted");f.service.call("notification_test",json!({"id":"wechat"})).unwrap();deliver_one(&f.service.inner,|_,_|panic!("wrong transport")).unwrap();let s=f.snapshot();assert_eq!(s["records"][0]["status"],"unknown");assert!(f.service.call("notification_retry",json!({"id":s["records"][0]["id"]})).is_err());deliver_one(&f.service.inner,|_,_|panic!("unknown must not retry")).unwrap();}
#[test]fn weixin_crash_lease_disconnect_and_disable_invalidate(){let f=Fixture::new();configure_weixin(&f,vec![]);f.service.call("notification_test",json!({"id":"wechat"})).unwrap();transaction(&f.root,|s|{s.records[0].attempts=1;s.records[0].lease_until=now()-1.;Ok(())}).unwrap();deliver_one(&f.service.inner,|_,_|panic!("ambiguous crash must not resend")).unwrap();assert_eq!(f.snapshot()["records"][0]["status"],"unknown");f.service.call("notification_test",json!({"id":"wechat"})).unwrap();f.service.call("notification_weixin_action",json!({"action":"disconnect"})).unwrap();let s=f.snapshot();assert_eq!(s["weixin"]["bound"],false);assert_eq!(s["channels"][0]["enabled"],false);assert_eq!(s["records"][0]["status"],"skipped");deliver_one(&f.service.inner,|_,_|panic!("disconnected queue must not send")).unwrap();}
#[test]
fn defaults_unbound_wechat_rejection_retention_and_clear(){
    let f=Fixture::new();assert_eq!(f.snapshot()["enabled"],false);assert_eq!(f.snapshot()["subscriptions"]["blocker_opened"],true);
    for method in ["notification_channel_save","notification_test"]{assert!(f.service.call(method,json!({"id":"wechat","enabled":true})).is_err());}
    transaction(&f.root,|s|{for _ in 0..510{enqueue(s,"wecom","test","fixture",None);}prune(s,now());assert_eq!(s.records.len(),500);s.records[0].created_at=now()-RETENTION-1.;prune(s,now());assert_eq!(s.records.len(),499);s.records[0].status="accepted".into();Ok(())}).unwrap();
    f.service.call("notification_clear",json!({})).unwrap();assert_eq!(f.snapshot()["records"].as_array().unwrap().len(),498);
}
