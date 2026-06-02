use reqwest::header::ACCEPT;

use crate::error::ClientError;

pub const ASSRT_BASE_URL: &str = "https://assrt.net";
pub const ASSRT_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

pub fn absolute_url(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!(
            "{}{}{}",
            ASSRT_BASE_URL,
            if path.starts_with('/') { "" } else { "/" },
            path
        )
    }
}

pub struct AssrtConfig {
    pub http_client: reqwest::Client,
}

pub struct AssrtClient {
    http: reqwest::Client,
}

impl AssrtClient {
    pub fn new(config: AssrtConfig) -> Self {
        Self {
            http: config.http_client,
        }
    }

    /// Fetch an HTML page from ASSRT with proper headers.
    pub async fn fetch_html(&self, url: &str) -> Result<String, ClientError> {
        let response = self
            .http
            .get(url)
            .header(
                ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(ClientError::Api {
                status: response.status().as_u16(),
                message: format!("assrt request failed: {}", response.status().as_u16()),
            });
        }
        response
            .text()
            .await
            .map_err(|e| ClientError::Other(format!("Failed to read assrt response: {e}")))
    }

    /// Download an archive from ASSRT with proper Referer header.
    pub async fn download_archive(&self, download_url: &str) -> Result<bytes::Bytes, ClientError> {
        let response = self
            .http
            .get(download_url)
            .header(ACCEPT, "*/*")
            .header(reqwest::header::REFERER, ASSRT_BASE_URL)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(ClientError::Api {
                status: response.status().as_u16(),
                message: format!("assrt request failed: {}", response.status().as_u16()),
            });
        }
        Ok(response.bytes().await?)
    }
}
