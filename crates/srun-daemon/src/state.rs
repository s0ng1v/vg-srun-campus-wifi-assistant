use std::sync::Arc;

use srun_ipc::{DaemonCommand, DaemonSnapshot};
use tokio::sync::{mpsc, oneshot, RwLock};

pub type SharedSnapshot = Arc<RwLock<DaemonSnapshot>>;

pub struct WorkerRequest {
    pub command: DaemonCommand,
    pub reply: oneshot::Sender<Result<Option<DaemonSnapshot>, String>>,
}

pub type WorkerSender = mpsc::Sender<WorkerRequest>;

pub async fn read_snapshot(shared: &SharedSnapshot) -> DaemonSnapshot {
    shared.read().await.clone()
}

pub async fn replace_snapshot(shared: &SharedSnapshot, snapshot: DaemonSnapshot) {
    *shared.write().await = snapshot;
}
