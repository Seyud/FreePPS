use crate::common::FreePPSError;
#[cfg(unix)]
use crate::common::constants::MODULE_PROP;
#[cfg(unix)]
use crate::common::constants::PD_ADAPTER_VERIFIED_PATH;
use crate::common::constants::{AUTO_FILE, DISABLE_FILE, FREE_FILE, PD_VERIFIED_PATH};
use crate::monitoring::FileMonitor;
#[cfg(unix)]
use crate::pd::PdAdapterVerifier;
use crate::pd::PdVerifier;
use anyhow::Result;
use log::{info, warn};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

/// 模块状态管理器
pub struct ModuleManager {
    // 缓存最后一次处理的状态
    last_state: Mutex<String>,
}

impl ModuleManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            last_state: Mutex::new(String::new()),
        })
    }

    /// 初始化模块状态
    pub fn initialize_module(&self) -> Result<()> {
        info!("开始模块初始化...");

        // 确保free文件存在
        if !Path::new(FREE_FILE).exists() {
            info!("free文件不存在，创建并设置为1");
            FileMonitor::write_file_content(FREE_FILE, "1")?;
        }

        // 确保disable文件不存在（模块启用状态）
        if Path::new(DISABLE_FILE).exists() {
            info!("检测到disable文件，删除以启用模块");
            fs::remove_file(DISABLE_FILE).map_err(FreePPSError::FileOperation)?;
        }

        // 读取当前free文件状态并主动更新描述
        let free_content = FileMonitor::read_file_content(FREE_FILE)?;
        info!("当前free文件内容: {}", free_content);

        if free_content == "1" {
            // 检查是否为固定PPS支持模式（没有auto文件）
            let auto_exists = Path::new(AUTO_FILE).exists();
            if !auto_exists {
                info!("模块启用 - 固定PPS支持模式（无auto文件）");
                #[cfg(unix)]
                self.update_module_description(true)?;

                // 初始化时直接设置节点为1
                if Path::new(PD_VERIFIED_PATH).exists() {
                    info!("初始化：设置qcom节点为1");
                    match PdVerifier::new() {
                        Ok(pd_verifier) => match pd_verifier.set_pd_verified(true) {
                            Ok(_) => info!("qcom节点初始化成功"),
                            Err(e) => warn!("设置qcom节点失败: {}", e),
                        },
                        Err(e) => warn!("创建PD验证器失败: {}", e),
                    }
                }

                #[cfg(unix)]
                {
                    if Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
                        info!("初始化：设置mtk节点为1");
                        match PdAdapterVerifier::new() {
                            Ok(pd_adapter_verifier) => {
                                match pd_adapter_verifier.set_pd_adapter_verified(true) {
                                    Ok(_) => info!("mtk节点初始化成功"),
                                    Err(e) => warn!("设置mtk节点失败: {}", e),
                                }
                            }
                            Err(e) => warn!("创建PD适配器验证器失败: {}", e),
                        }
                    }
                }
            } else {
                info!("模块启用 - 协议自动识别模式（检测到auto文件）");
                #[cfg(unix)]
                self.update_module_description(true)?;
            }
        } else {
            info!("模块已暂停（free=0）");
            #[cfg(unix)]
            self.update_module_description(false)?;
        }

        info!("模块初始化完成");
        Ok(())
    }

    /// 更新module.prop描述
    #[cfg(unix)]
    pub fn update_module_description(&self, enabled: bool) -> Result<()> {
        let prop_content = FileMonitor::read_file_content(MODULE_PROP)?;

        // 检查auto文件是否存在，确定具体状态
        let auto_exists = Path::new(AUTO_FILE).exists();

        // 三种状态:
        // 1. free=0, 无auto → 关闭PPS支持
        // 2. free=1, 无auto → 固定PPS支持
        // 3. free=1, 有auto → 开启协议自动识别
        let status_prefix = if !enabled {
            "[⏸️PPS已暂停💤] "
        } else if !auto_exists {
            "[✅固定PPS支持⚡] "
        } else {
            "[🔄协议自动识别💡] "
        };

        let updated_content = prop_content
            .lines()
            .map(|line| {
                if line.starts_with("description=") {
                    // 提取原始描述文本
                    let original_description = line.strip_prefix("description=").unwrap_or("");
                    // 检查是否已经包含状态前缀，如果有则移除
                    let clean_description = if original_description
                        .starts_with("[✅固定PPS支持⚡] ")
                    {
                        original_description
                            .strip_prefix("[✅固定PPS支持⚡] ")
                            .unwrap_or(original_description)
                    } else if original_description.starts_with("[🔄协议自动识别💡] ") {
                        original_description
                            .strip_prefix("[🔄协议自动识别💡] ")
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
        info!(
            "更新module.prop描述，添加状态前缀: {}",
            status_prefix.trim()
        );
        Ok(())
    }

    /// 处理free文件变化
    #[cfg(unix)]
    pub fn handle_free_file_change(&self, content: &str) -> Result<()> {
        // 检查当前状态
        let auto_exists = Path::new(AUTO_FILE).exists();
        let current_state = format!("{}:{}", content, auto_exists);

        // 获取上次状态并检查是否相同
        {
            let mut last_state = self.last_state.lock().unwrap();
            if *last_state == current_state {
                // 状态未变化，跳过处理
                return Ok(());
            }
            // 更新状态缓存
            *last_state = current_state;
        }

        info!("free文件内容: {}", content);

        if content == "1" {
            // 检查auto文件是否存在以确定具体模式
            if auto_exists {
                info!("free文件为1，检测到auto文件，启用协议自动识别模式");
            } else {
                info!("free文件为1，无auto文件，启用固定PPS支持模式");
            }
            self.update_module_description(true)?;
        } else if content == "0" {
            info!("free文件为0，暂停模块");
            self.update_module_description(false)?;

            // 恢复PD验证为0（仅当系统文件存在）
            if Path::new(PD_VERIFIED_PATH).exists() {
                match PdVerifier::new() {
                    Ok(pd_verifier) => match pd_verifier.set_pd_verified(false) {
                        Ok(_) => {}
                        Err(e) => warn!("设置PD验证状态失败: {}，跳过此步骤", e),
                    },
                    Err(e) => warn!("创建PD验证器失败: {}，跳过此步骤", e),
                }
            } else {
                warn!("PD验证文件不存在，跳过恢复");
            }

            // 恢复PD适配器验证为0（仅当系统文件存在）
            if Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
                match PdAdapterVerifier::new() {
                    Ok(pd_adapter_verifier) => {
                        match pd_adapter_verifier.set_pd_adapter_verified(false) {
                            Ok(_) => {}
                            Err(e) => warn!("设置PD适配器验证状态失败: {}，跳过此步骤", e),
                        }
                    }
                    Err(e) => warn!("创建PD适配器验证器失败: {}，跳过此步骤", e),
                }
            } else {
                warn!("PD适配器验证文件不存在，跳过恢复");
            }
        }
        Ok(())
    }

    /// 处理disable文件变化
    #[cfg(unix)]
    pub fn handle_disable_file_change(&self, exists: bool) -> Result<()> {
        if exists {
            info!("检测到disable文件创建");
            // disable文件出现，设置free为0
            FileMonitor::write_file_content(FREE_FILE, "0")?;
            info!("已处理disable文件创建事件");
        } else {
            info!("检测到disable文件删除");
            // disable文件消失，设置free为1
            FileMonitor::write_file_content(FREE_FILE, "1")?;
            info!("已处理disable文件删除事件");
        }
        Ok(())
    }
}
