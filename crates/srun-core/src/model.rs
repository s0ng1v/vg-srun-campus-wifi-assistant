use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use url::Url;

use crate::SrunError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortalProfile {
    pub base_url: String,
    pub ac_id: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_url: Option<String>,
}

impl PortalProfile {
    pub fn validate(&self) -> Result<Url, SrunError> {
        let url = Url::parse(self.base_url.trim())
            .map_err(|error| SrunError::InvalidPortal(error.to_string()))?;
        if url.scheme() != "https" {
            return Err(SrunError::InvalidPortal("账号认证只允许 HTTPS 门户".into()));
        }
        if url.host_str().is_none() {
            return Err(SrunError::InvalidPortal("门户地址缺少主机名".into()));
        }
        if self.ac_id.trim().is_empty() {
            return Err(SrunError::InvalidPortal("门户缺少 ac_id".into()));
        }
        if let Some(service_url) = self
            .service_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let service = Url::parse(service_url)
                .map_err(|error| SrunError::InvalidPortal(error.to_string()))?;
            if service.scheme() != "https" || service.host_str().is_none() {
                return Err(SrunError::InvalidPortal(
                    "自助服务地址必须是有效的 HTTPS 地址".into(),
                ));
            }
        }
        Ok(url)
    }

    pub fn host(&self) -> Result<String, SrunError> {
        let url = self.validate()?;
        url.host_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| SrunError::InvalidPortal("门户地址缺少主机名".into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrunAccount {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserInfo {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub error: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub res: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub user_name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub username: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub uid: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub client_ip: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub online_ip: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub online_ip6: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub ip: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub user_mac: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub products_name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub sum_bytes: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub sum_seconds: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub user_balance: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub add_time: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub checkout_date: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub online_device_total: Option<String>,
    #[serde(default)]
    pub online_device_detail: Option<Value>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub error_msg: Option<String>,
}

impl UserInfo {
    pub fn is_online(&self) -> bool {
        self.error
            .as_deref()
            .is_some_and(|error| error.eq_ignore_ascii_case("ok"))
            && (self.user_name.is_some() || self.username.is_some() || self.uid.is_some())
    }

    pub fn is_rate_limited(&self) -> bool {
        self.error
            .as_deref()
            .is_some_and(|error| error.eq_ignore_ascii_case("speed_limit_error"))
    }

    pub fn client_ip(&self) -> Option<&str> {
        self.client_ip
            .as_deref()
            .or(self.online_ip.as_deref())
            .or(self.ip.as_deref())
            .filter(|value| !value.trim().is_empty())
    }

    pub fn display_username(&self) -> Option<&str> {
        self.user_name
            .as_deref()
            .or(self.username.as_deref())
            .filter(|value| !value.trim().is_empty())
    }
}

fn de_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::Null => None,
        Value::String(value) => Some(value),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => Some(value.to_string()),
    }))
}
