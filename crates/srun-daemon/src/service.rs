use std::sync::Arc;

use srun_ipc::{CommandRequest, CommandResponse, DaemonCommand, DaemonSnapshot, PROTOCOL_VERSION};
use tokio::sync::{mpsc, oneshot};

use crate::state::{read_snapshot, SharedSnapshot, WorkerRequest, WorkerSender};

pub struct DaemonService {
    snapshot: SharedSnapshot,
    worker: WorkerSender,
}

impl DaemonService {
    pub fn new(snapshot: SharedSnapshot, worker: WorkerSender) -> Arc<Self> {
        Arc::new(Self { snapshot, worker })
    }

    pub async fn dispatch(&self, request: CommandRequest) -> CommandResponse {
        if request.protocol_version != PROTOCOL_VERSION {
            return CommandResponse::failure(
                request.request_id,
                format!(
                    "IPC 协议版本不匹配：客户端 {}，守护 {}",
                    request.protocol_version, PROTOCOL_VERSION
                ),
            );
        }

        match request.command {
            DaemonCommand::Ping => CommandResponse::success(
                request.request_id,
                Some(read_snapshot(&self.snapshot).await),
            ),
            DaemonCommand::GetSnapshot => CommandResponse::success(
                request.request_id,
                Some(read_snapshot(&self.snapshot).await),
            ),
            command => {
                let (reply, response) = oneshot::channel();
                if self
                    .worker
                    .send(WorkerRequest { command, reply })
                    .await
                    .is_err()
                {
                    return CommandResponse::failure(request.request_id, "守护 worker 已停止");
                }
                match tokio::time::timeout(std::time::Duration::from_secs(45), response).await {
                    Ok(Ok(Ok(snapshot))) => CommandResponse::success(request.request_id, snapshot),
                    Ok(Ok(Err(message))) => CommandResponse::failure(request.request_id, message),
                    Ok(Err(_)) => {
                        CommandResponse::failure(request.request_id, "守护 worker 未返回结果")
                    }
                    Err(_) => CommandResponse::failure(request.request_id, "守护操作超时"),
                }
            }
        }
    }
}

pub fn build_worker_channel(buffer: usize) -> (WorkerSender, mpsc::Receiver<WorkerRequest>) {
    mpsc::channel(buffer)
}

pub fn empty_snapshot() -> DaemonSnapshot {
    DaemonSnapshot {
        protocol_version: PROTOCOL_VERSION,
        daemon_version: env!("CARGO_PKG_VERSION").into(),
        ..DaemonSnapshot::default()
    }
}
