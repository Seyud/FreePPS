use anyhow::Result;

/// A small eventfd wrapper used to wake a thread blocked in epoll.
pub struct EventFd {
    #[cfg(unix)]
    fd: libc::c_int,
}

impl EventFd {
    pub fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
            if fd == -1 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self { fd })
        }

        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    #[cfg(unix)]
    pub fn raw_fd(&self) -> libc::c_int {
        self.fd
    }

    /// Mark the event as ready. `eventfd` coalesces repeated notifications.
    pub fn notify(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            let value: u64 = 1;
            let result = unsafe {
                libc::write(
                    self.fd,
                    (&value as *const u64).cast::<libc::c_void>(),
                    std::mem::size_of::<u64>(),
                )
            };
            if result == -1 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::WouldBlock {
                    return Err(error);
                }
            }
        }

        Ok(())
    }

    /// Clear all pending notifications without blocking.
    #[cfg(unix)]
    pub fn clear(&self) -> std::io::Result<()> {
        let mut value = 0u64;
        let result = unsafe {
            libc::read(
                self.fd,
                (&mut value as *mut u64).cast::<libc::c_void>(),
                std::mem::size_of::<u64>(),
            )
        };
        if result == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::WouldBlock {
                return Err(error);
            }
        }
        Ok(())
    }
}

impl Drop for EventFd {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::close(self.fd);
        }
    }
}
