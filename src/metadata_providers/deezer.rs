use crate::error::ClientError;

const DEEZER_API: &str = "https://api.deezer.com";

pub struct DeezerClient {
    http: reqwest::Client,
}

impl DeezerClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    /// Search Deezer for an artist by name and return their best-quality photo URL.
    /// Uses Deezer's free public API — no API key required.
    pub async fn get_artist_photo(&self, name: &str) -> Result<Option<String>, ClientError> {
        let url = format!("{DEEZER_API}/search/artist");
        let resp = self.http.get(&url).query(&[("q", name), ("limit", "1")]).send().await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let data: serde_json::Value = resp.json().await?;

        let photo = data["data"]
            .as_array()
            .and_then(|arr: &Vec<serde_json::Value>| arr.first())
            .and_then(|artist: &serde_json::Value| {
                // Prefer xl (1000x1000), fall back to big (250x250)
                artist["picture_xl"].as_str().or_else(|| artist["picture_big"].as_str())
            })
            .map(String::from);

        Ok(photo)
    }
}

impl Default for DeezerClient {
    fn default() -> Self {
        Self::new()
    }
}
