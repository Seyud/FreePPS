use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Result;

#[cfg(unix)]
use crate::common::FreePPSError;
#[cfg(unix)]
use crate::common::constants::{FREE_FILE, PD_VERIFIED_PATH};
use crate::common::utils;
#[cfg(unix)]
use crate::monitoring::FileMonitor;
use crate::pd::PdVerifier;
#[cfg(unix)]
use std::io;

pub fn spawn_pd_verified_monitor(
    running: Arc<AtomicBool>,
    pd_verifier: Arc<PdVerifier>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("qcom".to_string())
        .spawn(move || {
            if let Err(e) = worker(running, pd_verifier) {
                crate::error!("qcom线程出错: {}", e);
            }
        })
        .expect("创建qcom线程失败")
}

fn worker(running: Arc<AtomicBool>, pd_verifier: Arc<PdVerifier>) -> Result<()> {
    let thread_name = utils::get_current_thread_name();
    crate::info!("[{}] 启动qcom监控线程...", thread_name);

    #[cfg(unix)]
    run_unix(running, pd_verifier)?;

    #[cfg(not(unix))]
    {
        let _ = (running, pd_verifier);
    }

    Ok(())
}

#[cfg(unix)]
fn run_unix(running: Arc<AtomicBool>, pd_verifier: Arc<PdVerifier>) -> Result<()> {
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

    crate::info!(
        "[{}] 开始通过uevent监控qcom状态: {}",
        utils::get_current_thread_name(),
        PD_VERIFIED_PATH
    );

    let mut last_status: Option<String> = None;
    let mut eintr_count: u64 = 0;
    let mut eagain_count: u64 = 0;
    let mut last_interrupt_report = std::time::Instant::now();
    let interrupt_report_interval = std::time::Duration::from_secs(60 * 60 * 10);

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        let mut events: Vec<libc::epoll_event> = vec![libc::epoll_event { events: 0, u64: 0 }; 10];

        let nfds =
            unsafe { libc::epoll_wait(epoll_fd, events.as_mut_ptr(), events.len() as c_int, -1) };

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
                        crate::debug!(
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
                    crate::error!("epoll_wait错误(code={})，5秒后重试：{}", code, err);
                    std::thread::sleep(std::time::Duration::from_millis(5000));
                }
                None => {
                    crate::error!("epoll_wait错误(未知code)，5秒后重试：{}", err);
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

                let is_pd_event = uevent_data.contains("pd_verifed");
                let is_power_supply_event = uevent_data.contains("POWER_SUPPLY");

                let fields = uevent_data.split(['\0', '\n']);
                let status = fields
                    .clone()
                    .find(|field| field.starts_with("POWER_SUPPLY_STATUS="))
                    .and_then(|field| field.split_once('=').map(|(_, value)| value));

                let is_transition = matches!(
                    (last_status.as_deref(), status),
                    (Some("Charging"), Some("Discharging"))
                );

                if let Some(current) = status {
                    last_status = Some(current.to_string());
                }

                if is_pd_event || is_transition || is_power_supply_event {
                    if is_pd_event && is_transition {
                        crate::debug!(
                            "检测到PD标记uevent，并伴随POWER_SUPPLY_STATUS从Charging跳变到Discharging"
                        );
                    } else if is_pd_event {
                        crate::debug!("检测到PD标记uevent事件");
                    } else if is_transition {
                        crate::debug!(
                            "检测到POWER_SUPPLY_STATUS从Charging跳变到Discharging的电源状态事件"
                        );
                    } else if is_power_supply_event {
                        crate::debug!("检测到电源相关uevent事件");
                    }

                    let free_content = FileMonitor::read_file_content(FREE_FILE)
                        .unwrap_or_else(|_| "0".to_string());

                    if free_content == "1" {
                        let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;

                        if pd_content == "0" {
                            crate::info!("[qcom] 检测到PD验证状态被改为0，立即重新设置为1");
                            pd_verifier.set_pd_verified(true)?;
                        } else if pd_content == "1" {
                            crate::debug!("[qcom] PD验证状态正常为1，无需处理");
                        }
                    }
                }
            }
        }
    }

    if eintr_count > 0 || eagain_count > 0 {
        crate::debug!(
            "epoll_wait暂时中断统计(线程退出前): EINTR={}次, EAGAIN={}次",
            eintr_count,
            eagain_count
        );
    }

    unsafe {
        libc::close(uevent_sock);
        libc::close(epoll_fd);
    }

    Ok(())
}
