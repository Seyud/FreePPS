use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;

use anyhow::Result;
use log::{error, info};

use crate::common::constants::FREE_FILE;
#[cfg(unix)]
use crate::common::constants::{IN_CLOSE_WRITE, IN_CREATE, IN_DELETE, IN_MODIFY, MODULE_BASE_PATH};
use crate::common::utils;
use crate::monitoring::{ChargingMode, FileMonitor, ModuleManager};
use crate::platform::EventFd;
#[cfg(unix)]
use std::io;

pub fn spawn_free_file_monitor(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    charging_mode: Arc<AtomicU8>,
    qcom_config_event: Arc<EventFd>,
    mtk_config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("mode-file-monitor".to_string())
        .spawn(move || {
            if let Err(error) = worker(
                running,
                module_manager,
                charging_mode,
                qcom_config_event,
                mtk_config_event,
                stop_event,
            ) {
                error!("模式文件监控线程出错: {}", error);
            }
        })
        .expect("创建模式文件监控线程失败")
}

fn worker(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    charging_mode: Arc<AtomicU8>,
    qcom_config_event: Arc<EventFd>,
    mtk_config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> Result<()> {
    info!(
        "[{}] 启动模式文件监控线程...",
        utils::get_current_thread_name()
    );
    if !Path::new(FREE_FILE).exists() {
        FileMonitor::write_file_content(FREE_FILE, "1")?;
    }

    let initial = ChargingMode::from_files()?;
    charging_mode.store(initial as u8, Ordering::Relaxed);

    #[cfg(unix)]
    run_unix(
        running,
        module_manager,
        charging_mode,
        qcom_config_event,
        mtk_config_event,
        stop_event,
    )?;

    #[cfg(not(unix))]
    let _ = (
        running,
        module_manager,
        charging_mode,
        qcom_config_event,
        mtk_config_event,
        stop_event,
    );

    Ok(())
}

#[cfg(unix)]
fn run_unix(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    charging_mode: Arc<AtomicU8>,
    qcom_config_event: Arc<EventFd>,
    mtk_config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> Result<()> {
    let file_monitor = FileMonitor::new()?;
    let free_watch = file_monitor.add_watch(FREE_FILE, IN_MODIFY | IN_CLOSE_WRITE)?;
    let directory_watch = file_monitor.add_watch(MODULE_BASE_PATH, IN_CREATE | IN_DELETE)?;
    file_monitor.add_inotify_to_epoll()?;
    file_monitor.add_fd_to_epoll(
        stop_event.raw_fd(),
        libc::EPOLLIN as u32,
        stop_event.raw_fd() as u64,
    )?;

    let mut buffer = [0u8; 2048];
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 8];
    while running.load(Ordering::Relaxed) {
        let nfds = match file_monitor.wait_events(&mut events, -1) {
            Ok(count) => count,
            Err(error) if matches!(error.raw_os_error(), Some(code) if code == libc::EINTR || code == libc::EAGAIN) =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };

        let ready = &events[..nfds as usize];
        if ready
            .iter()
            .any(|event| event.u64 == stop_event.raw_fd() as u64)
        {
            stop_event.clear()?;
            break;
        }
        if !ready
            .iter()
            .any(|event| event.u64 == file_monitor.inotify_fd as u64)
        {
            continue;
        }

        let bytes_read = unsafe {
            libc::read(
                file_monitor.inotify_fd,
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
            )
        };
        if bytes_read == -1 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(code) if code == libc::EINTR || code == libc::EAGAIN)
            {
                continue;
            }
            return Err(error.into());
        }

        let mut offset = 0usize;
        let mut changed = false;
        let event_size = std::mem::size_of::<libc::inotify_event>();
        while offset + event_size <= bytes_read as usize {
            let event = unsafe { &*(buffer.as_ptr().add(offset) as *const libc::inotify_event) };
            if event.wd == free_watch && event.mask & (IN_MODIFY | IN_CLOSE_WRITE) != 0 {
                changed = true;
            } else if event.wd == directory_watch && event.len > 0 {
                let name_start = offset + event_size;
                let name_bytes = &buffer[name_start..name_start + event.len as usize];
                let end = name_bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(name_bytes.len());
                if &name_bytes[..end] == b"auto" && event.mask & (IN_CREATE | IN_DELETE) != 0 {
                    changed = true;
                }
            }
            offset += event_size + event.len as usize;
        }

        if changed {
            let mode = module_manager.handle_configuration_change()?;
            charging_mode.store(mode as u8, Ordering::Release);
            qcom_config_event.notify()?;
            mtk_config_event.notify()?;
            info!("模式文件变化，切换为 {:?}", mode);
        }
    }

    Ok(())
}
