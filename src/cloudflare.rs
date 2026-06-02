//! Cloudflare bypass —— 实际实现在 `tokimo-web-fetch` crate。
//! 本模块保留薄壳是为了不破坏现有 metadata provider 的 API 形态
//! （`FetchHtmlResult { status, body }` + `ClientError` 错误类型）。

use crate::error::ClientError;

pub use tokimo_web_fetch::is_under_challenge;

/// Result of an HTML fetch that may encounter Cloudflare challenges.
pub struct FetchHtmlResult {
    pub status: u16,
    pub body: String,
}

/// Client that handles Cloudflare-protected pages via FlareSolverr or direct fetch.
pub struct CloudflareBypassClient {
    inner: tokimo_web_fetch::CloudflareBypassClient,
}

impl CloudflareBypassClient {
    pub fn new(flaresolverr_url: Option<String>) -> Self {
        Self {
            inner: tokimo_web_fetch::CloudflareBypassClient::new(flaresolverr_url),
        }
    }

    pub async fn fetch_html(
        &self,
        url: &str,
        cookie: Option<&str>,
    ) -> Result<FetchHtmlResult, ClientError> {
        let res = self.inner.fetch_html(url, cookie).await?;
        Ok(FetchHtmlResult {
            status: res.status,
            body: res.body,
        })
    }
}
