use std::sync::Arc;

use anyhow::{Context, Result};
use srun_ipc::{decode_request, encode_response, CommandRequest, CommandResponse, PIPE_NAME};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
};

use crate::logging;
use crate::security::PipeSecurity;
use crate::service::DaemonService;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn serve(service: Arc<DaemonService>, owner_sid: String) -> Result<()> {
    let mut first_instance = true;
    loop {
        let server = {
            let mut security = PipeSecurity::for_owner(&owner_sid)?;
            unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first_instance)
                    .create_with_security_attributes_raw(PIPE_NAME, security.attributes_ptr())
            }
        }
        .with_context(|| format!("无法创建命名管道 {PIPE_NAME}"))?;
        first_instance = false;
        server.connect().await.context("命名管道客户端连接失败")?;
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(server, service).await {
                logging::debug(format!("命名管道请求结束：{error}"));
            }
        });
    }
}

async fn handle_connection(mut server: NamedPipeServer, service: Arc<DaemonService>) -> Result<()> {
    let request = read_request(&mut server).await?;
    let response = match request {
        Ok(request) => service.dispatch(request).await,
        Err(message) => CommandResponse::failure(0, message),
    };
    let encoded = encode_response(&response)?;
    server.write_all(&encoded).await?;
    server.flush().await?;
    Ok(())
}

async fn read_request(
    server: &mut NamedPipeServer,
) -> Result<std::result::Result<CommandRequest, String>> {
    let mut payload = Vec::with_capacity(1024);
    let mut buffer = [0u8; 4096];
    loop {
        let read = tokio::time::timeout(REQUEST_TIMEOUT, server.read(&mut buffer)).await;
        let count = match read {
            Ok(Ok(0)) => break,
            Ok(Ok(count)) => count,
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => return Ok(Err("IPC 请求读取超时".into())),
        };
        payload.extend_from_slice(&buffer[..count]);
        if payload.len() > MAX_REQUEST_BYTES {
            return Ok(Err("IPC 请求超过大小限制".into()));
        }
        if payload.last() == Some(&b'\n') {
            break;
        }
    }
    if payload.is_empty() {
        return Ok(Err("IPC 请求为空".into()));
    }
    Ok(decode_request(&payload).map_err(|error| format!("IPC 请求格式无效：{error}")))
}
