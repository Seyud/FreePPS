mod common;
mod monitoring;
mod pd;
mod platform;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

use common::constants::{PD_ADAPTER_VERIFIED_PATH, PD_VERIFIED_PATH};
use common::utils;
use monitoring::{
    ModuleManager, spawn_disable_file_monitor, spawn_free_file_monitor,
    spawn_pd_adapter_verified_monitor, spawn_pd_verified_monitor,
};
use pd::{PdAdapterVerifier, PdVerifier};
use platform::install_signal_handlers;

fn main() {
    let main_thread_name = utils::get_current_thread_name();
    info!("[{}] 启动FreePPS", main_thread_name);

    // 创建管理器实例
    let module_manager = Arc::new(ModuleManager::new().expect("创建模块管理器失败"));

    // 初始化阶段：确保基础文件存在并设置初始状态
    if let Err(e) = module_manager.initialize_module() {
        error!("模块初始化失败: {}", e);
    }

    // 创建运行标志
    let running = Arc::new(AtomicBool::new(true));
    install_signal_handlers(&running);

    let pd_verifier = Arc::new(PdVerifier::new().expect("创建PD验证器失败"));
    let pd_adapter_verifier = Arc::new(PdAdapterVerifier::new().expect("创建PD适配器验证器失败"));

    let mut thread_handles: Vec<thread::JoinHandle<()>> = Vec::new();

    // 创建free文件监控线程
    thread_handles.push(spawn_free_file_monitor(
        Arc::clone(&running),
        Arc::clone(&module_manager),
        Arc::clone(&pd_verifier),
        Arc::clone(&pd_adapter_verifier),
    ));

    // 创建disable文件监控线程
    thread_handles.push(spawn_disable_file_monitor(
        Arc::clone(&running),
        Arc::clone(&module_manager),
    ));

    // 初始化时按节点存在性一次性创建 qcom/mtk 线程（不做后续轮询判断/重启）
    if std::path::Path::new(PD_VERIFIED_PATH).exists() {
        info!("检测到qcom节点存在，启动qcom线程: {}", PD_VERIFIED_PATH);
        thread_handles.push(spawn_pd_verified_monitor(
            Arc::clone(&running),
            Arc::clone(&pd_verifier),
        ));
    } else {
        info!("qcom节点不存在，跳过qcom线程启动: {}", PD_VERIFIED_PATH);
    }

    if std::path::Path::new(PD_ADAPTER_VERIFIED_PATH).exists() {
        info!(
            "检测到mtk节点存在，启动mtk线程: {}",
            PD_ADAPTER_VERIFIED_PATH
        );
        thread_handles.push(spawn_pd_adapter_verified_monitor(
            Arc::clone(&running),
            Arc::clone(&pd_adapter_verifier),
        ));
    } else {
        info!(
            "mtk节点不存在，跳过mtk线程启动: {}",
            PD_ADAPTER_VERIFIED_PATH
        );
    }

    info!(
        "[{}] 监控线程已按需启动（仅初始化判断一次），主线程park等待...",
        main_thread_name
    );
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        thread::park_timeout(Duration::from_secs(1));
    }

    info!("检测到退出信号，开始停止所有监控线程...");
    running.store(false, std::sync::atomic::Ordering::Relaxed);

    for handle in thread_handles {
        if let Err(e) = handle.join() {
            error!("线程join失败: {:?}", e);
        }
    }

    info!("所有监控线程已停止，FreePPS 主进程退出");
}
