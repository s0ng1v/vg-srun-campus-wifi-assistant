use serde::{Deserialize, Serialize};

pub const PIPE_NAME: &str = r"\\.\pipe\VGCampusWifiAssistant.v1";
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardPhase {
    #[default]
    Off,
    Starting,
    WaitingNetwork,
    DiscoveringPortal,
    Authenticating,
    Online,
    Suspicious,
    Reconnecting,
    NeedsAction,
    Crashed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub username: String,
    pub enabled: bool,
    pub verified: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortalSnapshot {
    pub host: String,
    pub ac_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserSnapshot {
    pub username: String,
    pub ip: String,
    pub mac: String,
    pub online_since: Option<String>,
    pub session_seconds: u64,
    pub total_bytes: u64,
    pub balance: Option<f64>,
    pub product_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    pub process_cpu_percent: f64,
    pub process_memory_bytes: u64,
    pub last_probe_latency_ms: u64,
    pub checks_total: u64,
    pub reconnect_total: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonSnapshot {
    pub protocol_version: u16,
    pub daemon_version: String,
    pub guard_enabled: bool,
    pub auto_start_enabled: bool,
    pub phase: GuardPhase,
    pub online: bool,
    pub active_account_id: Option<String>,
    pub accounts: Vec<AccountSummary>,
    pub portal: Option<PortalSnapshot>,
    pub user: Option<UserSnapshot>,
    pub metrics: RuntimeMetrics,
    pub last_check_at: Option<String>,
    pub next_retry_at: Option<String>,
    pub last_error: Option<String>,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountDraft {
    pub id: Option<String>,
    pub username: String,
    pub password: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonCommand {
    Ping,
    GetSnapshot,
    SetGuard { enabled: bool },
    SetAutoStart { enabled: bool },
    CheckNow,
    SwitchAccount { id: String },
    VerifyAccount { id: String },
    SaveAccount { account: AccountDraft },
    RemoveAccount { id: String },
    SetAccountEnabled { id: String, enabled: bool },
    ReorderAccounts { ids: Vec<String> },
    SetPortal { portal_url: String },
    ClearDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    pub protocol_version: u16,
    pub request_id: u64,
    pub command: DaemonCommand,
}

impl CommandRequest {
    pub fn new(request_id: u64, command: DaemonCommand) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            command,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    pub protocol_version: u16,
    pub request_id: u64,
    pub ok: bool,
    pub message: Option<String>,
    pub snapshot: Option<DaemonSnapshot>,
}

impl CommandResponse {
    pub fn success(request_id: u64, snapshot: Option<DaemonSnapshot>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            ok: true,
            message: None,
            snapshot,
        }
    }

    pub fn failure(request_id: u64, message: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            ok: false,
            message: Some(message.into()),
            snapshot: None,
        }
    }
}

pub fn encode_request(request: &CommandRequest) -> serde_json::Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(request)?;
    encoded.push(b'\n');
    Ok(encoded)
}

pub fn decode_request(bytes: &[u8]) -> serde_json::Result<CommandRequest> {
    serde_json::from_slice(bytes.trim_ascii_end())
}

pub fn encode_response(response: &CommandResponse) -> serde_json::Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(response)?;
    encoded.push(b'\n');
    Ok(encoded)
}

pub fn decode_response(bytes: &[u8]) -> serde_json::Result<CommandResponse> {
    serde_json::from_slice(bytes.trim_ascii_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_round_trip_is_stable() {
        let request = CommandRequest::new(9, DaemonCommand::SetGuard { enabled: true });
        let bytes = encode_request(&request).unwrap();
        let decoded = decode_request(&bytes).unwrap();
        assert_eq!(decoded.request_id, 9);
        assert!(matches!(
            decoded.command,
            DaemonCommand::SetGuard { enabled: true }
        ));
    }
}
