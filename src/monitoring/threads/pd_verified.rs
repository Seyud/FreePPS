use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use anyhow::Result;
use log::debug;
use log::{error, info};

#[cfg(unix)]
use crate::common::constants::{FREE_FILE, IN_CLOSE_WRITE, IN_MODIFY, PD_VERIFIED_PATH};
use crate::common::utils;
#[cfg(unix)]
use crate::monitoring::FileMonitor;
use crate::pd::PdVerifier;

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
    use std::sync::atomic::Ordering;

    // 每线程独立创建 inotify（监控 free 文件），与 uevent 共用同一 epoll：
    // free=0 时也无限阻塞在 epoll_wait，由 free 文件 inotify 事件唤醒，实现零周期唤醒
    let file_monitor = FileMonitor::new()?;
    file_monitor.add_watch(FREE_FILE, IN_MODIFY | IN_CLOSE_WRITE)?;
    file_monitor.add_inotify_to_epoll()?;

    let uevent_sock = FileMonitor::create_uevent_monitor()?;
    if let Err(e) = file_monitor.add_fd_to_epoll(
        uevent_sock,
        (libc::EPOLLIN | libc::EPOLLPRI) as u32,
        uevent_sock as u64,
    ) {
        unsafe {
            libc::close(uevent_sock);
        }
        return Err(e);
    }

    info!(
        "[{}] 开始通过uevent监控qcom状态: {}",
        utils::get_current_thread_name(),
        PD_VERIFIED_PATH
    );

    // free 暂停状态用本线程本地变量维护：仅初始化时读取共享原子，
    // 之后由 inotify 唤醒时直接读 free 文件作为权威状态。
    // （free_file.rs 更新共享原子前有约100ms延迟，若依赖原子可能漏掉 0↔1 切换）
    let mut enabled = free_enabled.load(Ordering::Relaxed);

    let mut eintr_count: u64 = 0;
    let mut eagain_count: u64 = 0;
    let mut charging_session_active = false;
    let mut last_interrupt_report = std::time::Instant::now();
    let interrupt_report_interval = std::time::Duration::from_secs(60 * 60 * 10);

    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 10];

    while running.load(Ordering::Relaxed) {
        let nfds = match file_monitor.wait_events(&mut events, -1) {
            Ok(nfds) => nfds,
            Err(err) => {
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
            }
        };

        if nfds <= 0 {
            continue;
        }

        // 优先处理 free 文件 inotify 事件，刷新 enabled 后再处理 uevent，
        // 保证同一批事件中 free=0 时 uevent 不会被误处理
        if events
            .iter()
            .take(nfds as usize)
            .any(|ev| ev.u64 == file_monitor.inotify_fd as u64)
        {
            let mut inotify_buffer = [0u8; 1024];
            let bytes_read = unsafe {
                libc::read(
                    file_monitor.inotify_fd,
                    inotify_buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                    inotify_buffer.len(),
                )
            };

            if bytes_read > 0 {
                let bytes_read = bytes_read as usize;
                let event_size = std::mem::size_of::<libc::inotify_event>();
                let mut offset = 0usize;
                let mut close_write_seen = false;
                while offset + event_size <= bytes_read {
                    let event_ptr = unsafe {
                        inotify_buffer.as_ptr().add(offset) as *const libc::inotify_event
                    };
                    let event = unsafe { &*event_ptr };
                    if (event.mask & libc::IN_CLOSE_WRITE) != 0 {
                        close_write_seen = true;
                    }
                    offset += event_size + event.len as usize;
                }

                if close_write_seen {
                    // 直接读 free 文件内容作为权威状态
                    let new_enabled = FileMonitor::read_file_content(FREE_FILE)? == "1";
                    if new_enabled != enabled {
                        if new_enabled {
                            info!("[qcom] free文件恢复为1，重新启动PD验证节点监控");
                        } else {
                            info!("[qcom] free文件为0，暂停PD验证节点监控");
                        }
                        enabled = new_enabled;
                    }
                }
            }
        }

        // 消费 uevent（free=0 时也读取，避免 socket 未读导致 epoll 忙等），仅 free=1 时处理
        for ev in events.iter().take(nfds as usize) {
            if ev.u64 != uevent_sock as u64 {
                continue;
            }

            let mut buffer = [0u8; 4096];
            let bytes_read = unsafe {
                libc::recv(
                    uevent_sock,
                    buffer.as_mut_ptr() as *mut std::os::raw::c_void,
                    buffer.len(),
                    libc::MSG_DONTWAIT,
                )
            };

            if bytes_read <= 0 || !enabled {
                continue;
            }

            let uevent_data = String::from_utf8_lossy(&buffer[..bytes_read as usize]);

            // 提取POWER_SUPPLY_STATUS
            let fields = uevent_data.split(['\0', '\n']);
            let status = fields
                .clone()
                .find(|field| field.starts_with("POWER_SUPPLY_STATUS="))
                .and_then(|field| field.split_once('=').map(|(_, value)| value));

            let mut should_set_node = false;

            // 充电过程中不强制写入pd_verifed：
            // - 小米原装充电头：内核通过verify_process自行管理pd_verifed
            //   （verify结束后内核自己设pd_verifed=1），反复写入会干扰MIPPS握手
            // - 公版PPS充电头：内核不碰pd_verifed，依赖启动时设置的值
            // 仅在拔出(Discharging)时设置pd_verifed=1，为下次插电准备
            if let Some("Discharging") = status {
                if charging_session_active {
                    info!(
                        "[qcom] 检测到Charging→Discharging状态跳变，设置pd_verifed=1为下次插电准备"
                    );
                    should_set_node = true;
                    charging_session_active = false;
                }
            } else if let Some("Charging") = status
                && !charging_session_active
            {
                charging_session_active = true;
                debug!("[qcom] 检测到充电会话开始");
            }

            if should_set_node {
                let pd_content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;
                if pd_content == "0" {
                    info!("[qcom] 设置pd_verifed=1");
                    pd_verifier.set_pd_verified(true)?;
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
    }

    Ok(())
}
