use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;

const DEFAULT_CACHE_TTL: Duration = Duration::from_mins(30);
const TOKEN_REFRESH_BUFFER: Duration = Duration::from_mins(5);

pub struct ThetvdbConfig {
    pub api_key: String,
    pub pin: Option<String>,
    pub base_url: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThetvdbMedia {
    pub id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<String>,
    pub overview: Option<String>,
    #[serde(rename = "type")]
    pub media_type: String,
    pub poster_url: Option<String>,
    pub tvdb_id: Option<String>,
    pub imdb_id: Option<String>,
    pub rating: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThetvdbSeriesExtended {
    pub id: i64,
    pub name: String,
    pub slug: Option<String>,
    pub image: Option<String>,
    pub overview: Option<String>,
    #[serde(rename = "firstAired")]
    pub first_aired: Option<String>,
    pub year: Option<String>,
    pub status: Option<ThetvdbStatus>,
    #[serde(rename = "averageRuntime")]
    pub average_runtime: Option<i32>,
    pub rating: Option<f64>,
    #[serde(rename = "remoteIds")]
    pub remote_ids: Option<Vec<ThetvdbRemoteId>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThetvdbStatus {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThetvdbRemoteId {
    pub id: String,
    #[serde(rename = "sourceName")]
    pub source_name: String,
}

// ---- Internal types ----

#[derive(Serialize)]
struct LoginBody {
    apikey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pin: Option<String>,
}

#[derive(Deserialize)]
struct LoginResponse {
    status: Option<String>,
    data: Option<LoginData>,
}

#[derive(Deserialize)]
struct LoginData {
    token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SearchResponse {
    data: Option<Vec<SearchResult>>,
}

#[derive(Serialize, Deserialize)]
struct SearchResult {
    #[serde(rename = "objectID")]
    object_id: Option<String>,
    name: Option<String>,
    image_url: Option<String>,
    #[serde(rename = "type")]
    result_type: Option<String>,
    tvdb_id: Option<String>,
    year: Option<String>,
    overview: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SeriesResponse {
    data: Option<ThetvdbSeriesExtended>,
}

pub struct ThetvdbClient {
    api_key: String,
    pin: Option<String>,
    base_url: String,
    http: reqwest::Client,
    cache: RequestCache,
    token: tokio::sync::RwLock<Option<TokenState>>,
}

struct TokenState {
    token: String,
    expires_at: std::time::Instant,
}

impl ThetvdbClient {
    pub fn new(config: ThetvdbConfig) -> Self {
        Self {
            api_key: config.api_key,
            pin: config.pin,
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://api4.thetvdb.com/v4".to_string()),
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
            token: tokio::sync::RwLock::new(None),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    async fn get_token(&self) -> Result<String, ClientError> {
        {
            let guard = self.token.read().await;
            if let Some(ref state) = *guard
                && state
                    .expires_at
                    .checked_sub(TOKEN_REFRESH_BUFFER)
                    .is_some_and(|deadline| std::time::Instant::now() < deadline)
            {
                return Ok(state.token.clone());
            }
        }

        let body = LoginBody {
            apikey: self.api_key.clone(),
            pin: self.pin.clone(),
        };

        let resp = self
            .http
            .post(format!("{}/login", self.base_url))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Auth(format!("TheTVDB login failed: {}", resp.status())));
        }

        let data: LoginResponse = resp.json().await?;
        if data.status.as_deref() != Some("success") {
            return Err(ClientError::Auth("TheTVDB login: invalid response".into()));
        }

        let token = data
            .data
            .and_then(|d| d.token)
            .ok_or_else(|| ClientError::Auth("TheTVDB login: no token".into()))?;

        let mut guard = self.token.write().await;
        *guard = Some(TokenState {
            token: token.clone(),
            expires_at: std::time::Instant::now() + Duration::from_hours(24),
        });

        Ok(token)
    }

    async fn authed_get<T: serde::de::DeserializeOwned + serde::Serialize>(
        &self,
        cache_key: &str,
        url: &str,
    ) -> Result<T, ClientError> {
        if let Some(cached) = self.cache.get::<T>(cache_key).await {
            return Ok(cached);
        }

        let token = self.get_token().await?;
        let resp = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let data: T = resp.json().await?;
        self.cache.set(cache_key, &data).await;
        Ok(data)
    }

    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<ThetvdbMedia>, ClientError> {
        let cache_key = format!("thetvdb:search:{keyword}:{limit}");
        let encoded = url::form_urlencoded::byte_serialize(keyword.as_bytes()).collect::<String>();
        let url = format!("{}/search?query={encoded}&limit={limit}", self.base_url);

        let data: SearchResponse = self.authed_get(&cache_key, &url).await?;
        let results = data.data.unwrap_or_default();

        Ok(results
            .into_iter()
            .map(|item| {
                let media_type = match item.result_type.as_deref() {
                    Some("movie") => "movie",
                    Some("series") => "series",
                    _ => "other",
                };
                ThetvdbMedia {
                    id: item.tvdb_id.clone().or(item.object_id.clone()).unwrap_or_default(),
                    title: item.name.unwrap_or_default(),
                    original_title: None,
                    year: item.year,
                    overview: item.overview,
                    media_type: media_type.to_string(),
                    poster_url: item.image_url,
                    tvdb_id: item.tvdb_id.or(item.object_id),
                    imdb_id: None,
                    rating: None,
                }
            })
            .collect())
    }

    pub async fn get_series(&self, id: i64) -> Result<Option<ThetvdbSeriesExtended>, ClientError> {
        let cache_key = format!("thetvdb:series:{id}");
        let url = format!("{}/series/{id}/extended", self.base_url);

        let data: SeriesResponse = self.authed_get(&cache_key, &url).await?;
        Ok(data.data)
    }

    pub async fn test_connection(&self) -> Result<TestConnectionResult, ClientError> {
        self.get_token().await?;
        let results = self.search("Doctor Who", 1).await?;
        Ok(TestConnectionResult {
            success: true,
            sample_title: results.first().map(|r| r.title.clone()),
            error_message: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TestConnectionResult {
    pub success: bool,
    pub sample_title: Option<String>,
    pub error_message: Option<String>,
}
