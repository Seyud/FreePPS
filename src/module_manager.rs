#[cfg(unix)]
use crate::MODULE_PROP;
use crate::error::FreePPSError;
use crate::monitor::file_monitor::FileMonitor;
use crate::{DISABLE_FILE, FREE_FILE, PD_ADAPTER_VERIFIED_PATH, PD_VERIFIED_PATH};
use anyhow::Result;
use std::fs;
use std::path::Path;

/// 模块状态管理器
pub struct ModuleManager;

impl ModuleManager {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// 初始化模块状态
    pub fn initialize_module(&self) -> Result<()> {
        crate::info!("开始模块初始化...");

        // 确保free文件存在
        if !Path::new(FREE_FILE).exists() {
            crate::info!("free文件不存在，创建并设置为1");
            FileMonitor::write_file_content(FREE_FILE, "1")?;
        }

        // 确保disable文件不存在（模块启用状态）
        if Path::new(DISABLE_FILE).exists() {
            crate::info!("检测到disable文件，删除以启用模块");
            fs::remove_file(DISABLE_FILE).map_err(FreePPSError::FileOperation)?;
        }

        // 读取当前free文件状态并主动更新描述
        let free_content = FileMonitor::read_file_content(FREE_FILE)?;
        crate::info!("当前free文件内容: {}", free_content);

        if free_content == "1" {
            crate::info!("模块启用状态，更新描述");
            self.update_module_description(true)?;

            // 模块初始化时设置PD验证为1 - 添加错误处理
            match crate::monitor::PdVerifier::new() {
                Ok(pd_verifier) => {
                    if Path::new(PD_VERIFIED_PATH).exists() {
                        match pd_verifier.set_pd_verified(true) {
                            Ok(_) => {}
                            Err(e) => {
                                crate::warn!("模块初始化时设置PD验证状态失败: {}，跳过此步骤", e)
                            }
                        }
                    } else {
                        crate::warn!("PD验证文件不存在，跳过设置");
                    }
                }
                Err(e) => crate::warn!("模块初始化时创建PD验证器失败: {}，跳过此步骤", e),
            }

            #[cfg(unix)]
            {
                match crate::monitor::PdAdapterVerifier::new() {
                    Ok(pd_adapter_verifier) => {
                        if Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
                            if let Err(e) = pd_adapter_verifier.set_pd_adapter_verified(true) {
                                crate::warn!(
                                    "模块初始化时设置PD适配器验证状态失败: {}，跳过此步骤",
                                    e
                                );
                            }
                        } else {
                            crate::warn!("PD适配器验证文件不存在，跳过设置");
                        }
                    }
                    Err(e) => crate::warn!("模块初始化时创建PD适配器验证器失败: {}，跳过此步骤", e),
                }
            }
        } else {
            crate::info!("模块暂停状态，更新描述");
            self.update_module_description(false)?;
        }

        crate::info!("模块初始化完成");
        Ok(())
    }

    /// 更新module.prop描述
    #[cfg(unix)]
    pub fn update_module_description(&self, enabled: bool) -> Result<()> {
        let prop_content = FileMonitor::read_file_content(MODULE_PROP)?;
        let status_prefix = if enabled {
            "[✅PPS已支持⚡] "
        } else {
            "[⏸️PPS已暂停💤] "
        };

        let updated_content = prop_content
            .lines()
            .map(|line| {
                if line.starts_with("description=") {
                    // 提取原始描述文本
                    let original_description = line.strip_prefix("description=").unwrap_or("");
                    // 检查是否已经包含状态前缀，如果有则移除
                    let clean_description = if original_description.starts_with("[✅PPS已支持⚡] ")
                    {
                        original_description
                            .strip_prefix("[✅PPS已支持⚡] ")
                            .unwrap_or(original_description)
                    } else if original_description.starts_with("[⏸️PPS已暂停💤] ") {
                        original_description
                            .strip_prefix("[⏸️PPS已暂停💤] ")
                            .unwrap_or(original_description)
                    } else {
                        original_description
                    };
                    // 添加新的状态前缀
                    format!("description={}{}", status_prefix, clean_description)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        FileMonitor::write_file_content(MODULE_PROP, &updated_content)?;
        crate::info!(
            "更新module.prop描述，添加状态前缀: {}",
            status_prefix.trim()
        );
        Ok(())
    }

    /// 处理free文件变化
    #[cfg(unix)]
    pub fn handle_free_file_change(&self, content: &str) -> Result<()> {
        crate::info!("free文件内容: {}", content);

        if content == "1" {
            crate::info!("free文件为1，启用模块");
            self.update_module_description(true)?;
        } else if content == "0" {
            crate::info!("free文件为0，暂停模块");
            self.update_module_description(false)?;

            // 恢复PD验证为0（仅当系统文件存在）
            if Path::new(PD_VERIFIED_PATH).exists() {
                match crate::monitor::PdVerifier::new() {
                    Ok(pd_verifier) => match pd_verifier.set_pd_verified(false) {
                        Ok(_) => {}
                        Err(e) => crate::warn!("设置PD验证状态失败: {}，跳过此步骤", e),
                    },
                    Err(e) => crate::warn!("创建PD验证器失败: {}，跳过此步骤", e),
                }
            } else {
                crate::warn!("PD验证文件不存在，跳过恢复");
            }

            // 恢复PD适配器验证为0（仅当系统文件存在）
            if Path::new(crate::PD_ADAPTER_VERIFIED_PATH).exists() {
                match crate::monitor::PdAdapterVerifier::new() {
                    Ok(pd_adapter_verifier) => {
                        match pd_adapter_verifier.set_pd_adapter_verified(false) {
                            Ok(_) => {}
                            Err(e) => crate::warn!("设置PD适配器验证状态失败: {}，跳过此步骤", e),
                        }
                    }
                    Err(e) => crate::warn!("创建PD适配器验证器失败: {}，跳过此步骤", e),
                }
            } else {
                crate::warn!("PD适配器验证文件不存在，跳过恢复");
            }
        }
        Ok(())
    }

    /// 处理disable文件变化
    #[cfg(unix)]
    pub fn handle_disable_file_change(&self, exists: bool) -> Result<()> {
        if exists {
            crate::info!("检测到disable文件创建");
            // disable文件出现，设置free为0
            FileMonitor::write_file_content(FREE_FILE, "0")?;
            crate::info!("已处理disable文件创建事件");
        } else {
            crate::info!("检测到disable文件删除");
            // disable文件消失，设置free为1
            FileMonitor::write_file_content(FREE_FILE, "1")?;
            crate::info!("已处理disable文件删除事件");
        }
        Ok(())
    }
}
