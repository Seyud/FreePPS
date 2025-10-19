use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Result;

#[cfg(unix)]
use crate::common::constants::{DISABLE_FILE, IN_CREATE, IN_DELETE, MODULE_BASE_PATH};
use crate::common::utils;
#[cfg(unix)]
use crate::monitoring::FileMonitor;
use crate::monitoring::ModuleManager;
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::path::Path;

pub fn spawn_disable_file_monitor(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("disable-file-monitor".to_string())
        .spawn(move || {
            if let Err(e) = worker(running, module_manager) {
                crate::error!("disable文件监控线程出错: {}", e);
            }
        })
        .expect("创建disable文件监控线程失败")
}

fn worker(running: Arc<AtomicBool>, module_manager: Arc<ModuleManager>) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    crate::info!("[{}] 启动disable文件监控线程...", thread_name);

    #[cfg(unix)]
    {
        let mut disable_exists = Path::new(DISABLE_FILE).exists();
        run_unix(running, module_manager, &mut disable_exists)?;
    }

    #[cfg(not(unix))]
    {
        let _ = (running, module_manager);
    }

    Ok(())
}

#[cfg(unix)]
fn run_unix(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    disable_exists: &mut bool,
) -> Result<()> {
    let file_monitor = FileMonitor::new()?;
    file_monitor.add_watch(MODULE_BASE_PATH, IN_CREATE | IN_DELETE)?;

    let mut buffer = [0u8; 1024];
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 8];
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        let nfds = match file_monitor.wait_events(&mut events, -1) {
            Ok(nfds) => nfds,
            Err(err) => match err.raw_os_error() {
                Some(code) if code == libc::EINTR || code == libc::EAGAIN => continue,
                _ => {
                    crate::error!("等待inotify事件失败，将在1秒后重试：{}", err);
                    thread::sleep(std::time::Duration::from_millis(1000));
                    continue;
                }
            },
        };

        if nfds <= 0 {
            continue;
        }

        let bytes_read = unsafe {
            let count = buffer.len();
            libc::read(
                file_monitor.inotify_fd,
                buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                count,
            )
        };

        if bytes_read == -1 {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(code) if code == libc::EINTR || code == libc::EAGAIN => continue,
                _ => {
                    crate::error!("读取inotify事件失败({})，1秒后重试", err);
                    thread::sleep(std::time::Duration::from_millis(1000));
                    continue;
                }
            }
        } else if bytes_read > 0 {
            crate::info!("检测到目录变化事件");

            let current_exists = Path::new(DISABLE_FILE).exists();
            if current_exists != *disable_exists {
                module_manager.handle_disable_file_change(current_exists)?;
                *disable_exists = current_exists;
            }
        }
    }

    Ok(())
}
