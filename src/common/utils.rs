use std::thread;

/// 获取当前线程的名称
pub fn get_current_thread_name() -> String {
    thread::current()
        .name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("unnamed-thread-{:?}", thread::current().id()))
}
