use std::time::Duration;

use regex::Regex;
use reqwest::{redirect::Policy, Client, StatusCode};
use url::Url;

use crate::{PortalProfile, SrunError};

const DISCOVERY_ENDPOINTS: [&str; 2] = [
    "http://connectivitycheck.gstatic.com/generate_204",
    "http://www.baidu.com/",
];

pub async fn discover_portal() -> Result<PortalProfile, SrunError> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(Policy::none())
        .user_agent(concat!("VGCampusWifiAssistant/", env!("CARGO_PKG_VERSION")))
        .build()?;

    for endpoint in DISCOVERY_ENDPOINTS {
        let Ok(response) = client.get(endpoint).send().await else {
            continue;
        };
        if response.status() == StatusCode::NO_CONTENT {
            continue;
        }
        let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
            continue;
        };
        let Ok(value) = location.to_str() else {
            continue;
        };
        let Ok(target) = Url::parse(endpoint).and_then(|base| base.join(value)) else {
            continue;
        };
        if let Ok(profile) = portal_profile_from_login_url(target.as_str()) {
            return Ok(profile);
        }
    }
    Err(SrunError::InvalidPortal(
        "未自动发现校园网门户，请粘贴浏览器中的登录页地址".into(),
    ))
}

pub fn portal_profile_from_login_url(input: &str) -> Result<PortalProfile, SrunError> {
    let normalized = if input.contains("://") {
        input.trim().to_owned()
    } else {
        format!("https://{}", input.trim())
    };
    let parsed =
        Url::parse(&normalized).map_err(|error| SrunError::InvalidPortal(error.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| SrunError::InvalidPortal("登录页地址缺少主机名".into()))?;
    let ac_id = parsed
        .query_pairs()
        .find(|(key, _)| key == "ac_id")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_else(|| "1".into());
    let profile = PortalProfile {
        base_url: format!("https://{host}"),
        ac_id,
        domain: String::new(),
    };
    profile.validate()?;
    Ok(profile)
}

pub fn extract_portal_domain(source: &str) -> Option<String> {
    let patterns = [
        r#"(?i)domain\s*[:=]\s*["']([^"']+)["']"#,
        r#""domain"\s*:\s*"([^"]+)""#,
    ];
    patterns.iter().find_map(|pattern| {
        Regex::new(pattern)
            .ok()
            .and_then(|regex| regex.captures(source))
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_http_login_page_is_upgraded_for_authentication() {
        let profile = portal_profile_from_login_url(
            "http://portal.example.edu.cn/srun_portal_pc?ac_id=5&theme=pro",
        )
        .unwrap();
        assert_eq!(profile.base_url, "https://portal.example.edu.cn");
        assert_eq!(profile.ac_id, "5");
    }

    #[test]
    fn extracts_domain_from_portal_javascript() {
        assert_eq!(
            extract_portal_domain(r#"var config = {"domain": "@campus"};"#),
            Some("@campus".into())
        );
    }
}
