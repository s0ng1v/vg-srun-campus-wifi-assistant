use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use chrono::{DateTime, SecondsFormat, Utc};
use srun_core::{
    enrich_portal_profile, portal_profile_from_login_url, probe_external, ExternalProbe,
    LoginFailureKind, PortalClient, SrunAccount, SrunError, UserInfo,
};
use srun_ipc::{
    AccountDraft, AccountSummary, DaemonCommand, DaemonSnapshot, GuardPhase, PortalSnapshot,
    UserSnapshot,
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{load_config, save_config, AccountMeta, AppConfig, AppPaths};
use crate::logging;
use crate::notify::{self, NotificationEvent};
use crate::state::{replace_snapshot, SharedSnapshot, WorkerRequest};
use crate::vault::{CredentialRecord, CredentialVault};

const ONLINE_CHECK_INTERVAL: Duration = Duration::from_secs(60);
const FIRST_FAILURE_RECHECK: Duration = Duration::from_secs(10);
const NETWORK_BACKOFF: [Duration; 7] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
];
const ACCOUNT_BACKOFF: [Duration; 5] = [
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
    Duration::from_secs(600),
    Duration::from_secs(1800),
];

pub struct GuardWorker {
    paths: AppPaths,
    config: AppConfig,
    vault: CredentialVault,
    snapshot: SharedSnapshot,
    state: DaemonSnapshot,
    next_check: Option<Instant>,
    failure_count: u32,
    network_backoff_step: usize,
    account_backoff_step: usize,
    outage_started: Option<Instant>,
    outage_notified: bool,
    action_notification: Option<String>,
    portal_client: Option<(srun_core::PortalProfile, PortalClient)>,
}

impl GuardWorker {
    pub fn load(paths: AppPaths, snapshot: SharedSnapshot) -> Result<Self> {
        let config = load_config(&paths)?;
        let vault = CredentialVault::load(&paths.credentials)?;
        let mut state = DaemonSnapshot {
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            guard_enabled: config.guard_enabled,
            auto_start_enabled: config.auto_start_enabled,
            phase: if config.guard_enabled {
                GuardPhase::Starting
            } else {
                GuardPhase::Off
            },
            ..DaemonSnapshot::default()
        };
        fill_config_snapshot(&mut state, &config);
        Ok(Self {
            paths,
            next_check: config.guard_enabled.then(Instant::now),
            config,
            vault,
            snapshot,
            state,
            failure_count: 0,
            network_backoff_step: 0,
            account_backoff_step: 0,
            outage_started: None,
            outage_notified: false,
            action_notification: None,
            portal_client: None,
        })
    }

    pub async fn run(
        mut self,
        mut receiver: mpsc::Receiver<WorkerRequest>,
        mut network_events: mpsc::UnboundedReceiver<()>,
    ) {
        self.publish().await;
        loop {
            let deadline = self
                .next_check
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(24 * 60 * 60));
            tokio::select! {
                maybe_request = receiver.recv() => {
                    let Some(request) = maybe_request else {
                        break;
                    };
                    let should_shutdown = matches!(request.command, DaemonCommand::Shutdown);
                    let result = self.handle_command(request.command).await;
                    let _ = request.reply.send(result);
                    self.publish().await;
                    if should_shutdown {
                        break;
                    }
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                    if self.config.guard_enabled {
                        self.perform_cycle().await;
                        self.publish().await;
                    } else {
                        self.next_check = None;
                    }
                }
                event = network_events.recv() => {
                    if event.is_some() && self.config.guard_enabled {
                        self.next_check = Some(Instant::now());
                    }
                }
            }
        }
    }

    async fn handle_command(
        &mut self,
        command: DaemonCommand,
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        match command {
            DaemonCommand::SetGuard { enabled } => self.set_guard(enabled).await,
            DaemonCommand::SetAutoStart { enabled } => {
                self.config.auto_start_enabled = enabled;
                self.save_config()?;
                self.state.auto_start_enabled = enabled;
                Ok(Some(self.state.clone()))
            }
            DaemonCommand::Shutdown => {
                self.config.guard_enabled = false;
                self.save_config()?;
                self.state.guard_enabled = false;
                self.state.online = false;
                self.state.phase = GuardPhase::Off;
                self.next_check = None;
                Ok(Some(self.state.clone()))
            }
            DaemonCommand::CheckNow => {
                if !self.config.guard_enabled {
                    return Err("网络守护当前已关闭".into());
                }
                self.failure_count = 0;
                self.next_check = Some(Instant::now());
                Ok(Some(self.state.clone()))
            }
            DaemonCommand::SwitchAccount { id } => self.switch_account(&id).await,
            DaemonCommand::VerifyAccount { id } => self.verify_account(&id).await,
            DaemonCommand::SaveAccount { account } => self.save_account(account).await,
            DaemonCommand::RemoveAccount { id } => self.remove_account(&id),
            DaemonCommand::SetAccountEnabled { id, enabled } => {
                self.set_account_enabled(&id, enabled)
            }
            DaemonCommand::ReorderAccounts { ids } => self.reorder_accounts(&ids),
            DaemonCommand::SetPortal { portal_url } => self.set_portal(&portal_url).await,
            DaemonCommand::Ping | DaemonCommand::GetSnapshot => Ok(Some(self.state.clone())),
        }
    }

    async fn set_guard(
        &mut self,
        enabled: bool,
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        if enabled && self.config.portal.is_none() {
            return Err("请先完成校园网门户配置".into());
        }
        if enabled && self.enabled_account().is_none() {
            return Err("请先添加可用账号".into());
        }
        self.config.guard_enabled = enabled;
        self.save_config()?;
        self.state.guard_enabled = enabled;
        self.failure_count = 0;
        self.network_backoff_step = 0;
        self.account_backoff_step = 0;
        self.outage_started = None;
        self.outage_notified = false;
        self.action_notification = None;
        if enabled {
            self.state.phase = GuardPhase::Starting;
            self.state.last_error = None;
            self.next_check = Some(Instant::now());
        } else {
            self.state.phase = GuardPhase::Off;
            self.state.online = false;
            self.state.user = None;
            self.next_check = None;
        }
        Ok(Some(self.state.clone()))
    }

    async fn save_account(
        &mut self,
        draft: AccountDraft,
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        let username = draft.username.trim().to_owned();
        if username.is_empty() {
            return Err("账号不能为空".into());
        }
        let id = draft.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let existing = self.config.accounts.iter().find(|account| account.id == id);
        let password_was_changed = draft
            .password
            .as_deref()
            .is_some_and(|password| !password.is_empty());
        let password = match draft.password {
            Some(password) if !password.is_empty() => password,
            _ => self
                .vault
                .get(&id)
                .map(|record| record.password.clone())
                .ok_or_else(|| "新账号必须填写密码".to_string())?,
        };
        let verified = !password_was_changed
            && existing.is_some_and(|account| account.username == username && account.verified);
        self.vault
            .upsert(CredentialRecord {
                id: id.clone(),
                username: username.clone(),
                password,
            })
            .map_err(|error| error.to_string())?;
        self.vault
            .save(&self.paths.credentials)
            .map_err(|error| error.to_string())?;

        if let Some(account) = self
            .config
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
        {
            account.username = username;
            account.enabled = draft.enabled;
            account.verified = verified;
        } else {
            self.config.accounts.push(AccountMeta {
                id: id.clone(),
                username,
                enabled: draft.enabled,
                verified: false,
            });
        }
        if self.config.active_account_id.is_none() {
            self.config.active_account_id = Some(id);
        }
        self.save_config()?;
        fill_config_snapshot(&mut self.state, &self.config);
        Ok(Some(self.state.clone()))
    }

    fn remove_account(&mut self, id: &str) -> std::result::Result<Option<DaemonSnapshot>, String> {
        if self.config.active_account_id.as_deref() == Some(id) && self.state.online {
            return Err("请先切换账号，再移除当前在线账号".into());
        }
        self.config.accounts.retain(|account| account.id != id);
        self.vault.remove(id);
        self.vault
            .save(&self.paths.credentials)
            .map_err(|error| error.to_string())?;
        if self.config.active_account_id.as_deref() == Some(id) {
            self.config.active_account_id = self
                .config
                .accounts
                .iter()
                .find(|account| account.enabled)
                .map(|account| account.id.clone());
        }
        self.save_config()?;
        fill_config_snapshot(&mut self.state, &self.config);
        Ok(Some(self.state.clone()))
    }

    fn set_account_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        if self.config.active_account_id.as_deref() == Some(id) && !enabled && self.state.online {
            return Err("不能停用当前在线账号".into());
        }
        let account = self
            .config
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or_else(|| "账号不存在".to_string())?;
        account.enabled = enabled;
        self.save_config()?;
        fill_config_snapshot(&mut self.state, &self.config);
        Ok(Some(self.state.clone()))
    }

    fn reorder_accounts(
        &mut self,
        ids: &[String],
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        if ids.len() != self.config.accounts.len() {
            return Err("账号排序不完整".into());
        }
        let mut reordered = Vec::with_capacity(ids.len());
        for id in ids {
            let account = self
                .config
                .accounts
                .iter()
                .find(|account| &account.id == id)
                .ok_or_else(|| "账号排序包含未知账号".to_string())?;
            reordered.push(account.clone());
        }
        self.config.accounts = reordered;
        self.save_config()?;
        fill_config_snapshot(&mut self.state, &self.config);
        Ok(Some(self.state.clone()))
    }

    async fn set_portal(
        &mut self,
        portal_url: &str,
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        let profile =
            enrich_portal_profile(portal_profile_from_login_url(portal_url).map_err(safe_error)?)
                .await;
        profile.validate().map_err(|error| error.to_string())?;
        self.config.portal = Some(profile);
        self.portal_client = None;
        self.save_config()?;
        fill_config_snapshot(&mut self.state, &self.config);
        Ok(Some(self.state.clone()))
    }

    async fn verify_account(
        &mut self,
        id: &str,
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        if self.state.online && self.config.active_account_id.as_deref() != Some(id) {
            return Err("请先停止或切换当前在线会话，再验证其他账号".into());
        }
        let record = self
            .vault
            .get(id)
            .cloned()
            .ok_or_else(|| "凭据不存在".to_string())?;
        let client = self
            .cached_client()
            .map_err(|error| safe_error_ref(&error))?;
        self.state.phase = GuardPhase::Authenticating;
        match client
            .login(&SrunAccount {
                username: record.username,
                password: record.password,
            })
            .await
        {
            Ok(info) => {
                self.mark_verified(id);
                self.save_config()?;
                self.set_online(id, info).await;
                Ok(Some(self.state.clone()))
            }
            Err(error) => {
                self.set_error(&error);
                Err(safe_error(error))
            }
        }
    }

    async fn switch_account(
        &mut self,
        id: &str,
    ) -> std::result::Result<Option<DaemonSnapshot>, String> {
        let old_id = self.config.active_account_id.clone();
        let old_record = old_id.as_deref().and_then(|id| self.vault.get(id)).cloned();
        let target = self
            .vault
            .get(id)
            .cloned()
            .ok_or_else(|| "目标账号凭据不存在".to_string())?;
        let client = self
            .cached_client()
            .map_err(|error| safe_error_ref(&error))?;
        self.state.phase = GuardPhase::Reconnecting;

        let current = match client.get_user_info().await {
            Ok(info) => info,
            Err(error) => {
                self.set_error(&error);
                return Err(safe_error(error));
            }
        };
        if current.is_online() {
            client.logout(&current).await.map_err(safe_error)?;
        }

        match client
            .login(&SrunAccount {
                username: target.username,
                password: target.password,
            })
            .await
        {
            Ok(info) => {
                self.config.active_account_id = Some(id.to_owned());
                self.mark_verified(id);
                self.save_config()?;
                self.set_online(id, info).await;
                Ok(Some(self.state.clone()))
            }
            Err(target_error) => {
                if let Some(old) = old_record {
                    let _ = client
                        .login(&SrunAccount {
                            username: old.username,
                            password: old.password,
                        })
                        .await;
                }
                self.set_error(&target_error);
                Err(format!(
                    "目标账号切换失败，已尝试恢复原账号：{}",
                    safe_error(target_error)
                ))
            }
        }
    }

    async fn perform_cycle(&mut self) {
        if self.config.portal.is_none() {
            self.require_action("尚未配置校园网门户");
            return;
        }
        let Some(active_id) = self.config.active_account_id.clone() else {
            self.require_action("尚未添加可用账号");
            return;
        };
        if self.vault.get(&active_id).is_none() {
            self.require_action("当前账号凭据缺失，请重新录入密码");
            return;
        }

        let client = match self.cached_client() {
            Ok(client) => client,
            Err(error) => {
                self.require_action(&safe_error(error));
                return;
            }
        };
        self.state.phase = if self.state.online {
            GuardPhase::Reconnecting
        } else {
            GuardPhase::WaitingNetwork
        };
        let started = Instant::now();
        let (portal_result, external) = tokio::join!(client.get_user_info(), probe_external());
        self.state.metrics.last_probe_latency_ms = started.elapsed().as_millis() as u64;
        self.state.metrics.checks_total += 1;
        self.state.last_check_at = Some(now_string());

        match portal_result {
            Ok(info) if info.is_online() && external == ExternalProbe::Reachable => {
                self.failure_count = 0;
                self.network_backoff_step = 0;
                self.account_backoff_step = 0;
                self.set_online(&active_id, info).await;
                self.schedule_online_check();
            }
            Ok(info) if !info.is_online() => {
                self.failure_count = 0;
                self.network_backoff_step = 0;
                self.authenticate(client).await;
            }
            Ok(_) => {
                self.health_failure().await;
            }
            Err(error) => {
                self.set_error(&error);
                self.health_failure().await;
            }
        }
    }

    async fn health_failure(&mut self) {
        self.failure_count += 1;
        if self.outage_started.is_none() {
            self.outage_started = Some(Instant::now());
        }
        if !self.outage_notified
            && self
                .outage_started
                .is_some_and(|started| started.elapsed() >= Duration::from_secs(180))
        {
            notify::dispatch(&self.paths.root, NotificationEvent::NetworkDown);
            self.outage_notified = true;
        }
        if self.failure_count < 2 {
            self.state.phase = GuardPhase::Suspicious;
            self.next_check = Some(Instant::now() + FIRST_FAILURE_RECHECK);
            return;
        }
        self.state.phase = GuardPhase::WaitingNetwork;
        self.state.online = false;
        self.state.user = None;
        self.state.metrics.reconnect_total += 1;
        let wait = NETWORK_BACKOFF[self.network_backoff_step.min(NETWORK_BACKOFF.len() - 1)];
        self.network_backoff_step = (self.network_backoff_step + 1).min(NETWORK_BACKOFF.len() - 1);
        self.next_check = Some(Instant::now() + wait);
        self.state.next_retry_at = Some(
            (Utc::now() + chrono::Duration::from_std(wait).unwrap_or_default())
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        );
    }

    async fn authenticate(&mut self, client: PortalClient) {
        self.state.phase = GuardPhase::Authenticating;
        let active_id = self.config.active_account_id.clone();
        let account_ids: Vec<String> = active_id
            .into_iter()
            .chain(
                self.config
                    .accounts
                    .iter()
                    .filter(|account| account.enabled)
                    .map(|account| account.id.clone()),
            )
            .collect();
        let mut unique = Vec::new();
        for id in account_ids {
            if !unique.contains(&id) {
                unique.push(id);
            }
        }

        let mut last_failure: Option<SrunError> = None;
        for id in unique {
            let Some(record) = self.vault.get(&id).cloned() else {
                continue;
            };
            match client
                .login(&SrunAccount {
                    username: record.username,
                    password: record.password,
                })
                .await
            {
                Ok(info) => {
                    self.config.active_account_id = Some(id.clone());
                    self.mark_verified(&id);
                    let _ = self.save_config();
                    self.set_online(&id, info).await;
                    return;
                }
                Err(SrunError::Login(failure)) if failure.kind.should_try_next_account() => {
                    logging::warn(format!(
                        "账号认证失败，准备尝试下一位：id={id}, kind={:?}",
                        failure.kind
                    ));
                    last_failure = Some(SrunError::Login(failure));
                }
                Err(error) => {
                    self.set_error(&error);
                    if is_transient(&error) {
                        self.health_failure().await;
                    } else {
                        self.schedule_account_retry(&error);
                    }
                    return;
                }
            }
        }

        let error = last_failure.unwrap_or_else(|| {
            SrunError::Login(srun_core::LoginFailure::new(
                LoginFailureKind::Unknown,
                "no_account",
                "没有可用账号",
            ))
        });
        self.set_error(&error);
        self.schedule_account_retry(&error);
    }

    fn schedule_account_retry(&mut self, error: &SrunError) {
        if let SrunError::Login(failure) = error {
            let notification = match failure.kind {
                LoginFailureKind::InvalidCredentials | LoginFailureKind::AccountUnavailable => {
                    Some((
                        NotificationEvent::CredentialsRequired,
                        "credentials_required",
                    ))
                }
                LoginFailureKind::BalanceInsufficient => Some((
                    NotificationEvent::BalanceInsufficient,
                    "balance_insufficient",
                )),
                LoginFailureKind::DeviceLimit => {
                    Some((NotificationEvent::DeviceLimit, "device_limit"))
                }
                _ => None,
            };
            if let Some((event, marker)) = notification {
                if self.action_notification.as_deref() != Some(marker) {
                    notify::dispatch(&self.paths.root, event);
                    self.action_notification = Some(marker.to_owned());
                }
            }
            if failure.kind.should_pause() {
                self.require_action(&safe_error_ref(error));
                return;
            }
        }
        self.state.phase = GuardPhase::NeedsAction;
        let wait = ACCOUNT_BACKOFF[self.account_backoff_step.min(ACCOUNT_BACKOFF.len() - 1)];
        self.account_backoff_step = (self.account_backoff_step + 1).min(ACCOUNT_BACKOFF.len() - 1);
        self.next_check = Some(Instant::now() + wait);
        self.state.next_retry_at = Some(
            (Utc::now() + chrono::Duration::from_std(wait).unwrap_or_default())
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        );
    }

    async fn set_online(&mut self, account_id: &str, info: UserInfo) {
        if self.outage_notified {
            notify::dispatch(&self.paths.root, NotificationEvent::NetworkRestored);
        }
        self.outage_started = None;
        self.outage_notified = false;
        self.action_notification = None;
        self.failure_count = 0;
        self.network_backoff_step = 0;
        self.account_backoff_step = 0;
        self.state.phase = GuardPhase::Online;
        self.state.online = true;
        self.state.active_account_id = Some(account_id.to_owned());
        self.state.user = Some(user_snapshot(&info, &self.config));
        self.state.last_error = None;
        self.state.next_retry_at = None;
        if let Some(portal) = &self.config.portal {
            self.state.portal = Some(PortalSnapshot {
                host: portal.host().unwrap_or_default(),
                ac_id: portal.ac_id.clone(),
                display_name: portal.host().unwrap_or_default(),
            });
        }
        fill_config_snapshot(&mut self.state, &self.config);
    }

    fn mark_verified(&mut self, id: &str) {
        if let Some(account) = self
            .config
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
        {
            account.verified = true;
        }
    }

    fn require_action(&mut self, message: &str) {
        self.state.phase = GuardPhase::NeedsAction;
        self.state.online = false;
        self.state.last_error = Some(message.to_owned());
        self.next_check = None;
    }

    fn set_error(&mut self, error: &SrunError) {
        let message = safe_error_ref(error);
        logging::warn(format!("校园网操作失败：{message}"));
        self.state.last_error = Some(message);
    }

    fn schedule_online_check(&mut self) {
        let jitter = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            % 11) as i64
            - 5;
        let seconds = (ONLINE_CHECK_INTERVAL.as_secs() as i64 + jitter).max(45) as u64;
        self.next_check = Some(Instant::now() + Duration::from_secs(seconds));
        self.state.next_retry_at = None;
    }

    fn enabled_account(&self) -> Option<&AccountMeta> {
        self.config
            .active_account_id
            .as_deref()
            .and_then(|id| {
                self.config
                    .accounts
                    .iter()
                    .find(|account| account.id == id && account.enabled)
            })
            .or_else(|| self.config.accounts.iter().find(|account| account.enabled))
    }

    fn cached_client(&mut self) -> std::result::Result<PortalClient, SrunError> {
        let profile = self
            .config
            .portal
            .clone()
            .ok_or_else(|| SrunError::InvalidPortal("尚未配置校园网门户".into()))?;
        if let Some((cached_profile, client)) = &self.portal_client {
            if cached_profile == &profile {
                return Ok(client.clone());
            }
        }
        let client = PortalClient::new(profile.clone())?;
        self.portal_client = Some((profile, client.clone()));
        Ok(client)
    }

    fn save_config(&self) -> std::result::Result<(), String> {
        save_config(&self.paths, &self.config).map_err(|error| error.to_string())
    }

    async fn publish(&mut self) {
        let metrics = self.snapshot.read().await.metrics.clone();
        self.set_process_metrics(&metrics);
        replace_snapshot(&self.snapshot, self.state.clone()).await;
    }

    fn set_process_metrics(&mut self, metrics: &srun_ipc::RuntimeMetrics) {
        // The metrics sampler owns process CPU and memory. All other counters
        // remain owned by this worker.
        self.state.metrics.process_cpu_percent = metrics.process_cpu_percent;
        self.state.metrics.process_memory_bytes = metrics.process_memory_bytes;
    }
}

fn fill_config_snapshot(snapshot: &mut DaemonSnapshot, config: &AppConfig) {
    snapshot.guard_enabled = config.guard_enabled;
    snapshot.auto_start_enabled = config.auto_start_enabled;
    snapshot.active_account_id = config.active_account_id.clone();
    snapshot.accounts = config
        .accounts
        .iter()
        .map(|account| AccountSummary {
            id: account.id.clone(),
            username: account.username.clone(),
            enabled: account.enabled,
            verified: account.verified,
            active: config.active_account_id.as_deref() == Some(account.id.as_str()),
        })
        .collect();
    snapshot.portal = config.portal.as_ref().map(|portal| PortalSnapshot {
        host: portal.host().unwrap_or_default(),
        ac_id: portal.ac_id.clone(),
        display_name: portal.host().unwrap_or_default(),
    });
}

fn user_snapshot(info: &UserInfo, config: &AppConfig) -> UserSnapshot {
    UserSnapshot {
        username: info
            .display_username()
            .map(ToOwned::to_owned)
            .or_else(|| {
                config.active_account_id.as_ref().and_then(|id| {
                    config
                        .accounts
                        .iter()
                        .find(|account| &account.id == id)
                        .map(|account| account.username.clone())
                })
            })
            .unwrap_or_default(),
        ip: info.client_ip().unwrap_or_default().to_owned(),
        mac: info.user_mac.clone().unwrap_or_default(),
        online_since: info.add_time.as_deref().and_then(format_unix_timestamp),
        session_seconds: parse_u64(info.sum_seconds.as_deref()),
        total_bytes: parse_u64(info.sum_bytes.as_deref()),
        balance: info
            .user_balance
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok()),
        product_name: info.products_name.clone().unwrap_or_default(),
    }
}

fn parse_u64(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| *value >= 0.0)
        .map(|value| value as u64)
        .unwrap_or_default()
}

fn format_unix_timestamp(value: &str) -> Option<String> {
    let timestamp = value.parse::<i64>().ok()?;
    let datetime = if timestamp > 10_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(timestamp)?
    } else {
        DateTime::<Utc>::from_timestamp(timestamp, 0)?
    };
    Some(datetime.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn now_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn is_transient(error: &SrunError) -> bool {
    matches!(
        error,
        SrunError::Transport(_) | SrunError::InvalidResponse(_)
    )
}

fn safe_error(error: SrunError) -> String {
    safe_error_ref(&error)
}

fn safe_error_ref(error: &SrunError) -> String {
    match error {
        SrunError::InvalidPortal(message) => format!("门户配置错误：{message}"),
        SrunError::Transport(_) => "无法连接校园网门户".into(),
        SrunError::InvalidResponse(_) => "校园网门户响应格式异常".into(),
        SrunError::Login(failure) => match failure.kind {
            LoginFailureKind::InvalidCredentials => "账号或密码错误，请更新后重试".into(),
            LoginFailureKind::AccountUnavailable => "账号不可用，请联系网络中心".into(),
            LoginFailureKind::BalanceInsufficient => "账号余额不足，请充值后重试".into(),
            LoginFailureKind::DeviceLimit => "账号在线设备数已达上限".into(),
            LoginFailureKind::PortalUnavailable => "校园网认证服务暂时不可用".into(),
            LoginFailureKind::MissingClientIp => "门户没有返回客户端 IP".into(),
            LoginFailureKind::NeedsHumanAction => "登录需要验证码或人工操作".into(),
            LoginFailureKind::Unknown => "校园网登录未成功".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_url_preserves_ac_id() {
        let profile = portal_profile_from_login_url(
            "https://portal.example.edu.cn/srun_portal_pc?ac_id=5&theme=pro",
        )
        .unwrap();
        assert_eq!(profile.base_url, "https://portal.example.edu.cn");
        assert_eq!(profile.ac_id, "5");
    }

    #[test]
    fn portal_url_rejects_plain_http() {
        let profile =
            portal_profile_from_login_url("http://portal.example.edu.cn?ac_id=5").unwrap();
        assert_eq!(profile.base_url, "https://portal.example.edu.cn");
    }
}
