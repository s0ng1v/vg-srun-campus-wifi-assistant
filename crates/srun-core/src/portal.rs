use std::{sync::OnceLock, time::Duration};

use reqwest::Client;
use serde_json::{json, Value};
use url::Url;

use crate::crypto::{build_username, hmac_md5_hex, sha1_hex, srun_base64_encode, xencode};
use crate::model::{PortalProfile, SrunAccount, UserInfo};
use crate::{LoginFailure, LoginFailureKind, SrunError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalProbe {
    Reachable,
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct PortalClient {
    http: Client,
    profile: PortalProfile,
}

impl PortalClient {
    pub fn new(profile: PortalProfile) -> Result<Self, SrunError> {
        profile.validate()?;
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("VGCampusWifiAssistant/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http, profile })
    }

    pub fn profile(&self) -> &PortalProfile {
        &self.profile
    }

    pub async fn get_user_info(&self) -> Result<UserInfo, SrunError> {
        let url = self.endpoint("/cgi-bin/rad_user_info")?;
        let response = self
            .http
            .get(url)
            .query(&[
                ("callback", callback_name()),
                ("_", unix_millis().to_string()),
            ])
            .send()
            .await?;
        let text = response.text().await?;
        let value = parse_jsonp(&text)?;
        serde_json::from_value(value).map_err(|error| SrunError::InvalidResponse(error.to_string()))
    }

    pub async fn is_online(&self) -> Result<bool, SrunError> {
        Ok(self.get_user_info().await?.is_online())
    }

    pub async fn get_challenge(&self, username: &str, ip: &str) -> Result<String, SrunError> {
        let url = self.endpoint("/cgi-bin/get_challenge")?;
        let response = self
            .http
            .get(url)
            .query(&[
                ("callback", callback_name()),
                ("username", username.to_owned()),
                ("ip", ip.to_owned()),
                ("_", unix_millis().to_string()),
            ])
            .send()
            .await?;
        let text = response.text().await?;
        let value = parse_jsonp(&text)?;
        value
            .get("challenge")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| SrunError::InvalidResponse("门户没有返回 challenge".into()))
    }

    pub async fn login(&self, account: &SrunAccount) -> Result<UserInfo, SrunError> {
        let real_username = build_username(&account.username, &self.profile.domain);
        let before = self.get_user_info().await?;
        let ip = before.client_ip().unwrap_or_default().to_owned();
        let challenge = self.get_challenge(&real_username, &ip).await?;

        let client_ip = if ip.is_empty() {
            self.get_user_info()
                .await?
                .client_ip()
                .unwrap_or_default()
                .to_owned()
        } else {
            ip
        };
        if client_ip.is_empty() {
            return Err(SrunError::Login(LoginFailure::new(
                LoginFailureKind::MissingClientIp,
                "missing_client_ip",
                "门户没有返回客户端 IP",
            )));
        }

        let hmd5 = hmac_md5_hex(&account.password, &challenge);
        let info = json!({
            "username": real_username,
            "password": account.password,
            "ip": client_ip,
            "acid": self.profile.ac_id,
            "enc_ver": "srun_bx1",
        });
        let info_json = serde_json::to_string(&info)
            .map_err(|error| SrunError::InvalidResponse(error.to_string()))?;
        let info_encoded = format!(
            "{{SRBX1}}{}",
            srun_base64_encode(&xencode(&info_json, &challenge))
        );
        let checksum_input = format!(
            "{challenge}{real_username}{challenge}{hmd5}{challenge}{ac_id}{challenge}{client_ip}{challenge}200{challenge}1{challenge}{info_encoded}",
            ac_id = self.profile.ac_id,
        );
        let checksum = sha1_hex(&checksum_input);
        let url = self.endpoint("/cgi-bin/srun_portal")?;
        let response = self
            .http
            .get(url)
            .query(&[
                ("callback", callback_name()),
                ("action", "login".to_owned()),
                ("username", real_username),
                ("password", format!("{{MD5}}{hmd5}")),
                ("os", "Windows".to_owned()),
                ("name", "Windows".to_owned()),
                ("double_stack", "0".to_owned()),
                ("chksum", checksum),
                ("info", info_encoded),
                ("ac_id", self.profile.ac_id.clone()),
                ("ip", client_ip),
                ("n", "200".to_owned()),
                ("type", "1".to_owned()),
                ("_", unix_millis().to_string()),
            ])
            .send()
            .await?;
        let text = response.text().await?;
        let value = parse_jsonp(&text)?;
        let result: UserInfo = serde_json::from_value(value.clone())
            .map_err(|error| SrunError::InvalidResponse(error.to_string()))?;

        if login_value_succeeded(&value) {
            return Ok(result);
        }

        let code = string_field(&value, "error")
            .or_else(|| string_field(&value, "res"))
            .unwrap_or_default();
        let message = string_field(&value, "error_msg")
            .or_else(|| string_field(&value, "message"))
            .or_else(|| string_field(&value, "res"))
            .unwrap_or_default();
        Err(SrunError::Login(LoginFailure::classify(&code, &message)))
    }

    pub async fn logout(&self, info: &UserInfo) -> Result<(), SrunError> {
        let username = info
            .display_username()
            .ok_or_else(|| SrunError::InvalidResponse("缺少用于注销的账号".into()))?;
        let ip = info
            .client_ip()
            .ok_or_else(|| SrunError::InvalidResponse("缺少用于注销的客户端 IP".into()))?;
        let url = self.endpoint("/cgi-bin/srun_portal")?;
        let response = self
            .http
            .get(url)
            .query(&[
                ("callback", callback_name()),
                ("action", "logout".to_owned()),
                ("username", username.to_owned()),
                ("ac_id", self.profile.ac_id.clone()),
                ("ip", ip.to_owned()),
                ("_", unix_millis().to_string()),
            ])
            .send()
            .await?;
        let value = parse_jsonp(&response.text().await?)?;
        if login_value_succeeded(&value) || value.get("error").and_then(Value::as_str) == Some("ok")
        {
            Ok(())
        } else {
            let code = string_field(&value, "error").unwrap_or_default();
            let message = string_field(&value, "error_msg").unwrap_or_default();
            Err(SrunError::Login(LoginFailure::classify(&code, &message)))
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, SrunError> {
        let mut base = self.profile.validate()?;
        base.set_path(path);
        base.set_query(None);
        base.set_fragment(None);
        Ok(base)
    }
}

pub async fn probe_external() -> ExternalProbe {
    static CLIENT: OnceLock<Option<Client>> = OnceLock::new();
    let Some(client) = CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()
    }) else {
        return ExternalProbe::Unreachable;
    };

    match client.get("https://www.baidu.com/robots.txt").send().await {
        Ok(response) if response.status().is_success() => ExternalProbe::Reachable,
        _ => ExternalProbe::Unreachable,
    }
}

fn parse_jsonp(input: &str) -> Result<Value, SrunError> {
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("ok") {
        return Ok(serde_json::json!({ "error": "ok" }));
    }
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed)
            .map_err(|error| SrunError::InvalidResponse(error.to_string()));
    }
    let start = trimmed
        .find('(')
        .ok_or_else(|| SrunError::InvalidResponse("缺少 JSONP 起始括号".into()))?;
    let end = trimmed
        .rfind(')')
        .ok_or_else(|| SrunError::InvalidResponse("缺少 JSONP 结束括号".into()))?;
    if end <= start {
        return Err(SrunError::InvalidResponse("JSONP 边界无效".into()));
    }
    serde_json::from_str(trimmed[start + 1..end].trim())
        .map_err(|error| SrunError::InvalidResponse(error.to_string()))
}

fn login_value_succeeded(value: &Value) -> bool {
    ["error", "res"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        .any(|value| value.eq_ignore_ascii_case("ok"))
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn callback_name() -> String {
    format!("jQuery_{}", unix_millis())
}

fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jsonp_and_plain_json() {
        let value = parse_jsonp("jQuery_1({\"error\":\"ok\",\"uid\":\"7\"})").unwrap();
        assert_eq!(value["error"], "ok");
        assert_eq!(parse_jsonp("{\"error\":\"ok\"}").unwrap()["error"], "ok");
        assert_eq!(parse_jsonp("ok").unwrap()["error"], "ok");
    }

    #[test]
    fn rejects_http_authentication_portals() {
        let profile = PortalProfile {
            base_url: "http://portal.example.edu.cn".into(),
            ac_id: "1".into(),
            domain: String::new(),
        };
        assert!(profile.validate().is_err());
    }
}
