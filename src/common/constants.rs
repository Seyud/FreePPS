// inotify 相关常量
#[cfg(unix)]
pub const IN_MODIFY: u32 = 0x00000002;
#[cfg(unix)]
pub const IN_CLOSE_WRITE: u32 = 0x00000008;
#[cfg(unix)]
pub const IN_CREATE: u32 = 0x00000100;
#[cfg(unix)]
pub const IN_DELETE: u32 = 0x00000200;

// 文件路径常量
#[cfg(unix)]
pub const MODULE_BASE_PATH: &str = "/data/adb/modules/FreePPS";
pub const FREE_FILE: &str = "/data/adb/modules/FreePPS/free";
pub const AUTO_FILE: &str = "/data/adb/modules/FreePPS/auto";
pub const DISABLE_FILE: &str = "/data/adb/modules/FreePPS/disable";
#[cfg(unix)]
pub const MODULE_PROP: &str = "/data/adb/modules/FreePPS/module.prop";
pub const PD_VERIFIED_PATH: &str = "/sys/class/qcom-battery/pd_verifed";
pub const PD_ADAPTER_VERIFIED_PATH: &str = "/sys/class/Charging_Adapter/pd_adapter/usbpd_verifed";
pub const BATTERY_STATUS_PATH: &str = "/sys/class/power_supply/battery/status";
pub const INPUT_SUSPEND_PATH: &str = "/sys/class/qcom-battery/input_suspend";
pub const TYPEC_MODE_PATH: &str = "/sys/class/qcom-battery/typec_mode";
pub const USB_REAL_TYPE_PATH: &str = "/sys/class/qcom-battery/usb_real_type";

// 金标动画广播伪造相关 sysfs 节点
#[cfg(unix)]
pub const REAL_TYPE_PATH: &str = "/sys/class/xm_power/charger/charger_common/real_type";
#[cfg(unix)]
pub const APDO_MAX_PATH: &str = "/sys/class/xm_power/typec/apdo_max";
#[cfg(unix)]
pub const ADAPTER_SVID_PATH: &str = "/sys/class/xm_power/typec/strategy_pd_auth/adapter_svid";
#[cfg(unix)]
pub const USB_VOLTAGE_NOW_PATH: &str = "/sys/class/power_supply/usb/voltage_now";
