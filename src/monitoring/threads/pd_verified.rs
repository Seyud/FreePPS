use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Result;
use log::{debug, error, info, warn};

#[cfg(unix)]
use crate::common::FreePPSError;
#[cfg(unix)]
use crate::common::constants::{AUTO_FILE, PD_VERIFIED_PATH};
use crate::common::utils;
#[cfg(unix)]
use crate::monitoring::FileMonitor;
use crate::pd::PdVerifier;
#[cfg(unix)]
use std::io;

pub fn spawn_pd_verified_monitor(
    running: Arc<AtomicBool>,
    pd_verifier: Arc<PdVerifier>,
    free_enabled: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("qcom".to_string())
        .spawn(move || {
            if let Err(e) = worker(running, pd_verifier, free_enabled) {
                error!("qcom线程出错: {}", e);
            }
        })
        .expect("创建qcom线程失败")
}

fn worker(
    running: Arc<AtomicBool>,
    pd_verifier: Arc<PdVerifier>,
    free_enabled: Arc<AtomicBool>,
) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    info!("[{}] 启动qcom监控线程...", thread_name);

    #[cfg(unix)]
    run_unix(running, pd_verifier, free_enabled)?;

    #[cfg(not(unix))]
    {
        let _ = (running, pd_verifier, free_enabled);
    }

    Ok(())
}

#[cfg(unix)]
fn run_unix(
    running: Arc<AtomicBool>,
    pd_verifier: Arc<PdVerifier>,
    free_enabled: Arc<AtomicBool>,
) -> Result<()> {
    use std::os::raw::c_int;

    let uevent_sock = FileMonitor::create_uevent_monitor()?;

    let epoll_fd = unsafe { libc::epoll_create1(0) };
    if epoll_fd == -1 {
        unsafe {
            libc::close(uevent_sock);
        }
        return Err(FreePPSError::InotifyError("无法初始化epoll".to_string()).into());
    }

    let mut event = libc::epoll_event {
        events: (libc::EPOLLIN | libc::EPOLLPRI) as u32,
        u64: uevent_sock as u64,
    };

    let result = unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, uevent_sock, &mut event) };
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
        utils::get_current_thread_name(),
        PD_VERIFIED_PATH
    );

    let mut eintr_count: u64 = 0;
    let mut eagain_count: u64 = 0;
    let mut last_status_log = false;
    let mut charging_session_active = false;
    let mut last_interrupt_report = std::time::Instant::now();
    let interrupt_report_interval = std::time::Duration::from_secs(60 * 60 * 10);
    let epoll_timeout_ms: c_int = -1;

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        let enabled = free_enabled.load(std::sync::atomic::Ordering::Relaxed);

        if !enabled {
            if !last_status_log {
                info!("[qcom] free文件为0，暂停PD验证节点监控");
                last_status_log = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            continue;
        }

        if last_status_log {
            info!("[qcom] free文件恢复为1，重新启动PD验证节点监控");
            last_status_log = false;
        }

        let mut events: Vec<libc::epoll_event> = vec![libc::epoll_event { events: 0, u64: 0 }; 10];

        let nfds = unsafe {
            libc::epoll_wait(
                epoll_fd,
                events.as_mut_ptr(),
                events.len() as c_int,
                epoll_timeout_ms,
            )
        };

        if nfds == -1 {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(code) if code == libc::EINTR || code == libc::EAGAIN => {
                    if code == libc::EINTR {
                        eintr_count += 1;
                    } else {
                        eagain_count += 1;
                    }

                    let now = std::time::Instant::now();
                    if now.duration_since(last_interrupt_report) >= interrupt_report_interval
                        && (eintr_count > 0 || eagain_count > 0)
                    {
                        debug!(
                            "epoll_wait暂时中断统计(最近{}秒): EINTR={}次, EAGAIN={}次",
                            interrupt_report_interval.as_secs(),
                            eintr_count,
                            eagain_count
                        );
                        eintr_count = 0;
                        eagain_count = 0;
                        last_interrupt_report = now;
                    }
                }
                Some(code) => {
                    error!("epoll_wait错误(code={})，5秒后重试：{}", code, err);
                    std::thread::sleep(std::time::Duration::from_millis(5000));
                }
                None => {
                    error!("epoll_wait错误(未知code)，5秒后重试：{}", err);
                    std::thread::sleep(std::time::Duration::from_millis(5000));
                }
            }
            continue;
        } else if nfds > 0 {
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
                let uevent_data = String::from_utf8_lossy(&buffer[..bytes_read as usize]);

                // 检查是否为POWER_SUPPLY事件
                let is_power_supply_event = uevent_data.contains("POWER_SUPPLY");

                // 提取POWER_SUPPLY_STATUS
                let fields = uevent_data.split(['\0', '\n']);
                let status = fields
                    .clone()
                    .find(|field| field.starts_with("POWER_SUPPLY_STATUS="))
                    .and_then(|field| field.split_once('=').map(|(_, value)| value));

                // 检查是否为锁定PPS支持模式
                let auto_exists = std::path::Path::new(AUTO_FILE).exists();

                if !auto_exists {
                    // 锁定PPS支持模式
                    let mut should_set_node = false;

                    // 条件1: 检测到任何POWER_SUPPLY事件
                    if is_power_supply_event {
                        debug!("[qcom] 锁定PPS模式：检测到POWER_SUPPLY事件");
                        should_set_node = true;
                    }

                    // 条件2: 检测到从Charging到Discharging的状态跳变
                    if let Some("Discharging") = status {
                        if charging_session_active {
                            info!("[qcom] 锁定PPS模式：检测到Charging→Discharging状态跳变");
                            should_set_node = true;
                            charging_session_active = false;
                        }
                    } else if let Some("Charging") = status
                        && !charging_session_active
                    {
                        charging_session_active = true;
                    }

                    // 执行节点设置
                    if should_set_node {
                        let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;
                        if pd_content == "0" {
                            info!("[qcom] 锁定PPS模式：设置节点为1");
                            pd_verifier.set_pd_verified(true)?;
                        }
                    }
                } else {
                    // 自动识别协议握手模式：保持原有逻辑
                    match status {
                        Some("Charging") if !charging_session_active => {
                            charging_session_active = true;
                            debug!("检测到POWER_SUPPLY_STATUS=Charging事件，开始监测PD验证节点");

                            let start = std::time::Instant::now();
                            let timeout = std::time::Duration::from_millis(3270);
                            let interval = std::time::Duration::from_millis(100);
                            let mut detected_external_handshake = false;

                            while start.elapsed() < timeout {
                                let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;

                                if pd_content == "1" {
                                    detected_external_handshake = true;
                                    break;
                                }

                                std::thread::sleep(interval);
                            }

                            if detected_external_handshake {
                                info!(
                                    "[qcom] {}秒内检测到节点已被置为1，判定为MIPPS握手",
                                    timeout.as_secs()
                                );
                            } else {
                                let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;

                                if pd_content == "0" {
                                    info!(
                                        "[qcom] {}秒后节点仍为0，判定为PPS握手，执行断电握手流程",
                                        timeout.as_secs()
                                    );

                                    // 1. 检查并写入input_suspend=1
                                    let input_suspend_exists = std::path::Path::new(
                                        crate::common::constants::INPUT_SUSPEND_PATH,
                                    )
                                    .exists();
                                    if input_suspend_exists {
                                        if let Err(e) = FileMonitor::write_file_content(
                                            crate::common::constants::INPUT_SUSPEND_PATH,
                                            "1",
                                        ) {
                                            error!("[qcom] 写入input_suspend=1失败: {}", e);
                                        } else {
                                            info!("[qcom] 已写入input_suspend=1");
                                        }
                                        // 延迟1秒
                                        std::thread::sleep(std::time::Duration::from_secs(1));
                                    } else {
                                        warn!("[qcom] input_suspend节点不存在，跳过断电操作");
                                    }

                                    // 2. 设置pd_verified=1
                                    pd_verifier.set_pd_verified(true)?;

                                    // 3. 检查并写入input_suspend=0
                                    if input_suspend_exists {
                                        // 延迟1秒
                                        std::thread::sleep(std::time::Duration::from_secs(1));

                                        if let Err(e) = FileMonitor::write_file_content(
                                            crate::common::constants::INPUT_SUSPEND_PATH,
                                            "0",
                                        ) {
                                            error!("[qcom] 写入input_suspend=0失败: {}", e);
                                        } else {
                                            info!("[qcom] 已写入input_suspend=0");
                                        }
                                    }
                                } else {
                                    debug!("[qcom] {}秒后节点已为1，无需处理", timeout.as_secs());
                                }
                            }
                        }
                        Some("Discharging") if charging_session_active => {
                            charging_session_active = false;
                            debug!("[qcom] 检测到Discharging事件");
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if eintr_count > 0 || eagain_count > 0 {
        debug!(
            "epoll_wait暂时中断统计(线程退出前): EINTR={}次, EAGAIN={}次",
            eintr_count, eagain_count
        );
    }

    unsafe {
        libc::close(uevent_sock);
        libc::close(epoll_fd);
    }

    Ok(())
}
