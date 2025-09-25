use crate::PD_VERIFIED_PATH;
use crate::error::FreePPSError;
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

        // 检查文件是否存在
        if !Path::new(PD_VERIFIED_PATH).exists() {
            return Err(FreePPSError::PdVerificationFailed(format!(
                "系统文件不存在: {}",
                PD_VERIFIED_PATH
            ))
            .into());
        }

        // 写入值到系统文件
        FileMonitor::write_file_content(PD_VERIFIED_PATH, value)?;

        Ok(())
    }

    /// 读取当前PD验证状态
    #[allow(dead_code)]
    pub fn read_pd_verified(&self) -> Result<bool> {
        let content = FileMonitor::read_file_content(PD_VERIFIED_PATH)?;
        Ok(content == "1")
    }

    /// 维护PD验证状态（检查并重置为1）
    #[allow(dead_code)]
    pub fn maintain_pd_verified(&self) -> Result<()> {
        let current_value = self.read_pd_verified()?;

        if !current_value {
            info!("PD验证状态为0，重新设置为1");
            self.set_pd_verified(true)?;
        } else {
            info!("PD验证状态正常为1，无需处理");
        }

        Ok(())
    }
}
