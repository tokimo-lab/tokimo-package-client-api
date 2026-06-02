use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;

const DEFAULT_CACHE_TTL: Duration = Duration::from_hours(1);

pub struct FanartConfig {
    pub api_key: String,
    pub client_key: Option<String>,
    pub base_url: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanartImage {
    pub id: String,
    pub url: String,
    pub lang: String,
    pub likes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanartSeasonImage {
    pub id: String,
    pub url: String,
    pub lang: String,
    pub likes: String,
    pub season: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FanartMovieImages {
    pub name: Option<String>,
    pub tmdb_id: Option<String>,
    pub imdb_id: Option<String>,
    pub movieposter: Option<Vec<FanartImage>>,
    pub moviethumb: Option<Vec<FanartImage>>,
    pub moviebackground: Option<Vec<FanartImage>>,
    pub moviebanner: Option<Vec<FanartImage>>,
    pub movielogo: Option<Vec<FanartImage>>,
    pub hdmovielogo: Option<Vec<FanartImage>>,
    pub hdmovieclearart: Option<Vec<FanartImage>>,
    pub movieart: Option<Vec<FanartImage>>,
    pub moviedisc: Option<Vec<FanartImage>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FanartTvImages {
    pub name: Option<String>,
    pub thetvdb_id: Option<String>,
    pub tvposter: Option<Vec<FanartImage>>,
    pub seasonposter: Option<Vec<FanartSeasonImage>>,
    pub tvthumb: Option<Vec<FanartImage>>,
    pub tvbanner: Option<Vec<FanartImage>>,
    pub showbackground: Option<Vec<FanartImage>>,
    pub tvlogo: Option<Vec<FanartImage>>,
    pub hdtvlogo: Option<Vec<FanartImage>>,
    pub hdclearart: Option<Vec<FanartImage>>,
    pub clearart: Option<Vec<FanartImage>>,
    pub characterart: Option<Vec<FanartImage>>,
    pub clearlogo: Option<Vec<FanartImage>>,
}

pub struct FanartClient {
    api_key: String,
    client_key: Option<String>,
    base_url: String,
    http: reqwest::Client,
    cache: RequestCache,
}

impl FanartClient {
    pub fn new(config: FanartConfig) -> Self {
        Self {
            api_key: config.api_key,
            client_key: config.client_key,
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://webservice.fanart.tv/v3".to_string()),
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    async fn request<T: serde::de::DeserializeOwned + serde::Serialize>(
        &self,
        path: &str,
    ) -> Result<T, ClientError> {
        let mut url = url::Url::parse(&format!("{}{}", self.base_url, path))
            .map_err(|e| ClientError::Other(e.to_string()))?;

        url.query_pairs_mut().append_pair("api_key", &self.api_key);
        if let Some(ref ck) = self.client_key {
            url.query_pairs_mut().append_pair("client_key", ck);
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

    pub async fn get_movie_images(&self, tmdb_id: i64) -> Result<FanartMovieImages, ClientError> {
        self.request(&format!("/movies/{tmdb_id}")).await
    }

    pub async fn get_tv_images(&self, tvdb_id: i64) -> Result<FanartTvImages, ClientError> {
        self.request(&format!("/tv/{tvdb_id}")).await
    }

    pub fn get_best_movie_poster(images: &FanartMovieImages, lang: &str) -> Option<String> {
        pick_best(
            images.movieposter.as_deref().unwrap_or_default(),
            Some(lang),
        )
        .map(|img| img.url.clone())
    }

    pub fn get_best_movie_background(images: &FanartMovieImages) -> Option<String> {
        pick_best(images.moviebackground.as_deref().unwrap_or_default(), None)
            .map(|img| img.url.clone())
    }

    pub fn get_best_movie_logo(images: &FanartMovieImages, lang: &str) -> Option<String> {
        let mut all: Vec<&FanartImage> = Vec::new();
        if let Some(ref hd) = images.hdmovielogo {
            all.extend(hd.iter());
        }
        if let Some(ref std) = images.movielogo {
            all.extend(std.iter());
        }
        pick_best(&all.into_iter().cloned().collect::<Vec<_>>(), Some(lang))
            .map(|img| img.url.clone())
    }

    pub fn get_best_tv_poster(images: &FanartTvImages, lang: &str) -> Option<String> {
        pick_best(images.tvposter.as_deref().unwrap_or_default(), Some(lang))
            .map(|img| img.url.clone())
    }

    pub fn get_best_tv_background(images: &FanartTvImages) -> Option<String> {
        pick_best(images.showbackground.as_deref().unwrap_or_default(), None)
            .map(|img| img.url.clone())
    }

    pub fn get_best_tv_logo(images: &FanartTvImages, lang: &str) -> Option<String> {
        let mut all: Vec<&FanartImage> = Vec::new();
        if let Some(ref hd) = images.hdtvlogo {
            all.extend(hd.iter());
        }
        if let Some(ref std) = images.tvlogo {
            all.extend(std.iter());
        }
        pick_best(&all.into_iter().cloned().collect::<Vec<_>>(), Some(lang))
            .map(|img| img.url.clone())
    }

    pub fn get_season_poster(
        images: &FanartTvImages,
        season_number: i32,
        lang: &str,
    ) -> Option<String> {
        let season_str = season_number.to_string();
        let season_posters: Vec<FanartImage> = images
            .seasonposter
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|p| p.season == season_str)
            .map(|p| FanartImage {
                id: p.id.clone(),
                url: p.url.clone(),
                lang: p.lang.clone(),
                likes: p.likes.clone(),
            })
            .collect();
        pick_best(&season_posters, Some(lang)).map(|img| img.url.clone())
    }

    pub async fn test_connection(&self) -> Result<bool, ClientError> {
        let data = self.get_movie_images(27205).await?;
        Ok(data.name.is_some())
    }
}

fn pick_best<'a>(images: &'a [FanartImage], prefer_lang: Option<&str>) -> Option<&'a FanartImage> {
    if images.is_empty() {
        return None;
    }

    let mut sorted: Vec<&FanartImage> = images.iter().collect();
    sorted.sort_by(|a, b| {
        if let Some(lang) = prefer_lang {
            let a_match = if a.lang == lang {
                0
            } else if a.lang == "en" {
                1
            } else {
                2
            };
            let b_match = if b.lang == lang {
                0
            } else if b.lang == "en" {
                1
            } else {
                2
            };
            if a_match != b_match {
                return a_match.cmp(&b_match);
            }
        }
        let a_likes: i32 = a.likes.parse().unwrap_or(0);
        let b_likes: i32 = b.likes.parse().unwrap_or(0);
        b_likes.cmp(&a_likes)
    });

    sorted.first().copied()
}
