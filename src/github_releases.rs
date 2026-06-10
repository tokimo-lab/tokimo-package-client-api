use serde::Deserialize;

use crate::error::ClientError;

#[derive(Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
}

pub struct GithubReleasesConfig {
    pub http_client: reqwest::Client,
    pub user_agent: Option<String>,
}

pub struct GithubReleasesClient {
    http: reqwest::Client,
    user_agent: String,
}

impl GithubReleasesClient {
    pub fn new(config: GithubReleasesConfig) -> Self {
        Self {
            http: config.http_client,
            user_agent: config.user_agent.unwrap_or_else(|| "rust-client-api/1.0".into()),
        }
    }

    /// Fetch the latest release from a GitHub repo. `repo` format: "owner/repo"
    pub async fn get_latest_release(&self, repo: &str) -> Result<GithubRelease, ClientError> {
        let url = format!("https://api.github.com/repos/{repo}/releases/latest");
        let resp = self
            .http
            .get(&url)
            .header("User-Agent", &self.user_agent)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: format!("GitHub API returned {}", resp.status()),
            });
        }
        Ok(resp.json::<GithubRelease>().await?)
    }

    /// Download a release asset. Returns raw bytes.
    pub async fn download_release_asset(
        &self,
        repo: &str,
        tag: &str,
        asset: &str,
    ) -> Result<bytes::Bytes, ClientError> {
        let url = format!("https://github.com/{repo}/releases/download/{tag}/{asset}");
        let resp = self
            .http
            .get(&url)
            .header("User-Agent", &self.user_agent)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: format!("Download returned HTTP {}", resp.status()),
            });
        }
        Ok(resp.bytes().await?)
    }
}
