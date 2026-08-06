use crate::common::constants::{
    ADAPTER_SVID_PATH, APDO_MAX_PATH, PD_VERIFIED_PATH, REAL_TYPE_PATH, USB_VOLTAGE_NOW_PATH,
};
use crate::common::utils;
use crate::monitoring::FileMonitor;
use log::{debug, info, warn};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

// 广播 action / extra 名（实测有效，勿改）
const ACTION_QUICK_CHARGE_TYPE: &str = "miui.intent.action.ACTION_QUICK_CHARGE_TYPE";
const EXTRA_QUICK_CHARGE_TYPE: &str = "miui.intent.extra.quick_charge_type";
const EXTRA_POWER_MAX: &str = "miui.intent.extra.POWER_MAX";
const EXTRA_CAR_CHARGE: &str = "miui.intent.extra.CAR_CHARGE";

// 满血档判定阈值：内核 apdo_max 达到此值即视为"平台满血 90W 级"，显示满血功率数字
const FULL_POWER_APDO_MAX: u32 = 90;
// 满血档显示的功率数字（W）
const FULL_POWER_DISPLAY_W: u32 = 100;

// 弱充电头判定阈值：Vbus 电压低于此值视为弱充（<45W）不伪造
const MIN_HIGH_POWER_VOLTAGE_UV: u64 = 12_000_000;

// 会话轮询间隔：兼顾充电会话开始（含重启）的检测延迟与 CPU 占用
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(100);
// 充电期间补发间隔（防 SystemUI 重启回退）
const RESEND_INTERVAL: Duration = Duration::from_secs(30);

/// 金标动画广播伪造器
///
/// 伪造小米原装 MIPPS 头的 `ACTION_QUICK_CHARGE_TYPE` 广播，让 SystemUI 对公版 PPS 头
/// 也显示金标功率数字动画（chargeSpeed=3 / maxChargingWattage=100）。
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

    /// 发送伪造广播（门控不通过时静默跳过）
    pub fn send(&self) {
        if !self.should_forge() {
            return;
        }

        let power_max = self.power_max();
        info!(
            "[broadcast-forger] 发送伪造金标动画广播: quick_charge_type=4 POWER_MAX={}W",
            power_max
        );

        let power_max_arg = power_max.to_string();
        let status = Command::new("/system/bin/am")
            .args([
                "broadcast",
                "-a",
                ACTION_QUICK_CHARGE_TYPE,
                "--ei",
                EXTRA_QUICK_CHARGE_TYPE,
                "4",
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
                debug!("[broadcast-forger] am broadcast 完成");
            }
            Ok(st) => warn!("[broadcast-forger] am broadcast 返回异常: {}", st),
            Err(e) => warn!("[broadcast-forger] 执行 am broadcast 失败: {}", e),
        }
    }
}

/// 金标动画广播伪造会话循环（broadcast-forger 线程）
///
/// - 每次 Charging 会话开始（generation 增加）时：立即发送 + 100/300/700ms 重试
///   （金标动画在插入 ~1s 内触发，需时序对齐）
/// - 充电期间每 ~30s 补发一次（防 SystemUI 重启回退）
/// - 会话结束（Discharging）后停止补发
pub fn spawn_broadcast_forger_worker(
    running: Arc<AtomicBool>,
    session_gen: Arc<AtomicU32>,
    session_active: Arc<AtomicBool>,
    forger: Arc<BroadcastForger>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("broadcast-forger".to_string())
        .spawn(move || {
            let thread_name = utils::get_current_thread_name();
            info!("[{}] 启动金标动画广播伪造线程...", thread_name);

            let mut burst_done_for: Option<u32> = None;
            let mut last_resend = std::time::Instant::now();

            while running.load(Ordering::Relaxed) {
                // 会话未激活：等待下一次充电会话（新会话需重新执行首轮发送）
                if !session_active.load(Ordering::Relaxed) {
                    burst_done_for = None;
                    thread::sleep(SESSION_POLL_INTERVAL);
                    continue;
                }

                let generation = session_gen.load(Ordering::Relaxed);
                if burst_done_for != Some(generation) {
                    // 新会话开始：立即发送 + 100/300/700ms 重试（动画在插入~1s内触发，时序对齐）
                    burst_done_for = Some(generation);
                    forger.send();
                    thread::sleep(Duration::from_millis(100));
                    forger.send();
                    thread::sleep(Duration::from_millis(200)); // 累计300ms
                    forger.send();
                    thread::sleep(Duration::from_millis(400)); // 累计700ms
                    forger.send();
                    last_resend = std::time::Instant::now();
                    continue;
                }

                // 会话持续中：每~30s补发一次（防SystemUI重启回退）
                if last_resend.elapsed() >= RESEND_INTERVAL {
                    forger.send();
                    last_resend = std::time::Instant::now();
                }

                thread::sleep(SESSION_POLL_INTERVAL);
            }
        })
        .expect("创建broadcast-forger线程失败")
}
