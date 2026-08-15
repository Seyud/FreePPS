use crate::common::FreePPSError;
use crate::common::constants::{DISABLE_FILE, FREE_FILE, PD_VERIFIED_PATH};
#[cfg(unix)]
use crate::common::constants::{MODULE_PROP, PD_ADAPTER_VERIFIED_PATH};
use crate::monitoring::{ChargingMode, FileMonitor};
#[cfg(unix)]
use crate::pd::PdAdapterVerifier;
use crate::pd::PdVerifier;
use anyhow::Result;
use log::{info, warn};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

pub struct ModuleManager {
    last_mode: Mutex<Option<ChargingMode>>,
}

impl ModuleManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            last_mode: Mutex::new(None),
        })
    }

    pub fn initialize_module(&self) -> Result<ChargingMode> {
        info!("开始模块初始化...");

        if !Path::new(FREE_FILE).exists() {
            info!("free文件不存在，创建并设置为1");
            FileMonitor::write_file_content(FREE_FILE, "1")?;
        }

        if Path::new(DISABLE_FILE).exists() {
            info!("检测到disable文件，删除以启用模块");
            fs::remove_file(DISABLE_FILE).map_err(FreePPSError::FileOperation)?;
        }

        let mode = ChargingMode::from_files()?;
        self.apply_mode(mode)?;
        *self.last_mode.lock().unwrap() = Some(mode);
        info!("模块初始化完成，当前模式: {:?}", mode);
        Ok(mode)
    }

    #[cfg(unix)]
    fn update_module_description(&self, mode: ChargingMode) -> Result<()> {
        let prop_content = FileMonitor::read_file_content(MODULE_PROP)?;
        const PREFIXES: [&str; 4] = [
            "[✅锁定PPS支持⚡] ",
            "[⏸️PPS已暂停💤] ",
            "[⏸️小米协议优先💤] ",
            "[🔄协议自动识别💡] ",
        ];

        let updated_content = prop_content
            .lines()
            .map(|line| {
                if let Some(description) = line.strip_prefix("description=") {
                    let clean = PREFIXES
                        .iter()
                        .find_map(|prefix| description.strip_prefix(prefix))
                        .unwrap_or(description);
                    format!("description={}{}", mode.description_prefix(), clean)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        FileMonitor::write_file_content(MODULE_PROP, &updated_content)?;
        Ok(())
    }

    fn apply_mode(&self, mode: ChargingMode) -> Result<()> {
        info!("应用充电协议模式: {:?}", mode);
        #[cfg(unix)]
        self.update_module_description(mode)?;

        if Path::new(PD_VERIFIED_PATH).exists() {
            let public_pps = mode == ChargingMode::ForcePublicPps;
            match PdVerifier::new().and_then(|verifier| verifier.set_pd_verified(public_pps)) {
                Ok(()) => info!("qcom pd_verifed初始化为{}", u8::from(public_pps)),
                Err(error) => warn!("设置qcom节点失败: {}", error),
            }
        }

        #[cfg(unix)]
        if Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
            // Automatic detection has only been verified on QCOM. On MTK, keep
            // automatic mode equivalent to public PPS rather than disabling it.
            let public_pps = mode != ChargingMode::Native;
            match PdAdapterVerifier::new()
                .and_then(|verifier| verifier.set_pd_adapter_verified(public_pps))
            {
                Ok(()) => info!(
                    "mtk pd_adapter/usbpd_verifed初始化为{}",
                    u8::from(public_pps)
                ),
                Err(error) => warn!("设置mtk节点失败: {}", error),
            }
        }

        Ok(())
    }

    pub fn handle_configuration_change(&self) -> Result<ChargingMode> {
        let mode = ChargingMode::from_files()?;
        let mut last_mode = self.last_mode.lock().unwrap();
        if *last_mode != Some(mode) {
            self.apply_mode(mode)?;
            *last_mode = Some(mode);
        }
        Ok(mode)
    }

    #[cfg(unix)]
    pub fn handle_disable_file_change(&self, exists: bool) -> Result<()> {
        if exists {
            info!("检测到disable文件创建");
            FileMonitor::write_file_content(FREE_FILE, "0")?;
        } else {
            info!("检测到disable文件删除");
            FileMonitor::write_file_content(FREE_FILE, "1")?;
        }
        Ok(())
    }
}
