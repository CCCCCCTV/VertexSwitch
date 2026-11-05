//! 检查更新：查 GitHub 上最新发布的版本。只读公开接口，请求里只带程序名和版本号（User-Agent），
//! 不下载、不安装任何东西；有新版时界面给出发布页链接。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use vertexswitch_core::tr;

pub const LATEST_URL: &str = "https://api.github.com/repos/CCCCCCTV/VertexSwitch/releases/latest";
/// 只打开本仓库的发布页
pub const RELEASES_PREFIX: &str = "https://github.com/CCCCCCTV/VertexSwitch/releases/";

#[derive(Debug, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    /// 最新发布的版本号（不带 v）；还没有发布过版本时为空
    pub latest: Option<String>,
    /// 最新版本的发布页
    pub url: Option<String>,
    pub newer: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

pub async fn check(url: &str, current: &str) -> Result<UpdateInfo, String> {
    let fail =
        |e: &dyn std::fmt::Display| tr!("检查更新失败：{e}", "Couldn't check for updates: {e}");
    let client = reqwest::Client::builder()
        .user_agent(format!("VertexSwitch/{current}"))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| fail(&e))?;
    let response = client
        .get(url)
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| fail(&e))?;
    let status = response.status();
    // 仓库还没有发布过版本
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateInfo {
            current: current.to_owned(),
            latest: None,
            url: None,
            newer: false,
        });
    }
    if !status.is_success() {
        return Err(fail(&format!("HTTP {status}")));
    }
    let release: Release = response.json().await.map_err(|e| fail(&e))?;
    Ok(from_release(current, release))
}

fn from_release(current: &str, release: Release) -> UpdateInfo {
    let latest = release.tag_name.trim_start_matches('v').to_owned();
    UpdateInfo {
        current: current.to_owned(),
        newer: is_newer(&latest, current),
        url: release
            .html_url
            .starts_with(RELEASES_PREFIX)
            .then_some(release.html_url),
        latest: Some(latest),
    }
}

/// 只比较“主.次.修订”三段数字；带预发布后缀等认不出的版本号不算新版。
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<u64> = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    match parts[..] {
        [major, minor, patch] => Some((major, minor, patch)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("v1.0.0", "0.9.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.3.0-beta", "0.2.0"));
        assert!(!is_newer("1.2.3.beta", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[tokio::test]
    async fn reads_latest_release() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/latest"))
            .and(header("user-agent", "VertexSwitch/0.1.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.2.0",
                "html_url": "https://github.com/CCCCCCTV/VertexSwitch/releases/tag/v0.2.0",
            })))
            .mount(&server)
            .await;
        let info = check(&format!("{}/latest", server.uri()), "0.1.0")
            .await
            .unwrap();
        assert_eq!(
            info,
            UpdateInfo {
                current: "0.1.0".into(),
                latest: Some("0.2.0".into()),
                url: Some("https://github.com/CCCCCCTV/VertexSwitch/releases/tag/v0.2.0".into()),
                newer: true,
            }
        );
    }

    #[tokio::test]
    async fn no_release_yet_and_errors() {
        let server = MockServer::start().await;
        Mock::given(path("/none"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(path("/down"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let none = check(&format!("{}/none", server.uri()), "0.1.0")
            .await
            .unwrap();
        assert_eq!((none.latest, none.newer), (None, false));
        let err = check(&format!("{}/down", server.uri()), "0.1.0")
            .await
            .unwrap_err();
        assert!(err.contains("503"), "{err}");
    }

    #[test]
    fn ignores_links_outside_the_repository() {
        let info = from_release(
            "0.1.0",
            Release {
                tag_name: "v9.9.9".into(),
                html_url: "https://example.com/malware".into(),
            },
        );
        assert_eq!(info.url, None);
    }
}
