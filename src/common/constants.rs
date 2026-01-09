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
pub const DISABLE_FILE: &str = "/data/adb/modules/FreePPS/disable";
#[cfg(unix)]
pub const MODULE_PROP: &str = "/data/adb/modules/FreePPS/module.prop";
pub const PD_VERIFIED_PATH: &str = "/sys/class/qcom-battery/pd_verifed";
pub const PD_ADAPTER_VERIFIED_PATH: &str = "/sys/class/Charging_Adapter/pd_adapter/usbpd_verifed";
