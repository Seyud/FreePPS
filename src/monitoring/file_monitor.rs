use crate::common::FreePPSError;
use anyhow::Result;
use std::fs;
use std::path::Path;

#[cfg(unix)]
use libc::c_int;
#[cfg(unix)]
use std::os::raw::c_char;

/// 文件监控器
pub struct FileMonitor {
    #[cfg(unix)]
    pub inotify_fd: c_int,
    #[cfg(unix)]
    epoll_fd: c_int,
}

impl FileMonitor {
    #[cfg(unix)]
    pub fn new() -> Result<Self> {
        // 外部函数声明（仅Unix）
        unsafe extern "C" {
            fn inotify_init() -> c_int;
            fn epoll_create1(flags: c_int) -> c_int;
        }

        let inotify_fd = unsafe { inotify_init() };
        if inotify_fd == -1 {
            return Err(FreePPSError::InotifyError("无法初始化inotify".to_string()).into());
        }

        // 创建epoll实例
        let epoll_fd = unsafe { epoll_create1(0) };
        if epoll_fd == -1 {
            unsafe {
                libc::close(inotify_fd);
            }
            return Err(FreePPSError::InotifyError("无法初始化epoll".to_string()).into());
        }

        Ok(Self {
            inotify_fd,
            epoll_fd,
        })
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
            fn epoll_ctl(epfd: c_int, op: c_int, fd: c_int, event: *mut libc::epoll_event)
            -> c_int;
        }

        let path_cstring = CString::new(path)
            .map_err(|e| FreePPSError::InotifyError(format!("路径转换失败: {}", e)))?;

        let wd = unsafe { inotify_add_watch(self.inotify_fd, path_cstring.as_ptr(), mask) };

        if wd == -1 {
            return Err(FreePPSError::InotifyError(format!("无法监控文件: {}", path)).into());
        }

        // 将inotify_fd添加到epoll中
        let mut event = libc::epoll_event {
            events: libc::EPOLLIN as u32,
            u64: self.inotify_fd as u64,
        };

        let result = unsafe {
            epoll_ctl(
                self.epoll_fd,
                libc::EPOLL_CTL_ADD,
                self.inotify_fd,
                &mut event,
            )
        };

        if result == -1 {
            return Err(
                FreePPSError::InotifyError(format!("无法将inotify添加到epoll: {}", path)).into(),
            );
        }

        Ok(wd)
    }

    /// 创建uevent监控
    #[cfg(unix)]
    pub fn create_uevent_monitor() -> Result<c_int> {
        use std::mem;

        unsafe {
            // 创建netlink socket用于监听uevent
            let sock = libc::socket(
                libc::PF_NETLINK,
                libc::SOCK_DGRAM,
                libc::NETLINK_KOBJECT_UEVENT,
            );

            if sock == -1 {
                return Err(FreePPSError::InotifyError("无法创建uevent socket".to_string()).into());
            }

            // 绑定socket
            let mut sa: libc::sockaddr_nl = mem::zeroed();
            sa.nl_family = libc::AF_NETLINK as u16;
            sa.nl_groups = 0x1; // 接收uevent组消息
            sa.nl_pid = 0; // 内核发送给用户空间

            let result = libc::bind(
                sock,
                &sa as *const libc::sockaddr_nl as *const libc::sockaddr,
                mem::size_of::<libc::sockaddr_nl>() as u32,
            );

            if result == -1 {
                libc::close(sock);
                return Err(FreePPSError::InotifyError("无法绑定uevent socket".to_string()).into());
            }

            Ok(sock)
        }
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

            if self.epoll_fd != -1 {
                unsafe {
                    close(self.epoll_fd);
                }
            }
        }
    }
}
