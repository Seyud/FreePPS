use crate::error::FreePPSError;
use anyhow::Result;
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::raw::{c_char, c_int};

/// 文件监控器
pub struct FileMonitor {
    #[cfg(unix)]
    pub inotify_fd: c_int,
    #[cfg(windows)]
    #[allow(dead_code)]
    pub dummy_fd: i32, // Windows环境下的占位符
}

impl FileMonitor {
    #[cfg(unix)]
    pub fn new() -> Result<Self> {
        // 外部函数声明（仅Unix）
        unsafe extern "C" {
            fn inotify_init() -> c_int;
        }

        let inotify_fd = unsafe { inotify_init() };
        if inotify_fd == -1 {
            return Err(FreePPSError::InotifyError("无法初始化inotify".to_string()).into());
        }

        Ok(Self { inotify_fd })
    }

    #[cfg(windows)]
    #[allow(dead_code)]
    pub fn new() -> Result<Self> {
        // Windows环境下返回一个简单的实现
        Ok(Self { dummy_fd: 0 })
    }

    /// 读取文件内容
    pub fn read_file_content(path: &str) -> Result<String> {
        if !Path::new(path).exists() {
            return Ok(String::new());
        }

        let content = fs::read_to_string(path)
            .map_err(FreePPSError::FileOperation)?
            .trim()
            .to_string();

        Ok(content)
    }

    /// 写入文件内容
    pub fn write_file_content(path: &str, content: &str) -> Result<()> {
        fs::write(path, content).map_err(FreePPSError::FileOperation)?;
        Ok(())
    }

    /// 添加文件监控
    #[cfg(unix)]
    pub fn add_watch(&self, path: &str, mask: u32) -> Result<i32> {
        use std::ffi::CString;

        // 外部函数声明（仅Unix）
        unsafe extern "C" {
            fn inotify_add_watch(fd: c_int, pathname: *const c_char, mask: u32) -> c_int;
        }

        let path_cstring = CString::new(path)
            .map_err(|e| FreePPSError::InotifyError(format!("路径转换失败: {}", e)))?;

        let wd = unsafe { inotify_add_watch(self.inotify_fd, path_cstring.as_ptr(), mask) };

        if wd == -1 {
            return Err(FreePPSError::InotifyError(format!("无法监控文件: {}", path)).into());
        }

        Ok(wd)
    }

    /// 添加文件监控（Windows版本）
    #[cfg(windows)]
    #[allow(dead_code)]
    pub fn add_watch(&self, _path: &str, _mask: u32) -> Result<i32> {
        // Windows环境下返回一个简单的实现
        Ok(0)
    }
}

impl Drop for FileMonitor {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // 外部函数声明（仅Unix）
            unsafe extern "C" {
                fn close(fd: c_int) -> c_int;
            }

            if self.inotify_fd != -1 {
                unsafe {
                    close(self.inotify_fd);
                }
            }
        }

        #[cfg(windows)]
        {
            // Windows环境下无需特殊清理
        }
    }
}
