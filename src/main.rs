use anyhow::Result;
use std::ffi::CString;
use std::fs;
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
// inotify 相关常量
const IN_MODIFY: u32 = 0x00000002;
const IN_CLOSE_WRITE: u32 = 0x00000008;
const IN_CREATE: u32 = 0x00000100;
const IN_DELETE: u32 = 0x00000200;

// 文件路径常量
const MODULE_BASE_PATH: &str = "/data/adb/modules/FreePPS";
const FREE_FILE: &str = "/data/adb/modules/FreePPS/free";
const DISABLE_FILE: &str = "/data/adb/modules/FreePPS/disable";
const MODULE_PROP: &str = "/data/adb/modules/FreePPS/module.prop";
const PD_VERIFIED_PATH: &str = "/sys/class/qcom-battery/pd_verifed";

// 获取当前时间戳（YYYY-MM-DD HH:MM:SS.mmm 格式）
fn get_timestamp() -> String {
    // 获取当前 Unix 时间戳
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let total_secs = now.as_secs();
    let millis = now.subsec_millis();

    // 尝试获取本地时区偏移（优先读取Android系统时区）
    let timezone_offset = get_timezone_offset();
    let adjusted_secs = total_secs.saturating_add(timezone_offset as u64);

    // 计算真实的日期时间（基于1970年1月1日，考虑时区）
    // 处理闰年和平年
    let mut remaining_secs = adjusted_secs;
    let mut year = 1970;

    // 计算年份
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        let secs_in_year = days_in_year * 24 * 3600;

        if remaining_secs >= secs_in_year {
            remaining_secs -= secs_in_year;
            year += 1;
        } else {
            break;
        }
    }

    // 计算一年中的第几天
    let day_of_year = remaining_secs / (24 * 3600);
    let secs_today = remaining_secs % (24 * 3600);

    // 计算时分秒
    let hour = secs_today / 3600;
    let minute = (secs_today % 3600) / 60;
    let second = secs_today % 60;

    // 计算月份和日期
    let (month, day) = get_month_day_from_year_day(year, day_of_year);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        year, month, day, hour, minute, second, millis
    )
}

/// 获取本地时区偏移（秒）
fn get_timezone_offset() -> i64 {
    // 方法1: 尝试读取Android系统时区文件
    if let Ok(content) = fs::read_to_string("/system/usr/share/zoneinfo/tzdata") {
        // 简化处理：如果是中国时区，返回+8小时
        if content.contains("Asia/Shanghai") || content.contains("CST") {
            return 8 * 3600;
        }
    }

    // 方法2: 尝试读取环境变量
    if let Ok(tz) = std::env::var("TZ")
        && (tz.contains("Asia/Shanghai") || tz.contains("Hongkong"))
    {
        return 8 * 3600;
    }

    // 方法3: 尝试读取系统属性（Android常用）
    if let Ok(output) = std::process::Command::new("getprop")
        .arg("persist.sys.timezone")
        .output()
        && output.status.success()
    {
        let timezone = String::from_utf8_lossy(&output.stdout);
        if timezone.contains("Asia/Shanghai") || timezone.contains("Hongkong") {
            return 8 * 3600;
        }
    }

    // 默认返回UTC+8（中国标准时间）
    8 * 3600
}

/// 判断是否为闰年
fn is_leap_year(year: u64) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

/// 根据年份和一年中的第几天，计算月份和日期
fn get_month_day_from_year_day(year: u64, day_of_year: u64) -> (u8, u8) {
    let days_in_month = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut remaining_days = day_of_year;
    let mut month = 1;

    for &days in &days_in_month {
        if remaining_days < days as u64 {
            return (month, (remaining_days + 1) as u8);
        }
        remaining_days -= days as u64;
        month += 1;
    }

    (12, 31) // 最后一天
}

// 简单的日志宏，替代重量级的 log 库
macro_rules! info {
    ($($arg:tt)*) => {
        let timestamp = get_timestamp();
        println!("[{}] [INFO] {}", timestamp, format!($($arg)*));
    };
}

macro_rules! warn {
    ($($arg:tt)*) => {
        let timestamp = get_timestamp();
        println!("[{}] [WARN] {}", timestamp, format!($($arg)*));
    };
}

macro_rules! error {
    ($($arg:tt)*) => {
        let timestamp = get_timestamp();
        eprintln!("[{}] [ERROR] {}", timestamp, format!($($arg)*));
    };
}

/// FreePPS监控错误类型
#[derive(Error, Debug)]
pub enum FreePPSError {
    #[error("系统文件操作失败: {0}")]
    FileOperation(#[from] std::io::Error),
    #[error("设置PD验证失败: {0}")]
    PdVerificationFailed(String),
    #[error("inotify监控失败: {0}")]
    InotifyError(String),
}

/// inotify监控器
struct InotifyMonitor {
    inotify_fd: c_int,
}

// 外部函数声明
unsafe extern "C" {
    fn inotify_init() -> c_int;
    fn inotify_add_watch(fd: c_int, pathname: *const c_char, mask: u32) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn close(fd: c_int) -> c_int;
}

impl InotifyMonitor {
    /// 读取文件内容
    fn read_file_content(path: &str) -> Result<String, FreePPSError> {
        if !Path::new(path).exists() {
            return Ok(String::new());
        }

        let content = fs::read_to_string(path)
            .map_err(FreePPSError::FileOperation)?
            .trim()
            .to_string();

        Ok(content)
    }

    /// 写入文件内容
    fn write_file_content(path: &str, content: &str) -> Result<(), FreePPSError> {
        fs::write(path, content).map_err(FreePPSError::FileOperation)?;
        Ok(())
    }

    /// 设置PD验证状态
    fn set_pd_verified(enable: bool) -> Result<(), FreePPSError> {
        let value = if enable { "1" } else { "0" };
        info!("设置PD验证状态为: {}", value);

        // 检查文件是否存在
        if !Path::new(PD_VERIFIED_PATH).exists() {
            return Err(FreePPSError::PdVerificationFailed(format!(
                "系统文件不存在: {}",
                PD_VERIFIED_PATH
            )));
        }

        // 写入前读取当前值，用于对比
        let current_value =
            Self::read_file_content(PD_VERIFIED_PATH).unwrap_or_else(|_| "unknown".to_string());
        info!(
            "PD验证文件当前值: {}, 准备写入: {}",
            current_value.trim(),
            value
        );

        // 写入值到系统文件
        Self::write_file_content(PD_VERIFIED_PATH, value)?;

        // 写入后验证
        let new_value =
            Self::read_file_content(PD_VERIFIED_PATH).unwrap_or_else(|_| "unknown".to_string());
        info!("PD验证文件新值: {}", new_value.trim());

        if new_value.trim() == value {
            info!("成功设置PD验证状态");
        } else {
            warn!(
                "PD验证状态设置可能失败，期望值: {}, 实际值: {}",
                value,
                new_value.trim()
            );
        }

        Ok(())
    }

    /// 读取当前PD验证状态
    fn read_pd_verified() -> Result<bool, FreePPSError> {
        let content = Self::read_file_content(PD_VERIFIED_PATH)?;
        Ok(content == "1")
    }

    /// 更新module.prop描述
    fn update_module_description(enabled: bool) -> Result<(), FreePPSError> {
        let prop_content = Self::read_file_content(MODULE_PROP)?;
        let new_description = if enabled {
            "[⚡✅PPS已支持] 启用搭载澎湃 P1、P2 芯片机型的公版 PPS 支持。（感谢\"酷安@低线阻狂魔\"提供方案）"
        } else {
            "[⚡⏸️PPS已暂停] 启用搭载澎湃 P1、P2 芯片机型的公版 PPS 支持。（感谢\"酷安@低线阻狂魔\"提供方案）"
        };

        let updated_content = prop_content
            .lines()
            .map(|line| {
                if line.starts_with("description=") {
                    format!("description={}", new_description)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Self::write_file_content(MODULE_PROP, &updated_content)?;
        info!("更新module.prop描述为: {}", new_description);
        Ok(())
    }

    /// 监控free文件
    fn monitor_free_file(running: Arc<AtomicBool>) -> Result<(), FreePPSError> {
        info!("启动free文件监控线程...");

        let inotify_fd = unsafe { inotify_init() };
        if inotify_fd == -1 {
            return Err(FreePPSError::InotifyError("无法初始化inotify".to_string()));
        }

        // 确保free文件存在
        if !Path::new(FREE_FILE).exists() {
            Self::write_file_content(FREE_FILE, "1")?;
        }

        // 添加inotify监控
        let path_cstring = CString::new(FREE_FILE)
            .map_err(|e| FreePPSError::InotifyError(format!("路径转换失败: {}", e)))?;

        let wd = unsafe {
            inotify_add_watch(
                inotify_fd,
                path_cstring.as_ptr(),
                IN_MODIFY | IN_CLOSE_WRITE,
            )
        };

        if wd == -1 {
            return Err(FreePPSError::InotifyError(format!(
                "无法监控文件: {}",
                FREE_FILE
            )));
        }

        info!("开始监控free文件: {}", FREE_FILE);
        let mut pd_monitor_active = false;

        // 主监控循环
        let mut buffer = [0u8; 1024];
        while running.load(Ordering::Relaxed) {
            let bytes_read =
                unsafe { read(inotify_fd, buffer.as_mut_ptr() as *mut c_void, buffer.len()) };

            if bytes_read == -1 {
                error!("读取inotify事件失败，继续监控...");
                thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            } else if bytes_read > 0 {
                info!("检测到free文件变化");

                let content = Self::read_file_content(FREE_FILE)?;
                info!("free文件内容: {}", content);

                if content == "1" {
                    info!("free文件为1，启动PD验证监控");
                    Self::update_module_description(true)?;

                    // 立即设置PD验证状态为1
                    if Path::new(PD_VERIFIED_PATH).exists() {
                        if let Err(e) = Self::set_pd_verified(true) {
                            error!("设置PD验证状态失败: {}", e);
                        }
                    } else {
                        warn!("PD验证文件不存在，跳过设置");
                    }

                    if !pd_monitor_active {
                        pd_monitor_active = true;
                    }
                } else if content == "0" {
                    info!("free文件为0，暂停PD验证监控");
                    Self::update_module_description(false)?;
                    pd_monitor_active = false;
                }
            }
        }

        unsafe { close(inotify_fd) };
        Ok(())
    }

    /// 监控disable文件
    fn monitor_disable_file(running: Arc<AtomicBool>) -> Result<(), FreePPSError> {
        info!("启动disable文件监控线程...");

        let inotify_fd = unsafe { inotify_init() };
        if inotify_fd == -1 {
            return Err(FreePPSError::InotifyError("无法初始化inotify".to_string()));
        }

        // 监控目录而不是具体文件，以便检测创建和删除事件
        let path_cstring = CString::new(MODULE_BASE_PATH)
            .map_err(|e| FreePPSError::InotifyError(format!("路径转换失败: {}", e)))?;

        let wd =
            unsafe { inotify_add_watch(inotify_fd, path_cstring.as_ptr(), IN_CREATE | IN_DELETE) };

        if wd == -1 {
            return Err(FreePPSError::InotifyError(format!(
                "无法监控目录: {}",
                MODULE_BASE_PATH
            )));
        }

        info!("开始监控disable文件: {}", DISABLE_FILE);
        let mut disable_exists = Path::new(DISABLE_FILE).exists();

        // 主监控循环
        let mut buffer = [0u8; 1024];
        while running.load(Ordering::Relaxed) {
            let bytes_read =
                unsafe { read(inotify_fd, buffer.as_mut_ptr() as *mut c_void, buffer.len()) };

            if bytes_read == -1 {
                error!("读取inotify事件失败，继续监控...");
                thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            } else if bytes_read > 0 {
                info!("检测到目录变化事件");

                let current_exists = Path::new(DISABLE_FILE).exists();

                if current_exists && !disable_exists {
                    info!("检测到disable文件创建");
                    // disable文件出现，设置free为0（free监控线程会处理描述更新）
                    Self::write_file_content(FREE_FILE, "0")?;
                    info!("已处理disable文件创建事件");
                } else if !current_exists && disable_exists {
                    info!("检测到disable文件删除");
                    // disable文件消失，设置free为1（free监控线程会处理描述更新和PD验证）
                    Self::write_file_content(FREE_FILE, "1")?;
                    info!("已处理disable文件删除事件");
                }

                disable_exists = current_exists;
            }
        }

        unsafe { close(inotify_fd) };
        Ok(())
    }

    /// 初始化模块状态
    fn initialize_module() -> Result<(), FreePPSError> {
        info!("开始模块初始化...");

        // 确保free文件存在
        if !Path::new(FREE_FILE).exists() {
            info!("free文件不存在，创建并设置为1");
            Self::write_file_content(FREE_FILE, "1")?;
        }

        // 确保disable文件不存在（模块启用状态）
        if Path::new(DISABLE_FILE).exists() {
            info!("检测到disable文件，删除以启用模块");
            fs::remove_file(DISABLE_FILE).map_err(FreePPSError::FileOperation)?;
        }

        // 读取当前free文件状态并主动更新描述
        let free_content = Self::read_file_content(FREE_FILE)?;
        info!("当前free文件内容: {}", free_content);

        if free_content == "1" {
            info!("模块启用状态，更新描述和PD验证");
            Self::update_module_description(true)?;

            // 主动设置PD验证状态
            if Path::new(PD_VERIFIED_PATH).exists() {
                // 先读取当前状态，如果已经是1就不重复设置
                match Self::read_pd_verified() {
                    Ok(is_verified) => {
                        if !is_verified {
                            info!("PD验证状态为0，设置为1");
                            Self::set_pd_verified(true)?;
                        } else {
                            info!("PD验证状态已经是1，跳过设置");
                        }
                    }
                    Err(e) => {
                        warn!("读取PD验证状态失败: {}，尝试直接设置为1", e);
                        Self::set_pd_verified(true)?;
                    }
                }
            } else {
                warn!("PD验证文件不存在，跳过初始化设置");
            }
        } else {
            info!("模块暂停状态，更新描述");
            Self::update_module_description(false)?;
        }

        info!("模块初始化完成");
        Ok(())
    }

    /// 监控PD验证状态（当free文件为1时启动)
    fn monitor_pd_verified(running: Arc<AtomicBool>) -> Result<(), FreePPSError> {
        info!("启动PD验证状态监控线程...");

        let inotify_fd = unsafe { inotify_init() };
        if inotify_fd == -1 {
            return Err(FreePPSError::InotifyError("无法初始化inotify".to_string()));
        }

        // 添加inotify监控
        let path_cstring = CString::new(PD_VERIFIED_PATH)
            .map_err(|e| FreePPSError::InotifyError(format!("路径转换失败: {}", e)))?;

        let wd = unsafe {
            inotify_add_watch(
                inotify_fd,
                path_cstring.as_ptr(),
                IN_MODIFY | IN_CLOSE_WRITE,
            )
        };

        if wd == -1 {
            return Err(FreePPSError::InotifyError(format!(
                "无法监控文件: {}",
                PD_VERIFIED_PATH
            )));
        }

        info!("开始监控PD验证状态: {}", PD_VERIFIED_PATH);

        // 主监控循环 - 纯inotify事件驱动
        let mut buffer = [0u8; 1024];

        while running.load(Ordering::Relaxed) {
            // inotify事件监控（非阻塞读取）
            let bytes_read = unsafe {
                // 设置非阻塞模式，但这里我们用超时机制
                read(inotify_fd, buffer.as_mut_ptr() as *mut c_void, buffer.len())
            };

            if bytes_read == -1 {
                // 没有事件时等待1秒
                thread::sleep(std::time::Duration::from_millis(1000));
            } else if bytes_read > 0 {
                // 检查是否应该处理（free文件为1）
                let free_content =
                    Self::read_file_content(FREE_FILE).unwrap_or_else(|_| "0".to_string());

                if free_content == "1" {
                    info!("检测到PD验证状态变化");

                    // 检查PD验证状态并重置为1
                    match Self::read_pd_verified() {
                        Ok(is_verified) => {
                            if !is_verified {
                                warn!("检测到PD验证状态被重置为0，重新设置为1");
                                if let Err(e) = Self::set_pd_verified(true) {
                                    error!("重新设置PD验证失败: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("读取PD验证状态失败: {}", e);
                        }
                    }
                }
            }
        }

        unsafe { close(inotify_fd) };
        Ok(())
    }
}

// 为 InotifyMonitor 实现 Drop trait 以清理资源
impl Drop for InotifyMonitor {
    fn drop(&mut self) {
        if self.inotify_fd != -1 {
            unsafe {
                close(self.inotify_fd);
            }
        }
    }
}

fn main() {
    info!("启动FreePPS");

    // 初始化阶段：确保基础文件存在并设置初始状态
    if let Err(e) = InotifyMonitor::initialize_module() {
        error!("模块初始化失败: {}", e);
    }

    // 创建运行标志
    let running = Arc::new(AtomicBool::new(true));
    let running_clone1 = Arc::clone(&running);
    let running_clone2 = Arc::clone(&running);
    let running_clone3 = Arc::clone(&running);

    // 创建free文件监控线程
    let mut free_thread = thread::spawn(move || {
        if let Err(e) = InotifyMonitor::monitor_free_file(running_clone1) {
            error!("free文件监控线程出错: {}", e);
        }
    });

    // 创建disable文件监控线程
    let mut disable_thread = thread::spawn(move || {
        if let Err(e) = InotifyMonitor::monitor_disable_file(running_clone2) {
            error!("disable文件监控线程出错: {}", e);
        }
    });

    // 创建PD验证监控线程
    let mut pd_thread = thread::spawn(move || {
        if let Err(e) = InotifyMonitor::monitor_pd_verified(running_clone3) {
            error!("PD验证监控线程出错: {}", e);
        }
    });

    // 主线程无限循环，保持程序运行
    info!("所有监控线程已启动，主线程进入守护模式...");
    loop {
        thread::sleep(std::time::Duration::from_secs(60));

        // 检查线程是否仍在运行，如果线程panic则重启
        if free_thread.is_finished() {
            warn!("free文件监控线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            free_thread = thread::spawn(move || {
                if let Err(e) = InotifyMonitor::monitor_free_file(running_clone) {
                    error!("重启的free文件监控线程出错: {}", e);
                }
            });
        }

        if disable_thread.is_finished() {
            warn!("disable文件监控线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            disable_thread = thread::spawn(move || {
                if let Err(e) = InotifyMonitor::monitor_disable_file(running_clone) {
                    error!("重启的disable文件监控线程出错: {}", e);
                }
            });
        }

        if pd_thread.is_finished() {
            warn!("PD验证监控线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            pd_thread = thread::spawn(move || {
                if let Err(e) = InotifyMonitor::monitor_pd_verified(running_clone) {
                    error!("重启的PD验证监控线程出错: {}", e);
                }
            });
        }
    }
}
