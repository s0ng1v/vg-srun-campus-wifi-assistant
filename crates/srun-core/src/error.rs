use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailureKind {
    InvalidCredentials,
    AccountUnavailable,
    BalanceInsufficient,
    DeviceLimit,
    PortalUnavailable,
    MissingClientIp,
    NeedsHumanAction,
    Unknown,
}

impl LoginFailureKind {
    pub fn should_try_next_account(self) -> bool {
        matches!(
            self,
            Self::InvalidCredentials
                | Self::AccountUnavailable
                | Self::BalanceInsufficient
                | Self::DeviceLimit
        )
    }

    pub fn should_pause(self) -> bool {
        matches!(
            self,
            Self::InvalidCredentials | Self::AccountUnavailable | Self::NeedsHumanAction
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginFailure {
    pub kind: LoginFailureKind,
    pub code: String,
    pub message: String,
}

impl LoginFailure {
    pub fn new(
        kind: LoginFailureKind,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
        }
    }

    pub(crate) fn classify(code: &str, message: &str) -> Self {
        let fingerprint = format!("{code} {message}").to_lowercase();
        let kind = if fingerprint.contains("password")
            || fingerprint.contains("e01")
            || fingerprint.contains("e2553")
            || fingerprint.contains("e2901")
            || fingerprint.contains("密码")
            || fingerprint.contains("账号错误")
        {
            LoginFailureKind::InvalidCredentials
        } else if fingerprint.contains("balance")
            || fingerprint.contains("insufficient")
            || fingerprint.contains("arrear")
            || fingerprint.contains("e2616")
            || fingerprint.contains("余额")
            || fingerprint.contains("欠费")
        {
            LoginFailureKind::BalanceInsufficient
        } else if fingerprint.contains("device")
            || fingerprint.contains("online_device")
            || fingerprint.contains("mac")
            || fingerprint.contains("e2620")
            || fingerprint.contains("设备")
            || fingerprint.contains("终端")
            || fingerprint.contains("超限")
        {
            LoginFailureKind::DeviceLimit
        } else if fingerprint.contains("user not")
            || fingerprint.contains("not exist")
            || fingerprint.contains("disabled")
            || fingerprint.contains("e2531")
            || fingerprint.contains("e2606")
            || fingerprint.contains("停用")
            || fingerprint.contains("禁用")
            || fingerprint.contains("不存在")
        {
            LoginFailureKind::AccountUnavailable
        } else if fingerprint.contains("captcha")
            || fingerprint.contains("verify")
            || fingerprint.contains("验证码")
            || fingerprint.contains("sso")
        {
            LoginFailureKind::NeedsHumanAction
        } else {
            LoginFailureKind::Unknown
        };
        Self::new(kind, code, message)
    }
}

#[derive(Debug, Error)]
pub enum SrunError {
    #[error("门户配置无效：{0}")]
    InvalidPortal(String),
    #[error("网络请求失败：{0}")]
    Transport(#[from] reqwest::Error),
    #[error("门户返回了无法解析的数据：{0}")]
    InvalidResponse(String),
    #[error("登录失败：{0:?}")]
    Login(LoginFailure),
}
