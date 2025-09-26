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
use monitor::{PdAdapterVerifier, PdVerifier};

/// 监控free文件
fn monitor_free_file(
    running: Arc<AtomicBool>,
    module_manager: ModuleManager,
    pd_verifier: PdVerifier,
    pd_adapter_verifier: PdAdapterVerifier,
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

                    // 立即设置PD适配器验证状态为1
                    if std::path::Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
                        if let Err(e) = pd_adapter_verifier.set_pd_adapter_verified(true) {
                            error!("设置PD适配器验证状态失败: {}", e);
                        }
                    } else {
                        warn!("PD适配器验证文件不存在，跳过设置");
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

                    // 立即设置PD适配器验证状态为1
                    if std::path::Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
                        if let Err(e) = pd_adapter_verifier.set_pd_adapter_verified(true) {
                            error!("设置PD适配器验证状态失败: {}", e);
                        }
                    } else {
                        warn!("PD适配器验证文件不存在，跳过设置");
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

/// qcom监控线程
fn monitor_pd_verified(running: Arc<AtomicBool>, pd_verifier: PdVerifier) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    info!("[{}] 启动qcom监控线程...", thread_name);

    #[cfg(unix)]
    {
        use crate::error::FreePPSError;
        use std::os::raw::c_int;

        // 创建uevent监控socket
        let uevent_sock = FileMonitor::create_uevent_monitor()?;

        // 创建epoll实例
        let epoll_fd = unsafe { libc::epoll_create1(0) };
        if epoll_fd == -1 {
            unsafe {
                libc::close(uevent_sock);
            }
            return Err(FreePPSError::InotifyError("无法初始化epoll".to_string()).into());
        }

        // 将uevent socket添加到epoll中
        let mut event = libc::epoll_event {
            events: (libc::EPOLLIN | libc::EPOLLPRI) as u32,
            u64: uevent_sock as u64,
        };

        let result =
            unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, uevent_sock, &mut event) };

        if result == -1 {
            unsafe {
                libc::close(uevent_sock);
                libc::close(epoll_fd);
            }
            return Err(
                FreePPSError::InotifyError("无法将uevent socket添加到epoll".to_string()).into(),
            );
        }

        info!(
            "[{}] 开始通过uevent监控qcom状态: {}",
            thread_name, PD_VERIFIED_PATH
        );

        // 主监控循环 - epoll事件驱动
        while running.load(Ordering::Relaxed) {
            let mut events: Vec<libc::epoll_event> =
                vec![libc::epoll_event { events: 0, u64: 0 }; 10];

            let nfds = unsafe {
                libc::epoll_wait(
                    epoll_fd,
                    events.as_mut_ptr(),
                    events.len() as c_int,
                    -1, // 阻塞等待
                )
            };

            if nfds == -1 {
                error!("epoll_wait错误，继续监控...");
                // 发生错误时休眠一段时间再重试，避免忙等待
                std::thread::sleep(std::time::Duration::from_millis(5000));
                continue;
            } else if nfds > 0 {
                // 检查uevent事件
                let mut buffer = [0u8; 4096];
                let bytes_read = unsafe {
                    libc::recv(
                        uevent_sock,
                        buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                        buffer.len(),
                        libc::MSG_DONTWAIT,
                    )
                };

                if bytes_read > 0 {
                    // 将接收到的数据转换为字符串
                    let uevent_data = String::from_utf8_lossy(&buffer[..bytes_read as usize]);

                    // 检查是否与PD验证相关
                    if uevent_data.contains("pd_verifed") || uevent_data.contains("POWER_SUPPLY") {
                        info!("检测到电源相关uevent事件");

                        // 检查是否应该处理（free文件为1）
                        let free_content = FileMonitor::read_file_content(FREE_FILE)
                            .unwrap_or_else(|_| "0".to_string());

                        if free_content == "1" {
                            // 读取PD验证文件内容
                            let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;

                            if pd_content == "0" {
                                info!("检测到PD验证状态被改为0，立即重新设置为1");
                                pd_verifier.set_pd_verified(true)?;
                            } else if pd_content == "1" {
                                info!("PD验证状态正常为1，无需处理");
                            }
                        }
                    }
                }
            }
        }

        // 清理资源
        unsafe {
            libc::close(uevent_sock);
            libc::close(epoll_fd);
        }
    }

    #[cfg(windows)]
    {
        info!("[{}] 开始监控qcom状态: {}", thread_name, PD_VERIFIED_PATH);

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

/// mtk监控线程
fn monitor_pd_adapter_verified(
    running: Arc<AtomicBool>,
    pd_adapter_verifier: PdAdapterVerifier,
) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    info!("[{}] 启动mtk监控线程...", thread_name);

    #[cfg(unix)]
    {
        use crate::error::FreePPSError;
        use std::os::raw::c_int;

        // 创建uevent监控socket
        let uevent_sock = FileMonitor::create_uevent_monitor()?;

        // 创建epoll实例
        let epoll_fd = unsafe { libc::epoll_create1(0) };
        if epoll_fd == -1 {
            unsafe {
                libc::close(uevent_sock);
            }
            return Err(FreePPSError::InotifyError("无法初始化epoll".to_string()).into());
        }

        // 将uevent socket添加到epoll中
        let mut event = libc::epoll_event {
            events: (libc::EPOLLIN | libc::EPOLLPRI) as u32,
            u64: uevent_sock as u64,
        };

        let result =
            unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, uevent_sock, &mut event) };

        if result == -1 {
            unsafe {
                libc::close(uevent_sock);
                libc::close(epoll_fd);
            }
            return Err(
                FreePPSError::InotifyError("无法将uevent socket添加到epoll".to_string()).into(),
            );
        }

        info!(
            "[{}] 开始通过uevent监控mtk状态: {}",
            thread_name, PD_ADAPTER_VERIFIED_PATH
        );

        // 主监控循环 - epoll事件驱动
        while running.load(Ordering::Relaxed) {
            let mut events: Vec<libc::epoll_event> =
                vec![libc::epoll_event { events: 0, u64: 0 }; 10];

            let nfds = unsafe {
                libc::epoll_wait(
                    epoll_fd,
                    events.as_mut_ptr(),
                    events.len() as c_int,
                    -1, // 阻塞等待
                )
            };

            if nfds == -1 {
                error!("epoll_wait错误，继续监控...");
                // 发生错误时休眠一段时间再重试，避免忙等待
                std::thread::sleep(std::time::Duration::from_millis(5000));
                continue;
            } else if nfds > 0 {
                // 检查uevent事件
                let mut buffer = [0u8; 4096];
                let bytes_read = unsafe {
                    libc::recv(
                        uevent_sock,
                        buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                        buffer.len(),
                        libc::MSG_DONTWAIT,
                    )
                };

                if bytes_read > 0 {
                    // 将接收到的数据转换为字符串
                    let uevent_data = String::from_utf8_lossy(&buffer[..bytes_read as usize]);

                    // 检查是否与PD适配器验证相关
                    if uevent_data.contains("usbpd_verifed")
                        || uevent_data.contains("Charging_Adapter")
                    {
                        info!("检测到充电适配器相关uevent事件");

                        // 检查是否应该处理（free文件为1）
                        let free_content = FileMonitor::read_file_content(FREE_FILE)
                            .unwrap_or_else(|_| "0".to_string());

                        if free_content == "1" {
                            // 读取PD适配器验证文件内容
                            let pd_adapter_content =
                                FileMonitor::read_file_content(PD_ADAPTER_VERIFIED_PATH)?;

                            if pd_adapter_content == "0" {
                                info!("检测到PD适配器验证状态被改为0，立即重新设置为1");
                                pd_adapter_verifier.set_pd_adapter_verified(true)?;
                            } else if pd_adapter_content == "1" {
                                info!("PD适配器验证状态正常为1，无需处理");
                            }
                        }
                    }
                }
            }
        }

        // 清理资源
        unsafe {
            libc::close(uevent_sock);
            libc::close(epoll_fd);
        }
    }

    #[cfg(windows)]
    {
        info!(
            "[{}] 开始监控mtk状态: {}",
            thread_name, PD_ADAPTER_VERIFIED_PATH
        );

        // Windows版本 - 使用轮询方式检查文件变化
        let mut last_pd_adapter_content = FileMonitor::read_file_content(PD_ADAPTER_VERIFIED_PATH)
            .unwrap_or_else(|_| "1".to_string());

        while running.load(Ordering::Relaxed) {
            thread::sleep(std::time::Duration::from_millis(1000));

            // 检查是否应该处理（free文件为1）
            let free_content =
                FileMonitor::read_file_content(FREE_FILE).unwrap_or_else(|_| "0".to_string());

            if free_content == "1" {
                // 检查PD适配器验证文件内容
                let pd_adapter_content = FileMonitor::read_file_content(PD_ADAPTER_VERIFIED_PATH)?;

                // 只有当内容发生变化时才处理
                if pd_adapter_content != last_pd_adapter_content {
                    info!("检测到PD适配器验证状态变化");

                    if pd_adapter_content == "0" {
                        info!("检测到PD适配器验证状态被改为0，立即重新设置为1");
                        pd_adapter_verifier.set_pd_adapter_verified(true)?;
                        last_pd_adapter_content = "1".to_string();
                    } else {
                        info!("PD适配器验证状态正常为1，无需处理");
                        last_pd_adapter_content = pd_adapter_content;
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
    let pd_adapter_verifier = PdAdapterVerifier::new().expect("创建PD适配器验证器失败");

    // 初始化阶段：确保基础文件存在并设置初始状态
    if let Err(e) = module_manager.initialize_module() {
        error!("模块初始化失败: {}", e);
    }

    // 创建运行标志
    let running = Arc::new(AtomicBool::new(true));
    let running_clone1 = Arc::clone(&running);
    let running_clone2 = Arc::clone(&running);
    let running_clone3 = Arc::clone(&running);
    let running_clone4 = Arc::clone(&running);

    let module_manager_clone1 = ModuleManager::new().expect("创建模块管理器失败");
    let module_manager_clone2 = ModuleManager::new().expect("创建模块管理器失败");
    let pd_verifier_clone = PdVerifier::new().expect("创建PD验证器失败");
    let pd_adapter_verifier_clone = PdAdapterVerifier::new().expect("创建PD适配器验证器失败");

    // 创建free文件监控线程
    let mut free_thread = thread::Builder::new()
        .name("free-file-monitor".to_string())
        .spawn(move || {
            if let Err(e) = monitor_free_file(
                running_clone1,
                module_manager_clone1,
                pd_verifier_clone,
                pd_adapter_verifier_clone,
            ) {
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

    // 创建qcom线程
    let mut pd_thread = thread::Builder::new()
        .name("qcom".to_string())
        .spawn(move || {
            if let Err(e) = monitor_pd_verified(running_clone3, pd_verifier) {
                error!("qcom线程出错: {}", e);
            }
        })
        .expect("创建qcom线程失败");

    // 创建mtk线程
    let mut pd_adapter_thread = thread::Builder::new()
        .name("mtk".to_string())
        .spawn(move || {
            if let Err(e) = monitor_pd_adapter_verified(running_clone4, pd_adapter_verifier) {
                error!("mtk线程出错: {}", e);
            }
        })
        .expect("创建mtk线程失败");

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
            let pd_adapter_verifier = PdAdapterVerifier::new().expect("创建PD适配器验证器失败");
            free_thread = thread::Builder::new()
                .name("free-file-monitor-restarted".to_string())
                .spawn(move || {
                    if let Err(e) = monitor_free_file(
                        running_clone,
                        module_manager,
                        pd_verifier,
                        pd_adapter_verifier,
                    ) {
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
            warn!("qcom线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            let pd_verifier = PdVerifier::new().expect("创建PD验证器失败");
            pd_thread = thread::Builder::new()
                .name("qcom".to_string())
                .spawn(move || {
                    if let Err(e) = monitor_pd_verified(running_clone, pd_verifier) {
                        error!("重启的qcom线程出错: {}", e);
                    }
                })
                .expect("重启qcom线程失败");
        }

        if pd_adapter_thread.is_finished() {
            warn!("mtk线程意外结束，正在重启...");
            let running_clone = Arc::clone(&running);
            let pd_adapter_verifier = PdAdapterVerifier::new().expect("创建PD适配器验证器失败");
            pd_adapter_thread = thread::Builder::new()
                .name("mtk".to_string())
                .spawn(move || {
                    if let Err(e) = monitor_pd_adapter_verified(running_clone, pd_adapter_verifier)
                    {
                        error!("重启的mtk线程出错: {}", e);
                    }
                })
                .expect("重启mtk线程失败");
        }
    }
}
