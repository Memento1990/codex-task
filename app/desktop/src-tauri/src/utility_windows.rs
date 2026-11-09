//! Exact-label capabilities for independently owned utility WebViews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Settings, Integration, Updates, Notifications, Maintenance, WeixinLab }
impl Kind {
    pub fn parse(value:&str)->Option<Self>{Some(match value{"settings"=>Self::Settings,"integration"=>Self::Integration,"updates"=>Self::Updates,"notifications"=>Self::Notifications,"maintenance"=>Self::Maintenance,"weixin-lab"=>Self::WeixinLab,_=>return None})}
    pub fn from_label(label:&str)->Option<Self>{Self::parse(label.strip_prefix("utility-")?)}
    pub fn name(self)->&'static str{match self{Self::Settings=>"settings",Self::Integration=>"integration",Self::Updates=>"updates",Self::Notifications=>"notifications",Self::Maintenance=>"maintenance",Self::WeixinLab=>"weixin-lab"}}
    pub fn label(self)->String{format!("utility-{}",self.name())}
    pub fn title(self)->&'static str{match self{Self::Settings=>"看板设置",Self::Integration=>"Codex 接入",Self::Updates=>"版本更新",Self::Notifications=>"消息通知",Self::Maintenance=>"备份与诊断",Self::WeixinLab=>"微信连接验证 · DEV"}}
    pub fn size(self,width:f64,height:f64)->(f64,f64){(560f64.min((width-24.).max(1.)),640f64.min((height-24.).max(1.)))}
    pub fn bridge(self,method:&str)->bool{["watch","snapshot"].contains(&method)||match self{Self::Settings=>matches!(method,"preferences"|"data_location"),Self::Integration=>matches!(method,"preview"|"connect"|"connect_request"|"disconnect"|"pairing_list"|"pairing_approve"|"pairing_revoke"),Self::Notifications=>matches!(method,"notification_snapshot"|"notification_save"|"notification_channel_save"|"notification_channel_delete"|"notification_test"|"notification_retry"|"notification_clear"|"notification_weixin_action"),Self::Maintenance=>matches!(method,"backup"|"diagnostics"|"data_location"),_=>false}}
    pub fn host(self,action:&str)->bool{matches!(action,"utility_busy"|"utility_close"|"drag"|"window_state"|"copy"|"show")||match self{Self::Settings=>matches!(action,"data_folder"|"restart"|"toast"|"utility_open"),Self::Updates=>action=="open_reference",_=>false}}
}
#[cfg(test)]mod tests{use super::*;
#[test]fn capabilities_are_exact(){assert_eq!(Kind::from_label("utility-settings-other"),None);assert_eq!(Kind::from_label("utility-repair"),None);for kind in [Kind::Settings,Kind::Integration,Kind::Updates,Kind::Notifications,Kind::Maintenance,Kind::WeixinLab]{assert!(kind.bridge("snapshot"));assert!(!kind.bridge("database_repair"));assert!(!kind.bridge("migrate_data"));assert!(!kind.host("quit"));}assert!(Kind::Integration.bridge("pairing_approve"));assert!(!Kind::Notifications.bridge("pairing_approve"));assert!(Kind::Maintenance.bridge("backup"));assert!(!Kind::Settings.bridge("backup"));}
#[test]fn dimensions_fit_small_workarea(){let(w,h)=Kind::Maintenance.size(360.,480.);assert!(w<=336.&&h<=456.&&w>0.&&h>0.);}}
