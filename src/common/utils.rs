use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

// 获取当前时间戳（YYYY-MM-DD HH:MM:SS.mmm 格式）
pub fn get_timestamp() -> String {
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
    if let Ok(content) = std::fs::read_to_string("/system/usr/share/zoneinfo/tzdata") {
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
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
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

/// 获取当前线程的名称
pub fn get_current_thread_name() -> String {
    thread::current()
        .name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("unnamed-thread-{:?}", thread::current().id()))
}
