use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;

const DEFAULT_CACHE_TTL: Duration = Duration::from_mins(30);

pub struct OmdbConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmdbSearchItem {
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "Year")]
    pub year: String,
    #[serde(rename = "imdbID")]
    pub imdb_id: String,
    #[serde(rename = "Type")]
    pub media_type: String,
    #[serde(rename = "Poster")]
    pub poster: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmdbDetail {
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "Year")]
    pub year: String,
    #[serde(rename = "Rated")]
    pub rated: Option<String>,
    #[serde(rename = "Released")]
    pub released: Option<String>,
    #[serde(rename = "Runtime")]
    pub runtime: Option<String>,
    #[serde(rename = "Genre")]
    pub genre: Option<String>,
    #[serde(rename = "Director")]
    pub director: Option<String>,
    #[serde(rename = "Writer")]
    pub writer: Option<String>,
    #[serde(rename = "Actors")]
    pub actors: Option<String>,
    #[serde(rename = "Plot")]
    pub plot: Option<String>,
    #[serde(rename = "Language")]
    pub language: Option<String>,
    #[serde(rename = "Country")]
    pub country: Option<String>,
    #[serde(rename = "Awards")]
    pub awards: Option<String>,
    #[serde(rename = "Poster")]
    pub poster: Option<String>,
    #[serde(rename = "Ratings")]
    pub ratings: Option<Vec<OmdbRating>>,
    #[serde(rename = "Metascore")]
    pub metascore: Option<String>,
    #[serde(rename = "imdbRating")]
    pub imdb_rating: Option<String>,
    #[serde(rename = "imdbVotes")]
    pub imdb_votes: Option<String>,
    #[serde(rename = "imdbID")]
    pub imdb_id: String,
    #[serde(rename = "Type")]
    pub media_type: String,
    #[serde(rename = "totalSeasons")]
    pub total_seasons: Option<String>,
    #[serde(rename = "Response")]
    pub response: String,
    #[serde(rename = "Error")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmdbRating {
    #[serde(rename = "Source")]
    pub source: String,
    #[serde(rename = "Value")]
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmdbSeasonDetail {
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "Season")]
    pub season: String,
    #[serde(rename = "totalSeasons")]
    pub total_seasons: String,
    #[serde(rename = "Episodes")]
    pub episodes: Vec<OmdbEpisode>,
    #[serde(rename = "Response")]
    pub response: String,
    #[serde(rename = "Error")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmdbEpisode {
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "Released")]
    pub released: String,
    #[serde(rename = "Episode")]
    pub episode: String,
    #[serde(rename = "imdbRating")]
    pub imdb_rating: String,
    #[serde(rename = "imdbID")]
    pub imdb_id: String,
}

#[derive(Serialize, Deserialize)]
struct OmdbSearchResponse {
    #[serde(rename = "Search")]
    search: Option<Vec<OmdbSearchItem>>,
    #[serde(rename = "Response")]
    response: String,
}

pub struct OmdbClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
    cache: RequestCache,
}

impl OmdbClient {
    pub fn new(config: OmdbConfig) -> Self {
        Self {
            api_key: config.api_key,
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://www.omdbapi.com".to_string()),
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    pub async fn cache_size(&self) -> usize {
        self.cache.size().await
    }

    async fn request<T: serde::de::DeserializeOwned + serde::Serialize>(
        &self,
        params: &[(&str, &str)],
    ) -> Result<T, ClientError> {
        let mut url =
            url::Url::parse(&self.base_url).map_err(|e| ClientError::Other(e.to_string()))?;

        url.query_pairs_mut().append_pair("apikey", &self.api_key);
        for (k, v) in params {
            url.query_pairs_mut().append_pair(k, v);
        }

        let cache_key = url.to_string();
        if let Some(cached) = self.cache.get::<T>(&cache_key).await {
            return Ok(cached);
        }

        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let data: T = resp.json().await?;
        self.cache.set(&cache_key, &data).await;
        Ok(data)
    }

    /// Search by title (supports movie/series filter).
    pub async fn search(
        &self,
        query: &str,
        media_type: Option<&str>,
    ) -> Result<Vec<OmdbSearchItem>, ClientError> {
        let mut params = vec![("s", query)];
        if let Some(t) = media_type {
            params.push(("type", t));
        }

        let data: OmdbSearchResponse = self.request(&params).await?;
        if data.response == "False" {
            return Ok(vec![]);
        }

        Ok(data
            .search
            .unwrap_or_default()
            .into_iter()
            .filter(|item| item.media_type == "movie" || item.media_type == "series")
            .collect())
    }

    /// Get detail by `IMDb` ID.
    pub async fn get_detail(&self, imdb_id: &str) -> Result<Option<OmdbDetail>, ClientError> {
        let data: OmdbDetail = self.request(&[("i", imdb_id)]).await?;
        if data.response == "False" {
            return Ok(None);
        }
        Ok(Some(data))
    }

    /// Get season episodes by `IMDb` ID.
    pub async fn get_season_detail(
        &self,
        imdb_id: &str,
        season: u32,
    ) -> Result<Option<OmdbSeasonDetail>, ClientError> {
        let season_str = season.to_string();
        let data: OmdbSeasonDetail = self
            .request(&[("i", imdb_id), ("Season", &season_str)])
            .await?;
        if data.response == "False" {
            return Ok(None);
        }
        Ok(Some(data))
    }
}
