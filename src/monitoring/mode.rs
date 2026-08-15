use anyhow::Result;
use std::path::Path;

use crate::common::constants::{AUTO_FILE, FREE_FILE};
use crate::monitoring::FileMonitor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ChargingMode {
    Native = 0,
    ForcePublicPps = 1,
    Automatic = 2,
}

impl ChargingMode {
    pub fn from_files() -> Result<Self> {
        let free = FileMonitor::read_file_content(FREE_FILE)?;
        Ok(if free != "1" {
            Self::Native
        } else if Path::new(AUTO_FILE).exists() {
            Self::Automatic
        } else {
            Self::ForcePublicPps
        })
    }

    pub fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::ForcePublicPps,
            2 => Self::Automatic,
            _ => Self::Native,
        }
    }

    pub fn description_prefix(self) -> &'static str {
        match self {
            Self::Native => "[⏸️小米协议优先💤] ",
            Self::ForcePublicPps => "[✅锁定PPS支持⚡] ",
            Self::Automatic => "[🔄协议自动识别💡] ",
        }
    }
}
