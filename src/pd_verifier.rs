use crate::PD_VERIFIED_PATH;
use crate::info;
use crate::monitor::file_monitor::FileMonitor;
use anyhow::Result;
use std::path::Path;

/// PD验证管理器
pub struct PdVerifier;

impl PdVerifier {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// 设置PD验证状态
    pub fn set_pd_verified(&self, enable: bool) -> Result<()> {
        let value = if enable { "1" } else { "0" };

        // 检查文件是否存在，不存在时记录警告但不报错
        if !Path::new(PD_VERIFIED_PATH).exists() {
            crate::warn!("PD验证文件不存在，跳过设置: {}", PD_VERIFIED_PATH);
            return Ok(());
        }

        // 写入值到系统文件
        FileMonitor::write_file_content(PD_VERIFIED_PATH, value)?;

        info!("已将PD验证状态写入为{}: {}", value, PD_VERIFIED_PATH);

        Ok(())
    }
}

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
        if !Path::new(crate::PD_ADAPTER_VERIFIED_PATH).exists() {
            crate::warn!(
                "PD适配器验证文件不存在，跳过设置: {}",
                crate::PD_ADAPTER_VERIFIED_PATH
            );
            return Ok(());
        }

        // 写入值到系统文件
        FileMonitor::write_file_content(crate::PD_ADAPTER_VERIFIED_PATH, value)?;

        info!(
            "已将PD适配器验证状态写入为{}: {}",
            value,
            crate::PD_ADAPTER_VERIFIED_PATH
        );

        Ok(())
    }
}
