//! Exact-label capabilities for independently owned utility WebViews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Settings, Integration, Updates, Notifications, WeixinLab }
impl Kind {
    pub fn parse(value:&str)->Option<Self>{Some(match value{"settings"=>Self::Settings,"integration"=>Self::Integration,"updates"=>Self::Updates,"notifications"=>Self::Notifications,"weixin-lab"=>Self::WeixinLab,_=>return None})}
    pub fn from_label(label:&str)->Option<Self>{Self::parse(label.strip_prefix("utility-")?)}
    pub fn name(self)->&'static str{match self{Self::Settings=>"settings",Self::Integration=>"integration",Self::Updates=>"updates",Self::Notifications=>"notifications",Self::WeixinLab=>"weixin-lab"}}
    pub fn label(self)->String{format!("utility-{}",self.name())}
    pub fn title(self)->&'static str{match self{Self::Settings=>"看板设置",Self::Integration=>"Codex 接入",Self::Updates=>"版本更新",Self::Notifications=>"消息通知",Self::WeixinLab=>"微信连接验证 · DEV"}}
    pub fn size(self,width:f64,height:f64)->(f64,f64){let desired:(f64,f64)=match self{Self::Settings=>(580.,760.),Self::Integration=>(720.,720.),Self::Updates=>(600.,640.),Self::Notifications=>(760.,740.),Self::WeixinLab=>(620.,720.)};(desired.0.min((width-24.).max(1.)),desired.1.min((height-24.).max(1.)))}
    pub fn bridge(self,method:&str)->bool{method=="snapshot"||match self{Self::Settings=>matches!(method,"preferences"|"data_location"|"migrate_data"|"database_repair"),Self::Integration=>matches!(method,"preview"|"connect"|"connect_request"|"disconnect"),Self::Notifications=>matches!(method,"notification_snapshot"|"notification_save"|"notification_channel_save"|"notification_channel_delete"|"notification_test"|"notification_retry"|"notification_clear"|"notification_weixin_action"),_=>false}}
    pub fn host(self,action:&str)->bool{matches!(action,"utility_busy"|"utility_close"|"drag"|"window_state"|"copy")||match self{Self::Settings=>matches!(action,"data_folder"|"restart"|"toast"|"utility_open"),Self::Updates=>action=="open_reference",_=>false}}
}
#[cfg(test)]mod tests{use super::*;
#[test]fn capabilities_are_exact(){assert_eq!(Kind::from_label("utility-settings-other"),None);assert_eq!(Kind::from_label("settings"),None);for kind in [Kind::Settings,Kind::Integration,Kind::Updates,Kind::Notifications,Kind::WeixinLab]{assert!(kind.bridge("snapshot"));assert!(!kind.bridge("window_save"));assert!(!kind.host("quit"));assert!(!kind.host("pin"));}assert!(Kind::Notifications.bridge("notification_test"));assert!(Kind::Notifications.bridge("notification_weixin_action"));assert!(Kind::Settings.bridge("database_repair"));assert!(!Kind::Notifications.bridge("database_repair"));assert!(!Kind::Settings.bridge("notification_weixin_action"));assert!(!Kind::Integration.bridge("preferences"));assert!(!Kind::WeixinLab.bridge("connect"));}
#[test]fn dimensions_fit_small_workarea(){for kind in [Kind::Settings,Kind::Integration,Kind::Updates,Kind::Notifications,Kind::WeixinLab]{let(w,h)=kind.size(360.,480.);assert!(w<=336.&&h<=456.&&w>0.&&h>0.);}assert_ne!(Kind::Settings.size(1920.,1080.),Kind::Notifications.size(1920.,1080.));}}
