mod config;
mod fsutil;
mod logging;
mod metrics;
mod network_events;
mod notify;
mod owner;
mod pipe;
mod secret;
mod security;
mod service;
mod state;
mod vault;
mod worker;

use std::{process::ExitCode, sync::Arc};

use anyhow::{Context, Result};
use state::SharedSnapshot;
use tokio::sync::RwLock;

use crate::{
    config::AppPaths,
    service::{build_worker_channel, empty_snapshot, DaemonService},
    worker::GuardWorker,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("VG 校园网守护启动失败：{error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let on_start = std::env::args().any(|argument| argument == "--onstart");
    let paths = AppPaths::discover()?;
    let _instance = SingleInstance::acquire()?;
    let pipe_owner = owner::resolve(&paths.root)?;

    logging::init(&paths.logs)?;
    let config = config::load_config(&paths)?;
    if on_start && !config.guard_enabled {
        logging::info("网络守护未启用，跳过本次开机任务");
        return Ok(());
    }
    logging::info(format!(
        "校园网守护进程启动：version={}, data_root={}",
        env!("CARGO_PKG_VERSION"),
        paths.root.display()
    ));

    let snapshot: SharedSnapshot = Arc::new(RwLock::new(empty_snapshot()));
    tokio::spawn(metrics::sample_loop(Arc::clone(&snapshot)));
    let worker = GuardWorker::load(paths, Arc::clone(&snapshot))?;
    let (worker_sender, worker_receiver) = build_worker_channel(32);
    let (network_sender, network_receiver) = tokio::sync::mpsc::unbounded_channel();
    if let Err(error) = network_events::start(network_sender) {
        logging::warn(format!("{error:#}"));
    }
    let worker_task = tokio::spawn(worker.run(worker_receiver, network_receiver));
    let service = DaemonService::new(Arc::clone(&snapshot), worker_sender);
    let pipe_service = Arc::clone(&service);

    let pipe_task = tokio::spawn(async move { pipe::serve(pipe_service, pipe_owner).await });
    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.context("无法监听停止信号")?;
            logging::info("收到停止信号");
        }
        result = worker_task => {
            match result {
                Ok(()) => logging::info("守护 worker 已停止"),
                Err(error) => return Err(error.into()),
            }
        }
        result = pipe_task => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

struct SingleInstance {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl SingleInstance {
    fn acquire() -> Result<Self> {
        use windows_sys::Win32::{
            Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        };
        const NAME: &str = "Local\\VGCampusWifiAssistantDaemon.v1";
        let name: Vec<u16> = NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error()).context("无法创建守护单实例互斥量");
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
            anyhow::bail!("已有一个校园网守护进程在运行");
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
