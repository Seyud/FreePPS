use crate::common::constants::{
    ADAPTER_SVID_PATH, APDO_MAX_PATH, PD_VERIFIED_PATH, REAL_TYPE_PATH, USB_VOLTAGE_NOW_PATH,
};
use crate::common::utils;
use crate::monitoring::FileMonitor;
use crate::platform::EventFd;
use log::{debug, info, warn};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

// 伪造广播只投递给 SystemUI，避免 PowerCenter（com.miui.securitycenter）监听
// ACTION_QUICK_CHARGE_TYPE 后弹出"退出快充加速"通知
const BROADCAST_TARGET_PKG: &str = "com.android.systemui";

// 广播 action / extra 名（实测有效，勿改）
const ACTION_QUICK_CHARGE_TYPE: &str = "miui.intent.action.ACTION_QUICK_CHARGE_TYPE";
const EXTRA_QUICK_CHARGE_TYPE: &str = "miui.intent.extra.quick_charge_type";
const EXTRA_POWER_MAX: &str = "miui.intent.extra.POWER_MAX";
const EXTRA_CAR_CHARGE: &str = "miui.intent.extra.CAR_CHARGE";
// 亮屏超级岛数字显示门控广播（SystemUI 需先收到它才显示 "xxW" 数字，见 send_soc_decimal）
const ACTION_SOC_DECIMAL: &str = "miui.intent.action.ACTION_SOC_DECIMAL";
const EXTRA_SOC_DECIMAL: &str = "miui.intent.extra.soc_decimal";
const EXTRA_SOC_DECIMAL_RATE: &str = "miui.intent.extra.soc_decimal_rate";

// SOC 小数伪造值：仅用于解锁超级岛数字显示路径，数值不参与真实充电计算
// （0/0 表示小数部分为 0，SystemUI 会显示如 "100W 62.00%"）
const SOC_DECIMAL: u32 = 0;
const SOC_DECIMAL_RATE: u32 = 0;

// 满血档判定阈值：内核 apdo_max 达到此值即视为"平台满血 90W 级"，显示满血功率数字
const FULL_POWER_APDO_MAX: u32 = 90;
// 满血档显示的功率数字（W）
const FULL_POWER_DISPLAY_W: u32 = 100;

// 弱充电头判定阈值：Vbus 电压低于此值视为弱充（<45W）不伪造
const MIN_HIGH_POWER_VOLTAGE_UV: u64 = 12_000_000;

/// 金标动画广播伪造器
///
/// 伪造小米原装 MIPPS 头的 `ACTION_QUICK_CHARGE_TYPE` 广播，让 SystemUI 对公版 PPS 头
/// 也显示金标功率数字动画（chargeSpeed=3 / maxChargingWattage=100）。
/// 同时伪造 `ACTION_SOC_DECIMAL`：亮屏超级岛（DynamicIsland）的 "xxW" 数字显示额外依赖
/// 该广播（SystemUI 的 receivedDecimal 门控），锁屏金标动画则只依赖前者。
/// 仅当 FreePPS 已解锁高功率（pd_verifed=1）且为公版 PPS 头（adapter_svid=0000）时伪造，
/// 小米原装头走原生 MIPPS 路径，不受影响。
pub struct BroadcastForger;

impl BroadcastForger {
    /// 门控：仅在以下条件全部满足时伪造（防误报）
    ///
    /// - real_type == PD_PPS：PPS 协议充电中
    /// - pd_verifed == 1：FreePPS 已解锁高功率档
    /// - adapter_svid == 0000：公版 PPS 头（非小米原装 MIPPS 头）
    /// - Vbus 电压足够高（排除弱充电头）
    fn should_forge(&self) -> bool {
        let real_type = FileMonitor::read_file_content(REAL_TYPE_PATH).unwrap_or_default();
        let pd_verifed = FileMonitor::read_file_content(PD_VERIFIED_PATH).unwrap_or_default();
        let adapter_svid = FileMonitor::read_file_content(ADAPTER_SVID_PATH).unwrap_or_default();
        let voltage_uv: u64 = FileMonitor::read_file_content(USB_VOLTAGE_NOW_PATH)
            .unwrap_or_default()
            .parse()
            .unwrap_or(0);

        real_type == "PD_PPS"
            && pd_verifed == "1"
            && adapter_svid == "0000"
            && voltage_uv >= MIN_HIGH_POWER_VOLTAGE_UV
    }

    /// 计算广播的 POWER_MAX：按充电头 PPS 能力 apdo_max 分级显示。
    ///
    /// - apdo_max >= 90（平台满血 90W 级）：显示满血功率数字 FULL_POWER_DISPLAY_W
    /// - apdo_max < 90（如 65W 头）：显示真实 PPS 能力 apdo_max
    fn power_max(&self) -> u32 {
        let apdo_max = FileMonitor::read_file_content(APDO_MAX_PATH).unwrap_or_default();
        match apdo_max.parse::<u32>() {
            Ok(v) if v >= FULL_POWER_APDO_MAX => FULL_POWER_DISPLAY_W,
            Ok(v) => v,
            Err(_) => {
                debug!(
                    "[broadcast-forger] apdo_max解析失败({:?})，默认取{}W",
                    apdo_max, FULL_POWER_DISPLAY_W
                );
                FULL_POWER_DISPLAY_W
            }
        }
    }

    /// 补发 SOC 小数广播（门控不通过时不会走到这里）
    ///
    /// 同样只投递给 SystemUI（`-p`），避免 PowerCenter 收到后弹"退出快充加速"通知。
    ///
    /// 亮屏超级岛（`DeviceNotificationListenerImpl`）显示功率数字的前提是收到过
    /// `ACTION_SOC_DECIMAL`（receivedDecimal=true）；该广播正常由 system_server 在
    /// 内核 quick_charge_type>=3 时发送，而公版 PPS 头内核只报 1，因此需要一并伪造。
    /// SystemUI 的 receiver 要求 `oldBatteryStatus.chargeSpeed>0`，所以必须在
    /// `ACTION_QUICK_CHARGE_TYPE` 处理完成（chargeSpeed 传播到 KeyguardUpdateMonitor）后
    /// 再发送，调用方 `send()` 内已预留延迟。
    fn send_soc_decimal(&self) {
        let soc_decimal = SOC_DECIMAL.to_string();
        let soc_decimal_rate = SOC_DECIMAL_RATE.to_string();
        let status = Command::new("/system/bin/am")
            .args([
                "broadcast",
                "-p",
                BROADCAST_TARGET_PKG,
                "-a",
                ACTION_SOC_DECIMAL,
                "--ei",
                EXTRA_SOC_DECIMAL,
                soc_decimal.as_str(),
                "--ei",
                EXTRA_SOC_DECIMAL_RATE,
                soc_decimal_rate.as_str(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(st) if st.success() => {
                debug!("[broadcast-forger] am broadcast ACTION_SOC_DECIMAL 完成");
            }
            Ok(st) => warn!(
                "[broadcast-forger] am broadcast ACTION_SOC_DECIMAL 返回异常: {}",
                st
            ),
            Err(e) => warn!(
                "[broadcast-forger] 执行 am broadcast ACTION_SOC_DECIMAL 失败: {}",
                e
            ),
        }
    }

    /// 发送单条快速充电广播（quick_charge_type=1 或 4）
    fn send_quick_charge(&self, quick_charge_type: &str, power_max: u32) {
        info!(
            "[broadcast-forger] 发送伪造金标动画广播: quick_charge_type={} POWER_MAX={}W",
            quick_charge_type, power_max
        );

        let power_max_arg = power_max.to_string();
        let status = Command::new("/system/bin/am")
            .args([
                "broadcast",
                "-p",
                BROADCAST_TARGET_PKG,
                "-a",
                ACTION_QUICK_CHARGE_TYPE,
                "--ei",
                EXTRA_QUICK_CHARGE_TYPE,
                quick_charge_type,
                "--ei",
                EXTRA_POWER_MAX,
                power_max_arg.as_str(),
                "--ei",
                EXTRA_CAR_CHARGE,
                "0",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(st) if st.success() => {
                debug!(
                    "[broadcast-forger] am broadcast(quick_charge_type={}) 完成",
                    quick_charge_type
                );
            }
            Ok(st) => warn!(
                "[broadcast-forger] am broadcast(quick_charge_type={}) 返回异常: {}",
                quick_charge_type, st
            ),
            Err(e) => warn!(
                "[broadcast-forger] 执行 am broadcast(quick_charge_type={}) 失败: {}",
                quick_charge_type, e
            ),
        }
    }

    /// 会话开始爆发序列：QUICK=1 → SOC_DECIMAL → QUICK=4（→ 直接显示 100W MAX）
    ///
    /// 亮屏超级岛（`DeviceNotificationListenerImpl`）显示 "100W MAX" 数字需要一次
    /// chargeSpeed=3 且 receivedDecimal=true 的刷新。若直接发 QUICK=4，SystemUI 第一次
    /// 收到时 receivedDecimal 仍为 false，会回退显示"快充中"（多余的第二次通知）。
    ///
    /// 正确时序（与 MIPPS 原生"快充中→数字"一致）：
    /// 1. QUICK=1：确保 chargeSpeed>=1（内核 quick_charge_type=1 已生效时无副作用）
    /// 2. SOC_DECIMAL：SystemUI 要求 chargeSpeed>0 才置 receivedDecimal=true
    /// 3. QUICK=4：chargeDeviceType 1→4 触发刷新，receivedDecimal=true 直接显示 100W MAX
    ///
    /// 之后补发 QUICK=4 两次，应对内核 quick_charge_type=1 广播竞争（降级后快速恢复）。
    pub fn send_burst(&self) {
        // 等待门控成立（插入后 Vbus 爬升到高功率阈值），超时 2s 放弃
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !self.should_forge() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        if !self.should_forge() {
            return;
        }

        // 1) 建立快充态（chargeSpeed>=1）
        let power_max = self.power_max();
        self.send_quick_charge("1", power_max);
        thread::sleep(Duration::from_millis(100));

        // 2) 让 SystemUI 的 receivedDecimal=true（需 chargeSpeed>0 已传播）
        if !self.should_forge() {
            return;
        }
        self.send_soc_decimal();
        thread::sleep(Duration::from_millis(100));

        // 3) 升级 chargeSpeed=3 → 超级岛直接显示 100W MAX（不再经过"快充中"回退）
        if !self.should_forge() {
            return;
        }
        self.send_quick_charge("4", power_max);

        // 4-5) 补发 QUICK=4，应对内核 quick_charge_type=1 广播在握手期降级
        thread::sleep(Duration::from_millis(250));
        if self.should_forge() {
            self.send_quick_charge("4", power_max);
        }
        thread::sleep(Duration::from_millis(400));
        if self.should_forge() {
            self.send_quick_charge("4", power_max);
        }
    }
}

/// 金标动画广播伪造会话循环（broadcast-forger 线程）
///
/// - 每次 Charging 会话开始（generation 增加）时：执行 QUICK=1 → SOC_DECIMAL → QUICK=4
///   爆发序列（金标动画在插入 ~1s 内触发，需时序对齐；同时解锁亮屏超级岛数字显示）
/// - 充电期间不再补发（避免重复触发 SystemUI/PowerCenter，实测仅爆发一次即稳定生效）
/// - 会话结束（Discharging）后停止补发
pub fn spawn_broadcast_forger_worker(
    running: Arc<AtomicBool>,
    session_gen: Arc<AtomicU32>,
    session_active: Arc<AtomicBool>,
    session_event: Arc<EventFd>,
    stop_event: Arc<EventFd>,
    forger: Arc<BroadcastForger>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("broadcast-forger".to_string())
        .spawn(move || {
            let thread_name = utils::get_current_thread_name();
            info!("[{}] 启动金标动画广播伪造线程...", thread_name);

            let mut burst_done_for: Option<u32> = None;
            let mut poll_fds = [
                libc::pollfd {
                    fd: session_event.raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop_event.raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];

            while running.load(Ordering::Relaxed) {
                let result = unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as _, -1) };
                if result == -1 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::EINTR) {
                        continue;
                    }
                    warn!("[broadcast-forger] poll失败: {}", error);
                    break;
                }

                if poll_fds[1].revents & libc::POLLIN != 0 {
                    if let Err(error) = stop_event.clear() {
                        warn!("[broadcast-forger] 清除停止事件失败: {}", error);
                    }
                    break;
                }

                if poll_fds[0].revents & libc::POLLIN == 0 {
                    continue;
                }
                if let Err(error) = session_event.clear() {
                    warn!("[broadcast-forger] 清除会话事件失败: {}", error);
                }

                if !session_active.load(Ordering::Relaxed) {
                    burst_done_for = None;
                    continue;
                }

                let generation = session_gen.load(Ordering::Relaxed);
                if burst_done_for != Some(generation) {
                    // 新会话开始：QUICK=1 → SOC_DECIMAL → QUICK=4 爆发序列
                    // （让超级岛直接显示 100W MAX，避免多余的"快充中"回退通知）
                    burst_done_for = Some(generation);
                    forger.send_burst();
                }
            }
        })
        .expect("创建broadcast-forger线程失败")
}
