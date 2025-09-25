#[path = "constants.rs"]
mod constants;
#[path = "error.rs"]
mod error;
#[path = "logger.rs"]
mod logger;
#[path = "monitor.rs"]
mod monitor;
#[path = "utils.rs"]
mod utils;

use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use constants::*;
use monitor::FileMonitor;
use monitor::ModuleManager;
use monitor::PdVerifier;

/// 监控free文件
fn monitor_free_file(
    running: Arc<AtomicBool>,
    module_manager: ModuleManager,
    pd_verifier: PdVerifier,
) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    info!("[{}] 启动free文件监控线程...", thread_name);

    // 确保free文件存在
    if !std::path::Path::new(FREE_FILE).exists() {
        FileMonitor::write_file_content(FREE_FILE, "1")?;
    }

    #[cfg(unix)]
    {
        let file_monitor = FileMonitor::new()?;
        file_monitor.add_watch(FREE_FILE, IN_MODIFY | IN_CLOSE_WRITE)?;

        // 主监控循环 - Unix版本
        let mut buffer = [0u8; 1024];
        while running.load(Ordering::Relaxed) {
            let bytes_read = unsafe {
                let count = buffer.len();
                libc::read(
                    file_monitor.inotify_fd,
                    buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                    count,
                )
            };

            if bytes_read == -1 {
                error!("读取inotify事件失败，继续监控...");
                thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            } else if bytes_read > 0 {
                info!("检测到free文件变化");

                let content = FileMonitor::read_file_content(FREE_FILE)?;
                module_manager.handle_free_file_change(&content)?;

                if content == "1" {
                    info!("free文件为1，启动PD验证监控");

                    // 立即设置PD验证状态为1
                    if std::path::Path::new(PD_VERIFIED_PATH).exists() {
                        if let Err(e) = pd_verifier.set_pd_verified(true) {
                            error!("设置PD验证状态失败: {}", e);
                        }
                    } else {
                        warn!("PD验证文件不存在，跳过设置");
                    }
                }
            }
        }
    }

    #[cfg(windows)]
    {
        // Windows版本 - 使用轮询方式检查文件变化
        let mut last_modified = std::fs::metadata(FREE_FILE)
            .and_then(|m| m.modified())
            .unwrap_or_else(|_| std::time::SystemTime::now());

        while running.load(Ordering::Relaxed) {
            thread::sleep(std::time::Duration::from_millis(1000));

            let current_modified = std::fs::metadata(FREE_FILE)
                .and_then(|m| m.modified())
                .unwrap_or_else(|_| std::time::SystemTime::now());

            if current_modified > last_modified {
                info!("检测到free文件变化");
                last_modified = current_modified;

                let content = FileMonitor::read_file_content(FREE_FILE)?;
                module_manager.handle_free_file_change(&content)?;

                if content == "1" {
                    info!("free文件为1，启动PD验证监控");

                    // 立即设置PD验证状态为1
                    if std::path::Path::new(PD_VERIFIED_PATH).exists() {
                        if let Err(e) = pd_verifier.set_pd_verified(true) {
                            error!("设置PD验证状态失败: {}", e);
                        }
                    } else {
                        warn!("PD验证文件不存在，跳过设置");
                    }
                }
            }
        }
    }

    Ok(())
}

/// 监控disable文件
fn monitor_disable_file(running: Arc<AtomicBool>, module_manager: ModuleManager) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    info!("[{}] 启动disable文件监控线程...", thread_name);

    let mut disable_exists = std::path::Path::new(DISABLE_FILE).exists();

    #[cfg(unix)]
    {
        let file_monitor = FileMonitor::new()?;
        file_monitor.add_watch(MODULE_BASE_PATH, IN_CREATE | IN_DELETE)?;

        // 主监控循环 - Unix版本
        let mut buffer = [0u8; 1024];
        while running.load(Ordering::Relaxed) {
            let bytes_read = unsafe {
                let count = buffer.len();
                libc::read(
                    file_monitor.inotify_fd,
                    buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                    count,
                )
            };

            if bytes_read == -1 {
                error!("读取inotify事件失败，继续监控...");
                thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            } else if bytes_read > 0 {
                info!("检测到目录变化事件");

                let current_exists = std::path::Path::new(DISABLE_FILE).exists();

                if current_exists != disable_exists {
                    module_manager.handle_disable_file_change(current_exists)?;
                    disable_exists = current_exists;
                }
            }
        }
    }

    #[cfg(windows)]
    {
        // Windows版本 - 使用轮询方式检查文件变化
        while running.load(Ordering::Relaxed) {
            thread::sleep(std::time::Duration::from_millis(1000));

            let current_exists = std::path::Path::new(DISABLE_FILE).exists();

            if current_exists != disable_exists {
                info!("检测到disable文件变化");
                module_manager.handle_disable_file_change(current_exists)?;
                disable_exists = current_exists;
            }
        }
    }

    Ok(())
}

/// 监控PD验证状态
fn monitor_pd_verified(running: Arc<AtomicBool>, pd_verifier: PdVerifier) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    info!("[{}] 启动PD验证状态监控线程...", thread_name);

    #[cfg(unix)]
    {
        let file_monitor = FileMonitor::new()?;
        file_monitor.add_watch(PD_VERIFIED_PATH, IN_MODIFY | IN_CLOSE_WRITE)?;

        info!("[{}] 开始监控PD验证状态: {}", thread_name, PD_VERIFIED_PATH);

        // 主监控循环 - 纯inotify事件驱动
        let mut buffer = [0u8; 1024];

        while running.load(Ordering::Relaxed) {
            let bytes_read = unsafe {
                let count = buffer.len();
                libc::read(
                    file_monitor.inotify_fd,
                    buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                    count,
                )
            };

            if bytes_read == -1 {
                // 没有事件时等待1秒
                thread::sleep(std::time::Duration::from_millis(1000));
            } else if bytes_read > 0 {
                info!("检测到PD验证状态变化");

                // 检查是否应该处理（free文件为1）
                let free_content =
                    FileMonitor::read_file_content(FREE_FILE).unwrap_or_else(|_| "0".to_string());

                if free_content == "1" {
                    // 直接检查PD验证文件内容，如果被改成0就立即写入1
                    let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;
                    if pd_content == "0" {
                        info!("检测到PD验证状态被改为0，立即重新设置为1");
                        pd_verifier.set_pd_verified(true)?;
                    } else {
                        info!("PD验证状态正常为1，无需处理");
                    }
                }
            }
        }
    }

    #[cfg(windows)]
    {
        info!("[{}] 开始监控PD验证状态: {}", thread_name, PD_VERIFIED_PATH);

        // Windows版本 - 使用轮询方式检查文件变化
        let mut last_pd_content =
            FileMonitor::read_file_content(PD_VERIFIED_PATH).unwrap_or_else(|_| "1".to_string());

        while running.load(Ordering::Relaxed) {
            thread::sleep(std::time::Duration::from_millis(1000));

            // 检查是否应该处理（free文件为1）
            let free_content =
                FileMonitor::read_file_content(FREE_FILE).unwrap_or_else(|_| "0".to_string());

            if free_content == "1" {
                // 检查PD验证文件内容
                let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;

                // 只有当内容发生变化时才处理
                if pd_content != last_pd_content {
                    info!("检测到PD验证状态变化");

                    if pd_content == "0" {
                        info!("检测到PD验证状态被改为0，立即重新设置为1");
                        pd_verifier.set_pd_verified(true)?;
                        last_pd_content = "1".to_string();
                    } else {
                        info!("PD验证状态正常为1，无需处理");
                        last_pd_content = pd_content;
                    }
                }
            }
        }
    }

    Ok(())
}

fn main() {
    let main_thread_name = utils::get_current_thread_name();
    info!("[{}] 启动FreePPS", main_thread_name);

    // 创建管理器实例
    let module_manager = ModuleManager::new().expect("创建模块管理器失败");
    let pd_verifier = PdVerifier::new().expect("创建PD验证器失败");

    // 初始化阶段：确保基础文件存在并设置初始状态
    if let Err(e) = module_manager.initialize_module() {
        error!("模块初始化失败: {}", e);
    }

    // 创建运行标志
    let running = Arc::new(AtomicBool::new(true));
    let running_clone1 = Arc::clone(&running);
    let running_clone2 = Arc::clone(&running);
    let running_clone3 = Arc::clone(&running);

    let module_manager_clone1 = ModuleManager::new().expect("创建模块管理器失败");
    let module_manager_clone2 = ModuleManager::new().expect("创建模块管理器失败");
    let pd_verifier_clone = PdVerifier::new().expect("创建PD验证器失败");

    // 创建free文件监控线程
    let mut free_thread = thread::Builder::new()
        .name("free-file-monitor".to_string())
        .spawn(move || {
            if let Err(e) =
                monitor_free_file(running_clone1, module_manager_clone1, pd_verifier_clone)
            {
                error!("free文件监控线程出错: {}", e);
            }
        })
        .expect("创建free文件监控线程失败");

    // 创建disable文件监控线程
    let mut disable_thread = thread::Builder::new()
        .name("disable-file-monitor".to_string())
        .spawn(move || {
            if let Err(e) = monitor_disable_file(running_clone2, module_manager_clone2) {
                error!("disable文件监控线程出错: {}", e);
            }
        })
        .expect("创建disable文件监控线程失败");

    // 创建PD验证监控线程
    let mut pd_thread = thread::Builder::new()
        .name("pd-verification-monitor".to_string())
        .spawn(move || {
            if let Err(e) = monitor_pd_verified(running_clone3, pd_verifier) {
                error!("PD验证监控线程出错: {}", e);
            }
        })
        .expect("创建PD验证监控线程失败");

    // 主线程无限循环，保持程序运行
    info!(
        "[{}] 所有监控线程已启动，主线程进入守护模式...",
        main_thread_name
    );
    loop {
        thread::sleep(std::time::Duration::from_secs(60));

        // 检查线程是否仍在运行，如果线程panic则重启
        if free_thread.is_finished() {
            warn!("free文件监控线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            let module_manager = ModuleManager::new().expect("创建模块管理器失败");
            let pd_verifier = PdVerifier::new().expect("创建PD验证器失败");
            free_thread = thread::Builder::new()
                .name("free-file-monitor-restarted".to_string())
                .spawn(move || {
                    if let Err(e) = monitor_free_file(running_clone, module_manager, pd_verifier) {
                        error!("重启的free文件监控线程出错: {}", e);
                    }
                })
                .expect("重启free文件监控线程失败");
        }

        if disable_thread.is_finished() {
            warn!("disable文件监控线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            let module_manager = ModuleManager::new().expect("创建模块管理器失败");
            disable_thread = thread::Builder::new()
                .name("disable-file-monitor-restarted".to_string())
                .spawn(move || {
                    if let Err(e) = monitor_disable_file(running_clone, module_manager) {
                        error!("重启的disable文件监控线程出错: {}", e);
                    }
                })
                .expect("重启disable文件监控线程失败");
        }

        if pd_thread.is_finished() {
            warn!("PD验证监控线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            let pd_verifier = PdVerifier::new().expect("创建PD验证器失败");
            pd_thread = thread::Builder::new()
                .name("pd-verification-monitor-restarted".to_string())
                .spawn(move || {
                    if let Err(e) = monitor_pd_verified(running_clone, pd_verifier) {
                        error!("重启的PD验证监控线程出错: {}", e);
                    }
                })
                .expect("重启PD验证监控线程失败");
        }
    }
}
