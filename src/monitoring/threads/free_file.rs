use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Result;

use crate::common::constants::FREE_FILE;
#[cfg(unix)]
use crate::common::constants::{
    IN_CLOSE_WRITE, IN_MODIFY, PD_ADAPTER_VERIFIED_PATH, PD_VERIFIED_PATH,
};
use crate::common::utils;
use crate::monitoring::{FileMonitor, ModuleManager};
use crate::pd::{PdAdapterVerifier, PdVerifier};

pub fn spawn_free_file_monitor(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    pd_verifier: Arc<PdVerifier>,
    pd_adapter_verifier: Arc<PdAdapterVerifier>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("free-file-monitor".to_string())
        .spawn(move || {
            if let Err(e) = worker(running, module_manager, pd_verifier, pd_adapter_verifier) {
                crate::error!("free文件监控线程出错: {}", e);
            }
        })
        .expect("创建free文件监控线程失败")
}

fn worker(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    pd_verifier: Arc<PdVerifier>,
    pd_adapter_verifier: Arc<PdAdapterVerifier>,
) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    crate::info!("[{}] 启动free文件监控线程...", thread_name);

    if !Path::new(FREE_FILE).exists() {
        FileMonitor::write_file_content(FREE_FILE, "1")?;
    }

    #[cfg(unix)]
    {
        run_unix(running, module_manager, pd_verifier, pd_adapter_verifier)?;
    }

    #[cfg(not(unix))]
    {
        let _ = (running, module_manager, pd_verifier, pd_adapter_verifier);
    }

    Ok(())
}

#[cfg(unix)]
fn run_unix(
    running: Arc<AtomicBool>,
    module_manager: Arc<ModuleManager>,
    pd_verifier: Arc<PdVerifier>,
    pd_adapter_verifier: Arc<PdAdapterVerifier>,
) -> Result<()> {
    let file_monitor = FileMonitor::new()?;
    file_monitor.add_watch(FREE_FILE, IN_MODIFY | IN_CLOSE_WRITE)?;

    let mut buffer = [0u8; 1024];
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        let bytes_read = unsafe {
            let count = buffer.len();
            libc::read(
                file_monitor.inotify_fd,
                buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                count,
            )
        };

        if bytes_read == -1 {
            crate::error!("读取inotify事件失败，继续监控...");
            thread::sleep(std::time::Duration::from_millis(1000));
            continue;
        } else if bytes_read > 0 {
            let bytes_read = bytes_read as usize;
            let event_size = std::mem::size_of::<libc::inotify_event>();
            let mut offset = 0usize;
            let mut should_process = false;

            while offset + event_size <= bytes_read {
                let event_ptr =
                    unsafe { buffer.as_ptr().add(offset) as *const libc::inotify_event };
                let event = unsafe { &*event_ptr };

                if (event.mask & libc::IN_CLOSE_WRITE) != 0 {
                    should_process = true;
                }

                let name_len = event.len as usize;
                offset += event_size + name_len;
            }

            if should_process {
                crate::info!("检测到free文件变化");

                let content = FileMonitor::read_file_content(FREE_FILE)?;
                module_manager.handle_free_file_change(&content)?;

                if content == "1" {
                    crate::info!("free文件为1，启动PD验证监控");

                    if Path::new(PD_VERIFIED_PATH).exists() {
                        if let Err(e) = pd_verifier.set_pd_verified(true) {
                            crate::error!("设置PD验证状态失败: {}", e);
                        }
                    } else {
                        crate::warn!("PD验证文件不存在，跳过设置");
                    }

                    if Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
                        if let Err(e) = pd_adapter_verifier.set_pd_adapter_verified(true) {
                            crate::error!("设置PD适配器验证状态失败: {}", e);
                        }
                    } else {
                        crate::warn!("PD适配器验证文件不存在，跳过设置");
                    }
                }
            }
        }
    }

    Ok(())
}
