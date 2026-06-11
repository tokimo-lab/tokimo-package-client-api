use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;

const DEFAULT_CACHE_TTL: Duration = Duration::from_hours(1);
const BANGUMI_BASE_URL: &str = "https://api.bgm.tv/v0";
const DEFAULT_USER_AGENT: &str = "tokimo/1.0 (https://github.com/tokimo)";

pub struct BangumiConfig {
    pub access_token: Option<String>,
    pub base_url: Option<String>,
    pub user_agent: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

/// Subject type: 1=Book, 2=Anime, 3=Music, 4=Game, 6=Real.
pub type BangumiSubjectType = u8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiSearchItem {
    pub id: i64,
    #[serde(rename = "type")]
    pub subject_type: BangumiSubjectType,
    pub name: String,
    pub name_cn: String,
    pub summary: Option<String>,
    pub air_date: Option<String>,
    pub date: Option<String>,
    pub platform: Option<String>,
    pub rank: Option<i32>,
    pub eps: Option<i32>,
    pub total_episodes: Option<i32>,
    pub volumes: Option<i32>,
    pub rating: Option<BangumiRating>,
    pub collection: Option<BangumiCollection>,
    pub images: Option<BangumiImages>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiSubjectDetail {
    pub id: i64,
    #[serde(rename = "type")]
    pub subject_type: BangumiSubjectType,
    pub name: String,
    pub name_cn: String,
    pub summary: Option<String>,
    pub air_date: Option<String>,
    pub total_episodes: Option<i32>,
    pub rating: Option<BangumiRating>,
    pub images: Option<BangumiImages>,
    pub tags: Option<Vec<BangumiTag>>,
    pub infobox: Option<Vec<BangumiInfoboxItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiRating {
    pub score: f64,
    pub total: i32,
    pub rank: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiCollection {
    pub doing: Option<i32>,
    pub wish: Option<i32>,
    pub collect: Option<i32>,
    pub on_hold: Option<i32>,
    pub dropped: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiImages {
    pub large: Option<String>,
    pub medium: Option<String>,
    pub small: Option<String>,
    pub grid: Option<String>,
    pub common: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiTag {
    pub name: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiInfoboxItem {
    pub key: String,
    pub value: serde_json::Value,
}

// ---- Calendar types (https://api.bgm.tv/calendar) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiCalendarWeekday {
    pub id: u32,
    pub cn: Option<String>,
    pub en: Option<String>,
    pub ja: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiCalendarItem {
    pub id: i64,
    pub url: Option<String>,
    pub name: Option<String>,
    pub name_cn: Option<String>,
    pub summary: Option<String>,
    pub air_date: Option<String>,
    pub air_weekday: Option<u32>,
    pub images: Option<BangumiImages>,
    pub rating: Option<BangumiRating>,
    pub rank: Option<i32>,
    pub collection: Option<BangumiCollection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiCalendarDay {
    pub weekday: BangumiCalendarWeekday,
    #[serde(default)]
    pub items: Vec<BangumiCalendarItem>,
}

// ---- Internal response types ----

#[derive(Deserialize)]
struct SearchResponse {
    data: Option<Vec<BangumiSearchItem>>,
    list: Option<Vec<BangumiSearchItem>>,
}

#[derive(Serialize)]
struct BrowseBody {
    keyword: String,
    filter: BrowseFilter,
    sort: String,
    limit: u32,
    offset: u32,
}

#[derive(Serialize)]
struct BrowseFilter {
    #[serde(rename = "type")]
    subject_type: Vec<BangumiSubjectType>,
    nsfw: bool,
}

#[derive(Deserialize)]
struct BrowseResponse {
    data: Option<Vec<BangumiSearchItem>>,
}

pub struct BangumiClient {
    access_token: Option<String>,
    base_url: String,
    user_agent: String,
    http: reqwest::Client,
    cache: RequestCache,
}

impl BangumiClient {
    pub fn new(config: BangumiConfig) -> Self {
        Self {
            access_token: config.access_token,
            base_url: config.base_url.unwrap_or_else(|| BANGUMI_BASE_URL.to_string()),
            user_agent: config.user_agent.unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    fn build_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", self.user_agent.parse().expect("valid header value"));
        headers.insert("Content-Type", "application/json".parse().expect("valid header value"));
        if let Some(ref token) = self.access_token {
            headers.insert(
                "Authorization",
                format!("Bearer {token}").parse().expect("valid header value"),
            );
        }
        headers
    }

    /// Search subjects by keyword.
    pub async fn search(
        &self,
        keyword: &str,
        subject_type: BangumiSubjectType,
        limit: u32,
    ) -> Result<Vec<BangumiSearchItem>, ClientError> {
        let cache_key = format!("bangumi:search:{keyword}:{subject_type}:{limit}");
        if let Some(cached) = self.cache.get::<Vec<BangumiSearchItem>>(&cache_key).await {
            return Ok(cached);
        }

        let url = format!(
            "{}/search/subjects?keyword={}&type={subject_type}&limit={limit}",
            self.base_url,
            urlencoding::encode(keyword)
        );

        let resp = self.http.get(&url).headers(self.build_headers()).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let data: SearchResponse = resp.json().await?;
        let items = data.data.or(data.list).unwrap_or_default();
        self.cache.set(&cache_key, &items).await;
        Ok(items)
    }

    /// Browse top subjects by type (POST API with sorting).
    pub async fn browse_subjects(
        &self,
        subject_type: BangumiSubjectType,
        limit: u32,
    ) -> Result<Vec<BangumiSearchItem>, ClientError> {
        let cache_key = format!("bangumi:browse:{subject_type}:{limit}");
        if let Some(cached) = self.cache.get::<Vec<BangumiSearchItem>>(&cache_key).await {
            return Ok(cached);
        }

        let body = BrowseBody {
            keyword: String::new(),
            filter: BrowseFilter {
                subject_type: vec![subject_type],
                nsfw: false,
            },
            sort: "heat".to_string(),
            limit,
            offset: 0,
        };

        let resp = self
            .http
            .post(format!("{}/search/subjects", self.base_url))
            .headers(self.build_headers())
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let data: BrowseResponse = resp.json().await?;
        let items = data.data.unwrap_or_default();
        self.cache.set(&cache_key, &items).await;
        Ok(items)
    }

    /// Get subject detail by ID.
    pub async fn get_subject(&self, id: i64) -> Result<Option<BangumiSubjectDetail>, ClientError> {
        let cache_key = format!("bangumi:subject:{id}");
        if let Some(cached) = self.cache.get::<Option<BangumiSubjectDetail>>(&cache_key).await {
            return Ok(cached);
        }

        let resp = self
            .http
            .get(format!("{}/subjects/{id}", self.base_url))
            .headers(self.build_headers())
            .send()
            .await?;

        if resp.status().as_u16() == 404 {
            self.cache.set(&cache_key, &None::<BangumiSubjectDetail>).await;
            return Ok(None);
        }

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let data: BangumiSubjectDetail = resp.json().await?;
        self.cache.set(&cache_key, &Some(&data)).await;
        Ok(Some(data))
    }

    /// Fetch the weekly anime calendar from `https://api.bgm.tv/calendar`.
    pub async fn get_calendar(&self) -> Result<Vec<BangumiCalendarDay>, ClientError> {
        let cache_key = "bangumi:calendar".to_string();
        if let Some(cached) = self.cache.get::<Vec<BangumiCalendarDay>>(&cache_key).await {
            return Ok(cached);
        }

        // Calendar endpoint is outside /v0, always at api.bgm.tv/calendar
        let url = self.base_url.replace("/v0", "/calendar");

        let resp = self
            .http
            .get(&url)
            .headers(self.build_headers())
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let data: Vec<BangumiCalendarDay> = resp.json().await?;
        self.cache.set(&cache_key, &data).await;
        Ok(data)
    }

    /// Test connection by fetching Neon Genesis Evangelion (ID: 1).
    pub async fn test_connection(&self) -> Result<bool, ClientError> {
        match self.get_subject(1).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) | Err(_) => Ok(false),
        }
    }
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
