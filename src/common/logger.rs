// 简单的日志宏，替代重量级的 log 库
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{
        let timestamp = $crate::common::utils::get_timestamp();
        println!("[{}] [INFO] {}", timestamp, format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        let timestamp = $crate::common::utils::get_timestamp();
        println!("[{}] [WARN] {}", timestamp, format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        let timestamp = $crate::common::utils::get_timestamp();
        eprintln!("[{}] [ERROR] {}", timestamp, format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        let timestamp = $crate::common::utils::get_timestamp();
        println!("[{}] [DEBUG] {}", timestamp, format!($($arg)*));
    }};
}
