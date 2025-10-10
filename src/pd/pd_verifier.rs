use crate::common::PD_VERIFIED_PATH;
use crate::info;
use crate::monitoring::FileMonitor;
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
