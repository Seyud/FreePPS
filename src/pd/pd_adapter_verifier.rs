use anyhow::Result;

#[cfg(unix)]
use crate::common::constants::PD_ADAPTER_VERIFIED_PATH;
#[cfg(unix)]
use crate::info;
#[cfg(unix)]
use crate::monitoring::FileMonitor;
#[cfg(unix)]
use std::path::Path;

/// PD适配器验证管理器
pub struct PdAdapterVerifier;

impl PdAdapterVerifier {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// 设置PD适配器验证状态
    #[cfg(unix)]
    pub fn set_pd_adapter_verified(&self, enable: bool) -> Result<()> {
        let value = if enable { "1" } else { "0" };

        // 检查文件是否存在，不存在时记录警告但不报错
        if !Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
            crate::warn!(
                "PD适配器验证文件不存在，跳过设置: {}",
                PD_ADAPTER_VERIFIED_PATH
            );
            return Ok(());
        }

        // 写入值到系统文件
        FileMonitor::write_file_content(PD_ADAPTER_VERIFIED_PATH, value)?;

        info!(
            "已将PD适配器验证状态写入为{}: {}",
            value, PD_ADAPTER_VERIFIED_PATH
        );

        Ok(())
    }
}
