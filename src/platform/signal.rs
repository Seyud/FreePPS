use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[cfg(unix)]
use std::sync::atomic::AtomicPtr;

#[cfg(unix)]
static RUNNING_FLAG_PTR: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());

#[cfg(unix)]
/// # Safety
/// 该函数作为异步信号处理器调用，需要保证 `RUNNING_FLAG_PTR` 指向的内存有效。
unsafe extern "C" fn termination_signal_handler(_sig: libc::c_int) {
    let ptr = RUNNING_FLAG_PTR.load(std::sync::atomic::Ordering::SeqCst);
    if !ptr.is_null() {
        unsafe {
            (*ptr).store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

#[cfg(unix)]
/// 注册用于捕获终止信号的处理函数。
///
/// # Safety
/// 调用者必须保证传入的 `running` 在整个信号处理期间保持有效。
pub fn install_signal_handlers(running: &Arc<AtomicBool>) {
    RUNNING_FLAG_PTR.store(
        Arc::as_ptr(running) as *mut AtomicBool,
        std::sync::atomic::Ordering::SeqCst,
    );

    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = termination_signal_handler as usize;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = libc::SA_RESTART;

        if libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) == -1 {
            crate::error!("注册SIGINT处理器失败: {}", std::io::Error::last_os_error());
        }

        if libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) == -1 {
            crate::error!("注册SIGTERM处理器失败: {}", std::io::Error::last_os_error());
        }
    }
}

#[cfg(not(unix))]
pub fn install_signal_handlers(_: &Arc<AtomicBool>) {}
