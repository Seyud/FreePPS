use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Result;
#[cfg(unix)]
use log::{debug, warn};
use log::{error, info};

#[cfg(unix)]
use crate::common::FreePPSError;
#[cfg(unix)]
use crate::common::constants::{AUTO_FILE, PD_VERIFIED_PATH, USB_TYPE_PATH};
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
    let mut mipps_session_handled = false; // 标记当前充电会话是否已处理MIPPS逻辑，避免重复断充
    let mut last_interrupt_report = std::time::Instant::now();
    let interrupt_report_interval = std::time::Duration::from_secs(60 * 60 * 10);
    let epoll_timeout_ms: c_int = -1;
    let mut ignore_charging_until: Option<std::time::Instant> = None; // MIPPS断充后的Charging屏蔽窗口

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
                    // 自动识别协议握手模式：新的实现逻辑

                    // 如果屏蔽窗口已过期，恢复检测
                    if ignore_charging_until.is_some()
                        && std::time::Instant::now()
                            >= ignore_charging_until.expect("ignore_charging_until checked")
                    {
                        ignore_charging_until = None;
                    }
                    let charging_event_blocked = ignore_charging_until.is_some();

                    // 首先，保持pd验证节点为1（类似锁定PPS支持模式）
                    // 已处理MIPPS的会话中跳过，屏蔽窗口期间也跳过
                    if is_power_supply_event && !mipps_session_handled && !charging_event_blocked {
                        let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;
                        if pd_content == "0" {
                            debug!("[qcom] 自动模式：保持pd验证节点为1");
                            pd_verifier.set_pd_verified(true)?;
                        }
                    }

                    // 检测从Discharging到Charging的跳变
                    match status {
                        Some("Charging") if !charging_session_active && !charging_event_blocked => {
                            charging_session_active = true;
                            info!(
                                "[qcom] 自动模式：检测到Discharging→Charging状态跳变，开始等待3.27秒"
                            );

                            // 如果本次充电会话已经处理过MIPPS，跳过重复检测
                            if mipps_session_handled {
                                debug!("[qcom] 本次会话已处理MIPPS，跳过重复检测");
                                continue;
                            }

                            // 等待3.27秒
                            std::thread::sleep(std::time::Duration::from_millis(3270));

                            // 读取usb_type节点内容
                            let usb_type_content = FileMonitor::read_file_content(USB_TYPE_PATH)
                                .unwrap_or_else(|e| {
                                    warn!("[qcom] 读取usb_type节点失败: {}", e);
                                    String::new()
                                });

                            info!("[qcom] 读取到usb_type内容: {}", usb_type_content);

                            // 判断是MIPPS还是PPS
                            // MIPPS特征: [PD] PD_DRP PD_PPS（中括号在PD而不是PD_PPS）
                            // PPS特征: [PD_PPS]（中括号在PD_PPS）
                            if usb_type_content.contains("[PD]")
                                && usb_type_content.contains("PD_PPS")
                            {
                                // 判定为MIPPS，执行断充流程
                                info!("[qcom] 判定为MIPPS协议，执行断充流程");

                                // 标记当前会话已处理MIPPS，后续Charging事件直接跳过
                                mipps_session_handled = true;

                                // 设置屏蔽窗口，防止断充恢复期间的Charging事件触发重复流程
                                // 覆盖：断充(写1) + 1秒等待 + pd_verified清零 + 1秒等待 + 恢复(写0) + 缓冲
                                ignore_charging_until = Some(
                                    std::time::Instant::now()
                                        + std::time::Duration::from_millis(5000),
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
                                        info!("[qcom] 已写入input_suspend=1（断充）");
                                    }
                                } else {
                                    warn!("[qcom] input_suspend节点不存在，跳过断充操作");
                                }

                                // 2. 延迟1秒
                                std::thread::sleep(std::time::Duration::from_secs(1));

                                // 3. 将pd验证节点清零
                                if let Err(e) = pd_verifier.set_pd_verified(false) {
                                    error!("[qcom] 写入pd_verified=0失败: {}", e);
                                } else {
                                    info!("[qcom] 已写入pd_verified=0");
                                }

                                // 4. 再延迟1秒
                                std::thread::sleep(std::time::Duration::from_secs(1));

                                // 5. 检查并写入input_suspend=0
                                if input_suspend_exists {
                                    if let Err(e) = FileMonitor::write_file_content(
                                        crate::common::constants::INPUT_SUSPEND_PATH,
                                        "0",
                                    ) {
                                        error!("[qcom] 写入input_suspend=0失败: {}", e);
                                    } else {
                                        info!("[qcom] 已写入input_suspend=0（恢复充电）");
                                    }
                                }
                                charging_session_active = false;
                            } else if usb_type_content.contains("[PD_PPS]") {
                                // 判定为PPS，不做多余处理
                                info!("[qcom] 判定为PPS协议，保持pd验证节点为1");
                            } else {
                                warn!(
                                    "[qcom] usb_type内容不匹配MIPPS或PPS特征: {}",
                                    usb_type_content
                                );
                            }
                        }
                        Some("Discharging") if charging_session_active => {
                            charging_session_active = false;
                            mipps_session_handled = false; // 断开充电后允许下一次会话重新判断
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
