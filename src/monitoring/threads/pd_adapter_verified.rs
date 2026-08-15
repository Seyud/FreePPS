use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;

use anyhow::Result;
use log::{debug, error, info};

#[cfg(unix)]
use crate::common::FreePPSError;
#[cfg(unix)]
use crate::common::constants::PD_ADAPTER_VERIFIED_PATH;
use crate::common::utils;
use crate::monitoring::ChargingMode;
#[cfg(unix)]
use crate::monitoring::FileMonitor;
use crate::pd::PdAdapterVerifier;
use crate::platform::EventFd;
#[cfg(unix)]
use std::io;

pub fn spawn_pd_adapter_verified_monitor(
    running: Arc<AtomicBool>,
    pd_adapter_verifier: Arc<PdAdapterVerifier>,
    charging_mode: Arc<AtomicU8>,
    config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("mtk".to_string())
        .spawn(move || {
            if let Err(error) = worker(
                running,
                pd_adapter_verifier,
                charging_mode,
                config_event,
                stop_event,
            ) {
                error!("mtk线程出错: {}", error);
            }
        })
        .expect("创建mtk线程失败")
}

fn worker(
    running: Arc<AtomicBool>,
    pd_adapter_verifier: Arc<PdAdapterVerifier>,
    charging_mode: Arc<AtomicU8>,
    config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> Result<()> {
    info!("[{}] 启动mtk监控线程...", utils::get_current_thread_name());

    #[cfg(unix)]
    run_unix(
        running,
        pd_adapter_verifier,
        charging_mode,
        config_event,
        stop_event,
    )?;

    #[cfg(not(unix))]
    let _ = (
        running,
        pd_adapter_verifier,
        charging_mode,
        config_event,
        stop_event,
    );

    Ok(())
}

#[cfg(unix)]
fn add_epoll_fd(epoll_fd: libc::c_int, fd: libc::c_int, events: u32) -> Result<()> {
    let mut event = libc::epoll_event {
        events,
        u64: fd as u64,
    };
    if unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) } == -1 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(unix)]
fn remove_epoll_fd(epoll_fd: libc::c_int, fd: libc::c_int) -> Result<()> {
    if unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut()) } == -1 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(unix)]
fn run_unix(
    running: Arc<AtomicBool>,
    pd_adapter_verifier: Arc<PdAdapterVerifier>,
    charging_mode: Arc<AtomicU8>,
    config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> Result<()> {
    let uevent_sock = FileMonitor::create_uevent_monitor()?;
    let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if epoll_fd == -1 {
        unsafe { libc::close(uevent_sock) };
        return Err(FreePPSError::InotifyError("无法初始化epoll".to_string()).into());
    }

    let mode = ChargingMode::from_raw(charging_mode.load(Ordering::Acquire));
    let mut uevent_registered = mode != ChargingMode::Native;
    let setup = add_epoll_fd(epoll_fd, stop_event.raw_fd(), libc::EPOLLIN as u32)
        .and_then(|()| add_epoll_fd(epoll_fd, config_event.raw_fd(), libc::EPOLLIN as u32))
        .and_then(|()| {
            if uevent_registered {
                add_epoll_fd(
                    epoll_fd,
                    uevent_sock,
                    (libc::EPOLLIN | libc::EPOLLPRI) as u32,
                )
            } else {
                Ok(())
            }
        });
    if let Err(error) = setup {
        unsafe {
            libc::close(uevent_sock);
            libc::close(epoll_fd);
        }
        return Err(error);
    }

    info!("通过配置事件与uevent监控mtk状态");
    let mut charging_session_active = false;
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 8];

    while running.load(Ordering::Relaxed) {
        let nfds = unsafe {
            libc::epoll_wait(
                epoll_fd,
                events.as_mut_ptr(),
                events.len() as libc::c_int,
                -1,
            )
        };
        if nfds == -1 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(code) if code == libc::EINTR || code == libc::EAGAIN)
            {
                continue;
            }
            error!("mtk epoll_wait失败: {}", error);
            thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }

        let ready = &events[..nfds as usize];
        if ready
            .iter()
            .any(|event| event.u64 == stop_event.raw_fd() as u64)
        {
            stop_event.clear()?;
            break;
        }

        if ready
            .iter()
            .any(|event| event.u64 == config_event.raw_fd() as u64)
        {
            config_event.clear()?;
            let enabled = ChargingMode::from_raw(charging_mode.load(Ordering::Acquire))
                != ChargingMode::Native;
            if enabled != uevent_registered {
                if enabled {
                    add_epoll_fd(
                        epoll_fd,
                        uevent_sock,
                        (libc::EPOLLIN | libc::EPOLLPRI) as u32,
                    )?;
                } else {
                    remove_epoll_fd(epoll_fd, uevent_sock)?;
                    charging_session_active = false;
                }
                uevent_registered = enabled;
                info!("[mtk] uevent监控状态: {}", enabled);
            }
        }

        if !uevent_registered || !ready.iter().any(|event| event.u64 == uevent_sock as u64) {
            continue;
        }

        let mut buffer = [0u8; 4096];
        loop {
            let bytes_read = unsafe {
                libc::recv(
                    uevent_sock,
                    buffer.as_mut_ptr().cast::<libc::c_void>(),
                    buffer.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            if bytes_read <= 0 {
                break;
            }

            let data = String::from_utf8_lossy(&buffer[..bytes_read as usize]);
            let status = data
                .split(['\0', '\n'])
                .find_map(|field| field.strip_prefix("POWER_SUPPLY_STATUS="));
            let mut should_set_node = data.contains("POWER_SUPPLY");
            if let Some("Discharging") = status {
                should_set_node |= charging_session_active;
                charging_session_active = false;
            } else if let Some("Charging") = status {
                charging_session_active = true;
            }

            if should_set_node && FileMonitor::read_file_content(PD_ADAPTER_VERIFIED_PATH)? == "0" {
                debug!("[mtk] 设置pd_adapter验证节点为1");
                pd_adapter_verifier.set_pd_adapter_verified(true)?;
            }
        }
    }

    unsafe {
        libc::close(uevent_sock);
        libc::close(epoll_fd);
    }
    Ok(())
}
