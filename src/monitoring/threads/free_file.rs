use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;

use anyhow::Result;
use log::{error, info};

use crate::common::constants::FREE_FILE;
#[cfg(unix)]
use crate::common::constants::IN_CLOSE_WRITE;
#[cfg(unix)]
use crate::common::constants::IN_MODIFY;
use crate::common::utils;
use crate::monitoring::{FileMonitor, ModuleManager};
#[cfg(unix)]
use std::io;

pub fn spawn_free_file_monitor(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    free_enabled: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("free-file-monitor".to_string())
        .spawn(move || {
            if let Err(e) = worker(running, module_manager, free_enabled) {
                error!("free文件监控线程出错: {}", e);
            }
        })
        .expect("创建free文件监控线程失败")
}

fn worker(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    free_enabled: Arc<AtomicBool>,
) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    info!("[{}] 启动free文件监控线程...", thread_name);

    if !Path::new(FREE_FILE).exists() {
        FileMonitor::write_file_content(FREE_FILE, "1")?;
    }

    let initial =
        FileMonitor::read_file_content(FREE_FILE).unwrap_or_else(|_| "0".to_string()) == "1";
    free_enabled.store(initial, Ordering::Relaxed);

    #[cfg(unix)]
    {
        run_unix(running, module_manager, free_enabled)?;
    }

    #[cfg(not(unix))]
    {
        let _ = (running, module_manager, free_enabled);
    }

    Ok(())
}

#[cfg(unix)]
fn run_unix(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    free_enabled: Arc<AtomicBool>,
) -> Result<()> {
    let file_monitor = FileMonitor::new()?;

    // 先添加所有需要监控的路径
    file_monitor.add_watch(FREE_FILE, IN_MODIFY | IN_CLOSE_WRITE)?;

    file_monitor.add_inotify_to_epoll()?;

    let mut buffer = [0u8; 1024];
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 8];
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        let nfds = match file_monitor.wait_events(&mut events, -1) {
            Ok(nfds) => nfds,
            Err(err) => match err.raw_os_error() {
                Some(code) if code == libc::EINTR || code == libc::EAGAIN => continue,
                _ => {
                    error!("等待inotify事件失败，将在1秒后重试：{}", err);
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
                    error!("读取inotify事件失败({})，1秒后重试", err);
                    thread::sleep(std::time::Duration::from_millis(1000));
                    continue;
                }
            }
        } else if bytes_read > 0 {
            let bytes_read = bytes_read as usize;
            let event_size = std::mem::size_of::<libc::inotify_event>();
            let mut offset = 0usize;
            let mut should_process_free = false;

            while offset + event_size <= bytes_read {
                let event_ptr =
                    unsafe { buffer.as_ptr().add(offset) as *const libc::inotify_event };
                let event = unsafe { &*event_ptr };

                // 检查是否是free文件的修改事件
                if (event.mask & libc::IN_CLOSE_WRITE) != 0 {
                    should_process_free = true;
                }

                let name_len = event.len as usize;
                offset += event_size + name_len;
            }

            if should_process_free {
                info!("检测到free文件变化");

                // 延迟100ms以便：
                // 1. 让文件系统操作完全完成
                // 2. 让后续相关事件也进入inotify队列
                thread::sleep(std::time::Duration::from_millis(100));

                // 将 inotify_fd 临时设置为非阻塞模式
                let flags = unsafe { libc::fcntl(file_monitor.inotify_fd, libc::F_GETFL, 0) };
                if flags != -1 {
                    unsafe {
                        libc::fcntl(
                            file_monitor.inotify_fd,
                            libc::F_SETFL,
                            flags | libc::O_NONBLOCK,
                        );
                    }

                    // 排空所有待处理的事件
                    let mut temp_buffer = [0u8; 1024];
                    loop {
                        let bytes = unsafe {
                            libc::read(
                                file_monitor.inotify_fd,
                                temp_buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                                temp_buffer.len(),
                            )
                        };

                        if bytes <= 0 {
                            break;
                        }
                    }

                    // 恢复为阻塞模式
                    unsafe {
                        libc::fcntl(
                            file_monitor.inotify_fd,
                            libc::F_SETFL,
                            flags & !libc::O_NONBLOCK,
                        );
                    }
                }

                // 读取 free 文件内容
                let content = FileMonitor::read_file_content(FREE_FILE)?;
                let enabled = content == "1";
                free_enabled.store(enabled, Ordering::Relaxed);

                // 更新模块描述
                module_manager.handle_free_file_change(&content)?;
            }
        }
    }

    Ok(())
}
