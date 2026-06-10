use std::time::Duration;

use serde::Deserialize;

use crate::cache::RequestCache;
use crate::error::ClientError;
use crate::types::AdultMetadata;

const CACHE_TTL: Duration = Duration::from_hours(24);

pub struct TpdbConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

#[derive(Deserialize)]
struct TpdbSearchResponse {
    data: Vec<TpdbScene>,
}

#[derive(Deserialize)]
struct TpdbScene {
    id: i64,
    title: Option<String>,
    date: Option<String>,
    duration: Option<i64>,
    poster: Option<String>,
    background: Option<TpdbBackground>,
    site: Option<TpdbSite>,
    performers: Option<Vec<TpdbPerformer>>,
    tags: Option<Vec<TpdbTag>>,
    external_id: Option<String>,
}

#[derive(Deserialize)]
struct TpdbBackground {
    full: Option<String>,
}

#[derive(Deserialize)]
struct TpdbSite {
    name: Option<String>,
}

#[derive(Deserialize)]
struct TpdbPerformer {
    name: Option<String>,
}

#[derive(Deserialize)]
struct TpdbTag {
    name: Option<String>,
}

pub struct TpdbClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
    cache: RequestCache,
}

impl TpdbClient {
    pub fn new(config: TpdbConfig) -> Self {
        Self {
            api_key: config.api_key,
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://api.theporndb.net".to_string())
                .trim_end_matches('/')
                .to_string(),
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(CACHE_TTL)),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    pub async fn search_by_video_id(&self, video_id: &str) -> Result<Option<AdultMetadata>, ClientError> {
        let cache_key = format!("tpdb:{video_id}");
        if let Some(cached) = self.cache.get::<Option<AdultMetadata>>(&cache_key).await {
            return Ok(cached);
        }

        let result = self.fetch_by_video_id(video_id).await;
        let metadata = result.unwrap_or(None);
        self.cache.set(&cache_key, &metadata).await;
        Ok(metadata)
    }

    async fn fetch_by_video_id(&self, video_id: &str) -> Result<Option<AdultMetadata>, ClientError> {
        let mut url =
            url::Url::parse(&format!("{}/scenes", self.base_url)).map_err(|e| ClientError::Other(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("q", video_id)
            .append_pair("per_page", "5");

        let resp = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let data: TpdbSearchResponse = resp.json().await?;
        if data.data.is_empty() {
            return Ok(None);
        }

        let scene = find_best_match(&data.data, video_id);
        Ok(scene.map(|s| transform_scene(s, video_id)))
    }
}

fn find_best_match<'a>(scenes: &'a [TpdbScene], video_id: &str) -> Option<&'a TpdbScene> {
    let normalized = video_id.to_uppercase().replace(['-', '_', ' '], "");

    // Match on external_id
    for scene in scenes {
        if let Some(ref ext_id) = scene.external_id
            && ext_id.to_uppercase().replace(['-', '_', ' '], "") == normalized
        {
            return Some(scene);
        }
    }

    // Title contains video ID
    for scene in scenes {
        if let Some(ref title) = scene.title
            && title.to_uppercase().replace(['-', '_', ' '], "").contains(&normalized)
        {
            return Some(scene);
        }
    }

    scenes.first()
}

fn transform_scene(scene: &TpdbScene, video_id: &str) -> AdultMetadata {
    AdultMetadata {
        video_id: video_id.to_string(),
        title: scene.title.clone(),
        poster_url: scene
            .poster
            .clone()
            .or_else(|| scene.background.as_ref().and_then(|b| b.full.clone())),
        cover_url: None,
        source_url: Some(format!("https://theporndb.net/scenes/{}", scene.id)),
        actors: scene
            .performers
            .as_ref()
            .map(|ps| ps.iter().filter_map(|p| p.name.clone()).collect()),
        genres: scene
            .tags
            .as_ref()
            .map(|ts| ts.iter().filter_map(|t| t.name.clone()).collect()),
        release_date: scene.date.clone(),
        studio: scene.site.as_ref().and_then(|s| s.name.clone()),
        duration: scene.duration.map(|d| (d / 60) as u32),
        rating: None,
        source: "tpdb".to_string(),
    }
}
