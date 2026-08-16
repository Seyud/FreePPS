use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use log::{debug, error, info, warn};

#[cfg(unix)]
use crate::common::constants::{
    BATTERY_STATUS_PATH, INPUT_SUSPEND_PATH, PD_VERIFIED_PATH, TYPEC_MODE_PATH, USB_REAL_TYPE_PATH,
};
use crate::common::utils;
use crate::monitoring::ChargingMode;
#[cfg(unix)]
use crate::monitoring::FileMonitor;
use crate::pd::{BroadcastForger, PdVerifier, spawn_broadcast_forger_worker};
use crate::platform::EventFd;

const NATIVE_NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(4);
const RECONNECT_STEP_DELAY: Duration = Duration::from_secs(1);
const DETACH_DEBOUNCE: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug)]
enum AutoPhase {
    Idle,
    WaitingNative(Instant),
    Suspended(Instant),
    PublicEnabled(Instant),
    Settled,
}

impl AutoPhase {
    fn deadline(self) -> Option<Instant> {
        match self {
            Self::WaitingNative(deadline)
            | Self::Suspended(deadline)
            | Self::PublicEnabled(deadline) => Some(deadline),
            Self::Idle | Self::Settled => None,
        }
    }
}

pub fn spawn_pd_verified_monitor(
    running: Arc<AtomicBool>,
    pd_verifier: Arc<PdVerifier>,
    charging_mode: Arc<AtomicU8>,
    config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("qcom".to_string())
        .spawn(move || {
            if let Err(error) = worker(
                running,
                pd_verifier,
                charging_mode,
                config_event,
                stop_event,
            ) {
                error!("qcom线程出错: {}", error);
            }
        })
        .expect("创建qcom线程失败")
}

fn worker(
    running: Arc<AtomicBool>,
    pd_verifier: Arc<PdVerifier>,
    charging_mode: Arc<AtomicU8>,
    config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> Result<()> {
    info!("[{}] 启动qcom监控线程...", utils::get_current_thread_name());

    #[cfg(unix)]
    run_unix(
        running,
        pd_verifier,
        charging_mode,
        config_event,
        stop_event,
    )?;

    #[cfg(not(unix))]
    let _ = (
        running,
        pd_verifier,
        charging_mode,
        config_event,
        stop_event,
    );

    Ok(())
}

#[cfg(unix)]
fn is_attached() -> Result<bool> {
    Ok(FileMonitor::read_file_content(TYPEC_MODE_PATH)? != "Nothing attached")
}

#[cfg(unix)]
fn set_input_suspended(suspended: bool) -> Result<()> {
    FileMonitor::write_file_content(INPUT_SUSPEND_PATH, if suspended { "1" } else { "0" })
}

#[cfg(unix)]
fn epoll_timeout(phase: AutoPhase, detach_deadline: Option<Instant>) -> libc::c_int {
    let deadline = match (phase.deadline(), detach_deadline) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(deadline), None) | (None, Some(deadline)) => Some(deadline),
        (None, None) => None,
    };
    let Some(deadline) = deadline else {
        return -1;
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        0
    } else {
        remaining.as_millis().clamp(1, libc::c_int::MAX as u128) as libc::c_int
    }
}

#[cfg(unix)]
fn run_unix(
    running: Arc<AtomicBool>,
    pd_verifier: Arc<PdVerifier>,
    charging_mode: Arc<AtomicU8>,
    config_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
) -> Result<()> {
    let uevent_sock = FileMonitor::create_uevent_monitor()?;
    let event_monitor = FileMonitor::new()?;

    let mut mode = ChargingMode::from_raw(charging_mode.load(Ordering::Acquire));
    let mut uevent_registered = mode != ChargingMode::Native;
    let setup = event_monitor
        .add_fd_to_epoll(
            stop_event.raw_fd(),
            libc::EPOLLIN as u32,
            stop_event.raw_fd() as u64,
        )
        .and_then(|()| {
            event_monitor.add_fd_to_epoll(
                config_event.raw_fd(),
                libc::EPOLLIN as u32,
                config_event.raw_fd() as u64,
            )
        })
        .and_then(|()| {
            if uevent_registered {
                event_monitor.add_fd_to_epoll(
                    uevent_sock,
                    (libc::EPOLLIN | libc::EPOLLPRI) as u32,
                    uevent_sock as u64,
                )
            } else {
                Ok(())
            }
        });
    if let Err(error) = setup {
        unsafe { libc::close(uevent_sock) };
        return Err(error);
    }

    info!("通过uevent与短时协商定时器监控qcom状态");
    let mut attached = is_attached()?;
    let mut detach_deadline = None;
    let mut phase = match (mode, attached) {
        (ChargingMode::Automatic, true) => {
            AutoPhase::WaitingNative(Instant::now() + NATIVE_NEGOTIATION_TIMEOUT)
        }
        _ => AutoPhase::Idle,
    };

    // Preserve upstream's SystemUI gold-label/100 W broadcast feature. The
    // worker is only activated for a charging session and exits with the daemon.
    let session_gen = Arc::new(AtomicU32::new(0));
    let session_active = Arc::new(AtomicBool::new(false));
    let broadcast_session_event = Arc::new(EventFd::new()?);
    let broadcast_stop_event = Arc::new(EventFd::new()?);
    let broadcast_handle = spawn_broadcast_forger_worker(
        Arc::clone(&running),
        Arc::clone(&session_gen),
        Arc::clone(&session_active),
        Arc::clone(&broadcast_session_event),
        Arc::clone(&broadcast_stop_event),
        Arc::new(BroadcastForger),
    );
    let mut charging_session_active = false;
    if uevent_registered
        && attached
        && FileMonitor::read_file_content(BATTERY_STATUS_PATH).unwrap_or_default() == "Charging"
    {
        start_charging_session(
            &mut charging_session_active,
            &session_gen,
            &session_active,
            &broadcast_session_event,
        );
    }

    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 8];

    while running.load(Ordering::Relaxed) {
        let nfds = match event_monitor
            .wait_events(&mut events, epoll_timeout(phase, detach_deadline))
        {
            Ok(count) => count,
            Err(error) => {
                if matches!(error.raw_os_error(), Some(code) if code == libc::EINTR || code == libc::EAGAIN)
                {
                    continue;
                }
                error!("qcom epoll_wait失败: {}", error);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

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
            if matches!(phase, AutoPhase::Suspended(_) | AutoPhase::PublicEnabled(_)) {
                set_input_suspended(false)?;
            }
            let new_mode = ChargingMode::from_raw(charging_mode.load(Ordering::Acquire));
            let should_monitor_uevents = new_mode != ChargingMode::Native;
            if should_monitor_uevents != uevent_registered {
                if should_monitor_uevents {
                    event_monitor.add_fd_to_epoll(
                        uevent_sock,
                        (libc::EPOLLIN | libc::EPOLLPRI) as u32,
                        uevent_sock as u64,
                    )?;
                } else {
                    event_monitor.remove_fd_from_epoll(uevent_sock)?;
                }
                uevent_registered = should_monitor_uevents;
                info!("[qcom] uevent监控状态: {}", uevent_registered);
            }
            mode = new_mode;
            attached = is_attached()?;
            detach_deadline = None;
            phase = match (mode, attached) {
                (ChargingMode::Automatic, true) => {
                    AutoPhase::WaitingNative(Instant::now() + NATIVE_NEGOTIATION_TIMEOUT)
                }
                _ => AutoPhase::Idle,
            };
            if uevent_registered && attached {
                start_charging_session(
                    &mut charging_session_active,
                    &session_gen,
                    &session_active,
                    &broadcast_session_event,
                );
            } else {
                stop_charging_session(
                    &mut charging_session_active,
                    &session_active,
                    &broadcast_session_event,
                );
            }
            debug!("qcom收到模式变化: {:?}", mode);
        }

        if uevent_registered && ready.iter().any(|event| event.u64 == uevent_sock as u64) {
            // Drain every queued netlink datagram so epoll cannot spin on stale events.
            let mut buffer = [0u8; 4096];
            loop {
                let read = unsafe {
                    libc::recv(
                        uevent_sock,
                        buffer.as_mut_ptr().cast::<libc::c_void>(),
                        buffer.len(),
                        libc::MSG_DONTWAIT,
                    )
                };
                if read <= 0 {
                    break;
                }
            }
        }

        let physically_attached = is_attached()?;
        if attached && !physically_attached {
            let deadline = detach_deadline.get_or_insert(Instant::now() + DETACH_DEBOUNCE);
            if Instant::now() >= *deadline {
                attached = false;
                detach_deadline = None;
                if matches!(phase, AutoPhase::Suspended(_) | AutoPhase::PublicEnabled(_)) {
                    set_input_suspended(false)?;
                }
                if mode == ChargingMode::Automatic {
                    pd_verifier.set_pd_verified(false)?;
                    info!("[自动] 已拔出，恢复小米协议优先基线");
                }
                stop_charging_session(
                    &mut charging_session_active,
                    &session_active,
                    &broadcast_session_event,
                );
                phase = AutoPhase::Idle;
            }
        } else if physically_attached {
            detach_deadline = None;
            if !attached {
                attached = true;
                if mode == ChargingMode::Automatic {
                    phase = AutoPhase::WaitingNative(Instant::now() + NATIVE_NEGOTIATION_TIMEOUT);
                    info!("[自动] 检测到连接，等待小米协议认证");
                }
                if mode != ChargingMode::Native {
                    start_charging_session(
                        &mut charging_session_active,
                        &session_gen,
                        &session_active,
                        &broadcast_session_event,
                    );
                }
            }
        }

        if mode != ChargingMode::Automatic || !attached {
            continue;
        }

        phase = match phase {
            AutoPhase::WaitingNative(deadline) => {
                if FileMonitor::read_file_content(PD_VERIFIED_PATH)? == "1" {
                    info!("[自动] 小米协议认证成功，保持原生协商");
                    AutoPhase::Settled
                } else if Instant::now() >= deadline {
                    let usb_type = FileMonitor::read_file_content(USB_REAL_TYPE_PATH)?;
                    if usb_type == "PD_PPS" {
                        info!("[自动] 未检测到小米认证，开始一次公版PPS软件重连");
                        set_input_suspended(true)?;
                        AutoPhase::Suspended(Instant::now() + RECONNECT_STEP_DELAY)
                    } else {
                        warn!("[自动] 未认证且接口类型为{}，本次不强制切换", usb_type);
                        AutoPhase::Settled
                    }
                } else {
                    AutoPhase::WaitingNative(deadline)
                }
            }
            AutoPhase::Suspended(deadline) if Instant::now() >= deadline => {
                pd_verifier.set_pd_verified(true)?;
                AutoPhase::PublicEnabled(Instant::now() + RECONNECT_STEP_DELAY)
            }
            AutoPhase::PublicEnabled(deadline) if Instant::now() >= deadline => {
                set_input_suspended(false)?;
                info!("[自动] 公版PPS软件重连完成，本次连接不再重试");
                AutoPhase::Settled
            }
            other => other,
        };
    }

    if matches!(phase, AutoPhase::Suspended(_) | AutoPhase::PublicEnabled(_)) {
        let _ = set_input_suspended(false);
    }
    unsafe {
        libc::close(uevent_sock);
    }
    if let Err(error) = broadcast_stop_event.notify() {
        error!("通知broadcast-forger线程停止失败: {}", error);
    }
    if let Err(error) = broadcast_handle.join() {
        error!("broadcast-forger线程join失败: {:?}", error);
    }
    Ok(())
}

#[cfg(unix)]
fn start_charging_session(
    charging_session_active: &mut bool,
    session_gen: &AtomicU32,
    session_active: &AtomicBool,
    session_event: &EventFd,
) {
    if !*charging_session_active {
        *charging_session_active = true;
        session_active.store(true, Ordering::Relaxed);
        session_gen.fetch_add(1, Ordering::Relaxed);
        if let Err(error) = session_event.notify() {
            warn!("通知broadcast-forger会话开始失败: {}", error);
        }
    }
}

#[cfg(unix)]
fn stop_charging_session(
    charging_session_active: &mut bool,
    session_active: &AtomicBool,
    session_event: &EventFd,
) {
    *charging_session_active = false;
    session_active.store(false, Ordering::Relaxed);
    if let Err(error) = session_event.notify() {
        warn!("通知broadcast-forger会话结束失败: {}", error);
    }
}
