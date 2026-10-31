use std::time::Duration;

use srun_ipc::{
    decode_response, encode_request, CommandRequest, CommandResponse, DaemonCommand, PIPE_NAME,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::ClientOptions,
};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

pub async fn request(
    request_id: u64,
    command: DaemonCommand,
    timeout: Duration,
) -> Result<CommandResponse, String> {
    tokio::time::timeout(timeout, exchange(request_id, command))
        .await
        .map_err(|_| "连接校园网守护超时".to_string())?
}

async fn exchange(request_id: u64, command: DaemonCommand) -> Result<CommandResponse, String> {
    let mut client = ClientOptions::new()
        .open(PIPE_NAME)
        .map_err(|_| "校园网守护没有运行".to_string())?;
    let request = CommandRequest::new(request_id, command);
    let encoded = encode_request(&request).map_err(|error| error.to_string())?;
    client
        .write_all(&encoded)
        .await
        .map_err(|error| format!("写入守护请求失败：{error}"))?;
    client
        .flush()
        .await
        .map_err(|error| format!("提交守护请求失败：{error}"))?;

    let mut payload = Vec::with_capacity(1024);
    let mut buffer = [0u8; 4096];
    loop {
        let read = client
            .read(&mut buffer)
            .await
            .map_err(|error| format!("读取守护响应失败：{error}"))?;
        if read == 0 {
            break;
        }
        payload.extend_from_slice(&buffer[..read]);
        if payload.len() > MAX_RESPONSE_BYTES {
            return Err("守护响应超过大小限制".into());
        }
        if payload.last() == Some(&b'\n') {
            break;
        }
    }
    if payload.is_empty() {
        return Err("守护没有返回响应".into());
    }
    decode_response(&payload).map_err(|error| format!("守护响应格式无效：{error}"))
}
