use std::{
    collections::BTreeMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{
    header::{HeaderMap, CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE},
    redirect::Policy,
    Client,
};
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

    pub async fn get_user_info_settled(&self, timeout: Duration) -> Result<UserInfo, SrunError> {
        let info = self.get_user_info_best_effort(timeout).await?;
        if info.is_rate_limited() {
            return Err(SrunError::Login(LoginFailure::new(
                LoginFailureKind::Unknown,
                "speed_limit_error",
                "门户查询频率受限，请稍后重试",
            )));
        }
        Ok(info)
    }

    pub async fn get_user_info_best_effort(
        &self,
        timeout: Duration,
    ) -> Result<UserInfo, SrunError> {
        let deadline = Instant::now() + timeout;
        loop {
            let info = self.get_user_info().await?;
            if !info.is_rate_limited() || Instant::now() >= deadline {
                return Ok(info);
            }
            tokio::time::sleep(Duration::from_millis(1200)).await;
        }
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
        let before = self.get_user_info_settled(Duration::from_secs(10)).await?;
        let ip = before.client_ip().unwrap_or_default().to_owned();
        let client_ip = if ip.is_empty() {
            self.get_user_info_settled(Duration::from_secs(10))
                .await?
                .client_ip()
                .unwrap_or_default()
                .to_owned()
        } else {
            ip
        };
        self.login_with_ip(account, &client_ip, true).await
    }

    pub async fn login_after_logout(
        &self,
        account: &SrunAccount,
        client_ip: &str,
    ) -> Result<UserInfo, SrunError> {
        self.login_with_ip(account, client_ip, false).await
    }

    pub async fn login_after_logout_confirmed(
        &self,
        account: &SrunAccount,
        client_ip: &str,
    ) -> Result<UserInfo, SrunError> {
        self.login_with_ip(account, client_ip, true).await
    }

    async fn login_with_ip(
        &self,
        account: &SrunAccount,
        client_ip: &str,
        require_confirmation: bool,
    ) -> Result<UserInfo, SrunError> {
        if client_ip.is_empty() {
            return Err(SrunError::Login(LoginFailure::new(
                LoginFailureKind::MissingClientIp,
                "missing_client_ip",
                "门户没有返回客户端 IP",
            )));
        }

        let real_username = build_username(&account.username, &self.profile.domain);
        let challenge = self.get_challenge(&real_username, client_ip).await?;
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
                ("username", real_username.clone()),
                ("password", format!("{{MD5}}{hmd5}")),
                ("os", "Windows".to_owned()),
                ("name", "Windows".to_owned()),
                ("double_stack", "0".to_owned()),
                ("chksum", checksum),
                ("info", info_encoded),
                ("ac_id", self.profile.ac_id.clone()),
                ("ip", client_ip.to_owned()),
                ("n", "200".to_owned()),
                ("type", "1".to_owned()),
                ("_", unix_millis().to_string()),
            ])
            .send()
            .await?;
        let login_result = response
            .text()
            .await
            .map_err(SrunError::from)
            .and_then(|text| parse_jsonp(&text));
        let response_succeeded = login_result.as_ref().is_ok_and(login_value_succeeded);
        let response_code = login_result
            .as_ref()
            .ok()
            .and_then(login_error_code)
            .unwrap_or_default();
        let confirmation_timeout = if response_succeeded {
            Duration::from_secs(12)
        } else if response_code == "speed_limit_error" {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(3)
        };
        if let Some(session) = self
            .wait_for_authenticated_session(&real_username, confirmation_timeout)
            .await
        {
            return Ok(session);
        }

        let value = login_result?;
        if response_succeeded {
            if !require_confirmation {
                return Ok(user_info_from_login_response(
                    &value,
                    &real_username,
                    client_ip,
                ));
            }
            return Err(SrunError::Login(LoginFailure::new(
                LoginFailureKind::Unknown,
                "session_not_confirmed",
                "登录接口返回成功，但未确认目标账号会话",
            )));
        }
        let code = login_error_code(&value).unwrap_or_default();
        let message = string_field(&value, "error_msg")
            .or_else(|| string_field(&value, "message"))
            .or_else(|| string_field(&value, "res"))
            .unwrap_or_default();
        Err(SrunError::Login(LoginFailure::classify(&code, &message)))
    }

    async fn wait_for_authenticated_session(
        &self,
        expected_username: &str,
        timeout: Duration,
    ) -> Option<UserInfo> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(info) = self.get_user_info().await {
                if info.is_online()
                    && info
                        .display_username()
                        .is_some_and(|actual| same_portal_identity(actual, expected_username))
                {
                    return Some(info);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(1200)).await;
        }
    }

    pub async fn logout(&self, info: &UserInfo) -> Result<(), SrunError> {
        if self.profile.service_url.is_some() {
            match self.logout_via_self_service(info).await {
                Ok(()) => return Ok(()),
                Err(service_error) => {
                    return match self.logout_via_portal(info).await {
                        Ok(()) => Ok(()),
                        Err(portal_error) => Err(SrunError::InvalidResponse(format!(
                            "自助服务注销失败：{service_error}；认证门户注销失败：{portal_error}"
                        ))),
                    };
                }
            }
        }
        self.logout_via_portal(info).await
    }

    async fn logout_via_self_service(&self, info: &UserInfo) -> Result<(), SrunError> {
        let service_url = self
            .profile
            .service_url
            .as_deref()
            .ok_or_else(|| SrunError::InvalidResponse("校园网自助服务地址未配置".into()))?;
        let service = Url::parse(service_url)
            .map_err(|error| SrunError::InvalidResponse(error.to_string()))?;
        let username = info
            .display_username()
            .and_then(|value| value.split('@').next())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| SrunError::InvalidResponse("缺少用于自助服务登录的账号".into()))?;
        let sso_data = STANDARD.encode(format!("zh-CN:{username}"));
        let mut url = service
            .join("/site/sso")
            .map_err(|error| SrunError::InvalidResponse(error.to_string()))?;
        url.query_pairs_mut().append_pair("data", &sso_data);

        let http = Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .user_agent(concat!("VGCampusWifiAssistant/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let mut cookies = BTreeMap::new();
        let mut home_html = None;

        for _ in 0..8 {
            let response = http
                .get(url.clone())
                .header(COOKIE, cookie_header(&cookies))
                .send()
                .await?;
            absorb_cookies(response.headers(), &mut cookies);
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| SrunError::InvalidResponse("自助服务登录跳转缺少地址".into()))?;
                url = response
                    .url()
                    .join(location)
                    .map_err(|error| SrunError::InvalidResponse(error.to_string()))?;
                continue;
            }
            if !response.status().is_success() {
                return Err(SrunError::InvalidResponse(format!(
                    "自助服务登录返回 HTTP {}",
                    response.status()
                )));
            }
            if !response.url().path().starts_with("/home") {
                return Err(SrunError::InvalidResponse(
                    "自助服务登录后没有进入用户首页".into(),
                ));
            }
            home_html = Some(response.text().await?);
            break;
        }

        let home_html = home_html
            .ok_or_else(|| SrunError::InvalidResponse("自助服务登录跳转次数过多".into()))?;
        let csrf = extract_csrf_token(&home_html)
            .ok_or_else(|| SrunError::InvalidResponse("自助服务页面缺少 CSRF token".into()))?;
        let onekey_url = service
            .join("/home/onekey")
            .map_err(|error| SrunError::InvalidResponse(error.to_string()))?;
        let response = http
            .post(onekey_url)
            .header(COOKIE, cookie_header(&cookies))
            .header("X-CSRF-Token", &csrf)
            .header(
                CONTENT_TYPE,
                "application/x-www-form-urlencoded;charset=UTF-8",
            )
            .form(&[("_csrf-8800", csrf.as_str())])
            .send()
            .await?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if location.contains("login") || location.contains("sso") {
                return Err(SrunError::InvalidResponse(
                    "自助服务一键下线后跳回了登录页".into(),
                ));
            }
            return Ok(());
        }
        if response.status().is_success() {
            return Ok(());
        }
        Err(SrunError::InvalidResponse(format!(
            "自助服务一键下线返回 HTTP {}",
            response.status()
        )))
    }

    async fn logout_via_portal(&self, info: &UserInfo) -> Result<(), SrunError> {
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

fn user_info_from_login_response(value: &Value, username: &str, client_ip: &str) -> UserInfo {
    let mut info = serde_json::from_value::<UserInfo>(value.clone()).unwrap_or_default();
    if info.error.is_none() {
        info.error = Some("ok".into());
    }
    if info.display_username().is_none() {
        info.user_name = Some(username.to_owned());
    }
    if info.client_ip.is_none() {
        info.client_ip = Some(client_ip.to_owned());
    }
    if info.online_ip.is_none() {
        info.online_ip = Some(client_ip.to_owned());
    }
    info
}

fn absorb_cookies(headers: &HeaderMap, cookies: &mut BTreeMap<String, String>) {
    for value in headers.get_all(SET_COOKIE).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let Some((name, value_and_attributes)) = value.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value_and_attributes
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        if !name.is_empty() {
            cookies.insert(name.to_owned(), value.to_owned());
        }
    }
}

fn cookie_header(cookies: &BTreeMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn extract_csrf_token(html: &str) -> Option<String> {
    let patterns = [
        r#"(?i)name=["']_csrf-8800["']\s+value=["']([^"']+)["']"#,
        r#"(?i)name=["']csrf-token["']\s+content=["']([^"']+)["']"#,
    ];
    patterns.iter().find_map(|pattern| {
        regex::Regex::new(pattern)
            .ok()?
            .captures(html)?
            .get(1)
            .map(|value| value.as_str().to_owned())
    })
}

pub async fn probe_external() -> ExternalProbe {
    static CLIENT: OnceLock<Option<Client>> = OnceLock::new();
    let Some(client) = CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("VGCampusWifiAssistant/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()
    }) else {
        return ExternalProbe::Unreachable;
    };

    if let Ok(response) = client.get("https://www.baidu.com/robots.txt").send().await {
        if response.status().is_success() {
            return ExternalProbe::Reachable;
        }
    }
    if let Ok(response) = client
        .get("http://connect.rom.miui.com/generate_204")
        .send()
        .await
    {
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return ExternalProbe::Reachable;
        }
    }
    if let Ok(response) = client
        .get("http://connectivitycheck.gstatic.com/generate_204")
        .send()
        .await
    {
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return ExternalProbe::Reachable;
        }
    }
    ExternalProbe::Unreachable
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

fn login_error_code(value: &Value) -> Option<String> {
    string_field(value, "error").or_else(|| string_field(value, "res"))
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

pub fn same_portal_identity(actual: &str, expected: &str) -> bool {
    let actual = actual.trim();
    let expected = expected.trim();
    if actual.eq_ignore_ascii_case(expected) {
        return true;
    }
    if expected.contains('@') {
        return false;
    }
    actual
        .split_once('@')
        .is_some_and(|(base, _)| base.eq_ignore_ascii_case(expected))
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
            service_url: None,
        };
        assert!(profile.validate().is_err());
    }

    #[test]
    fn portal_identity_must_match_the_authenticated_account() {
        assert!(same_portal_identity("10000001", "10000001"));
        assert!(same_portal_identity("10000001@cmcc", "10000001"));
        assert!(same_portal_identity("10000001@cmcc", "10000001@cmcc"));
        assert!(!same_portal_identity("10000001@ctcc", "10000001@cmcc"));
        assert!(!same_portal_identity("20000002", "10000001"));
    }

    #[test]
    fn extracts_self_service_csrf_token() {
        let html = r#"<form><input type="hidden" name="_csrf-8800" value="token/value"></form>"#;
        assert_eq!(extract_csrf_token(html).as_deref(), Some("token/value"));
    }

    #[test]
    fn serializes_cookies_for_self_service_requests() {
        let mut cookies = BTreeMap::new();
        cookies.insert("session".into(), "abc".into());
        cookies.insert("identity".into(), "def".into());
        assert_eq!(cookie_header(&cookies), "identity=def; session=abc");
    }

    #[test]
    fn successful_login_response_can_seed_a_session_snapshot() {
        let value = json!({ "error": "ok" });
        let info = user_info_from_login_response(&value, "10000001", "10.0.0.2");
        assert!(info.is_online());
        assert_eq!(info.display_username(), Some("10000001"));
        assert_eq!(info.client_ip(), Some("10.0.0.2"));
    }
}
