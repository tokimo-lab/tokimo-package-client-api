use serde::Deserialize;

use crate::error::ClientError;

#[derive(Deserialize)]
pub struct NominatimEntry {
    pub lat: String,
    pub lon: String,
    pub name: String,
    #[serde(default)]
    pub address: NominatimAddress,
}

#[derive(Deserialize, Default)]
pub struct NominatimAddress {
    pub city: Option<String>,
    pub town: Option<String>,
    pub village: Option<String>,
    pub municipality: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
}

pub struct NominatimConfig {
    pub http_client: reqwest::Client,
    pub user_agent: Option<String>,
}

pub struct NominatimClient {
    http: reqwest::Client,
    user_agent: String,
}

impl NominatimClient {
    pub fn new(config: NominatimConfig) -> Self {
        Self {
            http: config.http_client,
            user_agent: config.user_agent.unwrap_or_else(|| "rust-client-api/1.0".into()),
        }
    }

    pub async fn search(&self, query: &str, limit: u8, lang: &str) -> Result<Vec<NominatimEntry>, ClientError> {
        let encoded_q = percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC).to_string();
        let encoded_lang = percent_encoding::utf8_percent_encode(lang, percent_encoding::NON_ALPHANUMERIC).to_string();
        let url = format!(
            "https://nominatim.openstreetmap.org/search?q={encoded_q}&format=json&limit={limit}&accept-language={encoded_lang}&addressdetails=1"
        );
        let resp = self
            .http
            .get(&url)
            .header("User-Agent", &self.user_agent)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Api {
                status: status.as_u16(),
                message: body,
            });
        }
        Ok(resp.json::<Vec<NominatimEntry>>().await?)
    }
}
