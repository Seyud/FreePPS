use thiserror::Error;

/// FreePPS监控错误类型
#[derive(Error, Debug)]
pub enum FreePPSError {
    #[error("系统文件操作失败: {0}")]
    FileOperation(#[from] std::io::Error),
    #[cfg(unix)]
    #[error("inotify监控失败: {0}")]
    InotifyError(String),
}
