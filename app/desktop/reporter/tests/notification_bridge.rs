use std::{fs,path::PathBuf};
use serde_json::json;
use floatboard_core::{desktop::Engine,store};
struct Temp(PathBuf);
impl Temp {fn new()->Self{let path=std::env::temp_dir().join(format!("floatboard-notification-bridge-{}",uuid::Uuid::new_v4().simple()));fs::create_dir_all(&path).unwrap();Self(path)}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
#[test]
fn bridge_routes_notifications_and_rejects_unverified_wechat(){
    let temp=Temp::new();
    let mut engine=Engine::new(temp.0.clone(),temp.0.join("fake-gui.exe"));
    let initial=engine.call("notification_snapshot",json!({})).unwrap();
    assert_eq!(initial["enabled"],false);
    assert_eq!(initial["channels"].as_array().unwrap().len(),3);
    assert!(engine.call("notification_channel_save",json!({"id":"wechat","enabled":true})).is_err());
    assert!(engine.call("notification_test",json!({"id":"wechat"})).is_err());
    let saved=engine.call("notification_save",json!({"enabled":true,"subscriptions":{"task_completed":false}})).unwrap();
    assert_eq!(saved["enabled"],true);assert_eq!(saved["subscriptions"]["task_completed"],false);
    assert!(engine.call("notification_shell",json!({})).is_err());
    drop(engine); // Join background worker before cleaning its own UUID sandbox.
}
#[test]
fn stale_engine_rejects_notification_changes_after_redirect(){
    let temp=Temp::new();let old=temp.0.join("old");let next=temp.0.join("next");fs::create_dir_all(&old).unwrap();fs::create_dir_all(&next).unwrap();
    let mut engine=Engine::new(old.clone(),temp.0.join("fake-gui.exe"));
    engine.call("notification_snapshot",json!({})).unwrap();
    store::atomic_json(&old.join(floatboard_core::migration::REDIRECT),&json!({"version":1,"path":next})).unwrap();
    assert!(engine.call("notification_save",json!({"enabled":true})).unwrap_err().contains("重启"));
    assert!(!next.join("notifications.json").exists());
    drop(engine);
}
