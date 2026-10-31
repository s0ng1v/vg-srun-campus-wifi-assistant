mod config;
mod logging;
mod metrics;
mod pipe;
mod secret;
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
    let paths = AppPaths::discover()?;
    let _instance = SingleInstance::acquire()?;

    logging::init(&paths.logs)?;
    logging::info(format!(
        "校园网守护进程启动：version={}, data_root={}",
        env!("CARGO_PKG_VERSION"),
        paths.root.display()
    ));

    let snapshot: SharedSnapshot = Arc::new(RwLock::new(empty_snapshot()));
    tokio::spawn(metrics::sample_loop(Arc::clone(&snapshot)));
    let worker = GuardWorker::load(paths, Arc::clone(&snapshot))?;
    let (worker_sender, worker_receiver) = build_worker_channel(32);
    tokio::spawn(worker.run(worker_receiver));
    let service = DaemonService::new(Arc::clone(&snapshot), worker_sender);
    let pipe_service = Arc::clone(&service);

    let pipe_task = tokio::spawn(async move { pipe::serve(pipe_service).await });
    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.context("无法监听停止信号")?;
            logging::info("收到停止信号");
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
