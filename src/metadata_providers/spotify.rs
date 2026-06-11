use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;

const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const SPOTIFY_API_URL: &str = "https://api.spotify.com/v1";
const DEFAULT_CACHE_TTL: Duration = Duration::from_mins(30);

pub struct SpotifyConfig {
    pub client_id: String,
    pub client_secret: String,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

/// Simplified artist reference from a Spotify album object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyArtistRef {
    pub name: String,
    pub spotify_id: String,
}

/// Track entry from a Spotify full album object (`GET /albums/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyAlbumTrack {
    pub disc: i32,
    pub number: i32,
    pub title: String,
    /// Duration in milliseconds.
    pub duration_ms: Option<i32>,
    pub spotify_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyAlbumSearchResult {
    pub spotify_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub release_date: Option<String>,
    pub total_tracks: Option<i32>,
    pub album_type: Option<String>,
    pub cover_url: Option<String>,
    pub genres: Option<Vec<String>>,
    /// Artist references with Spotify IDs (always populated).
    pub artist_refs: Vec<SpotifyArtistRef>,
    /// Track listing — only populated by `get_album()`, not by search results.
    pub tracks: Option<Vec<SpotifyAlbumTrack>>,
}

struct TokenState {
    access_token: String,
    expires_at: std::time::Instant,
}

pub struct SpotifyClient {
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
    #[allow(dead_code)]
    cache: RequestCache,
    token: tokio::sync::RwLock<Option<TokenState>>,
}

impl SpotifyClient {
    pub fn new(config: SpotifyConfig) -> Self {
        Self {
            client_id: config.client_id,
            client_secret: config.client_secret,
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
            token: tokio::sync::RwLock::new(None),
        }
    }

    async fn get_token(&self) -> Result<String, ClientError> {
        {
            let guard = self.token.read().await;
            if let Some(ref state) = *guard
                && state
                    .expires_at
                    .checked_sub(Duration::from_mins(1))
                    .is_some_and(|deadline| std::time::Instant::now() < deadline)
            {
                return Ok(state.access_token.clone());
            }
        }

        use base64::Engine;
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", self.client_id, self.client_secret));

        let resp = self
            .http
            .post(SPOTIFY_TOKEN_URL)
            .header("Authorization", format!("Basic {credentials}"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Auth(format!("Spotify token error: {}", resp.status())));
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: u64,
        }

        let data: TokenResponse = resp.json().await?;
        let mut guard = self.token.write().await;
        *guard = Some(TokenState {
            access_token: data.access_token.clone(),
            expires_at: std::time::Instant::now() + Duration::from_secs(data.expires_in),
        });

        Ok(data.access_token)
    }

    async fn api_fetch(&self, path: &str, params: &[(&str, &str)]) -> Result<serde_json::Value, ClientError> {
        let token = self.get_token().await?;
        let mut url =
            url::Url::parse(&format!("{SPOTIFY_API_URL}{path}")).map_err(|e| ClientError::Other(e.to_string()))?;
        for (k, v) in params {
            url.query_pairs_mut().append_pair(k, v);
        }
        let resp = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// Search for albums by artist and album name.
    pub async fn search_album(
        &self,
        artist: &str,
        album: &str,
        limit: u32,
    ) -> Result<Vec<SpotifyAlbumSearchResult>, ClientError> {
        let q = format!("album:{album} artist:{artist}");
        let limit_str = limit.to_string();
        let data = self
            .api_fetch("/search", &[("q", &q), ("type", "album"), ("limit", &limit_str)])
            .await?;

        let items = data["albums"]["items"].as_array().cloned().unwrap_or_default();

        Ok(items.into_iter().map(parse_album_item).collect())
    }

    /// Get album detail by Spotify ID.
    pub async fn get_album(&self, spotify_id: &str) -> Result<SpotifyAlbumSearchResult, ClientError> {
        let data = self.api_fetch(&format!("/albums/{spotify_id}"), &[]).await?;
        Ok(parse_album_item(data))
    }

    /// Get genres for an artist (by name search).
    /// Fetch artist image URL by Spotify artist ID (extracted from `MusicBrainz` url-rels).
    pub async fn get_artist_photo_by_id(&self, spotify_id: &str) -> Result<Option<String>, ClientError> {
        let data = self.api_fetch(&format!("/artists/{spotify_id}"), &[]).await?;

        let url = data["images"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|img| img["url"].as_str())
            .map(String::from);

        Ok(url)
    }

    /// Search for an artist by name and return their photo URL.
    pub async fn get_artist_photo(&self, artist_name: &str) -> Result<Option<String>, ClientError> {
        let q = format!("artist:{artist_name}");
        let data = self
            .api_fetch("/search", &[("q", &q), ("type", "artist"), ("limit", "1")])
            .await?;

        let url = data["artists"]["items"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|a| a["images"].as_array())
            .and_then(|imgs| imgs.first())
            .and_then(|img| img["url"].as_str())
            .map(String::from);

        Ok(url)
    }

    /// Search for albums by album name only (no artist filter).
    pub async fn search_album_by_title(
        &self,
        album: &str,
        limit: u32,
    ) -> Result<Vec<SpotifyAlbumSearchResult>, ClientError> {
        let q = format!("album:{album}");
        let limit_str = limit.to_string();
        let data = self
            .api_fetch("/search", &[("q", &q), ("type", "album"), ("limit", &limit_str)])
            .await?;
        let items = data["albums"]["items"].as_array().cloned().unwrap_or_default();
        Ok(items.into_iter().map(parse_album_item).collect())
    }

    pub async fn get_artist_genres(&self, artist_name: &str) -> Result<Vec<String>, ClientError> {
        let q = format!("artist:{artist_name}");
        let data = self
            .api_fetch("/search", &[("q", &q), ("type", "artist"), ("limit", "1")])
            .await?;

        let artists = data["artists"]["items"].as_array();
        if let Some(artists) = artists
            && let Some(artist) = artists.first()
        {
            let genres = artist["genres"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            return Ok(genres);
        }
        Ok(vec![])
    }

    pub async fn test_connection(&self) -> Result<bool, ClientError> {
        self.get_token().await?;
        Ok(true)
    }
}

fn parse_album_item(item: serde_json::Value) -> SpotifyAlbumSearchResult {
    let artists: Vec<String> = item["artists"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let artist_refs: Vec<SpotifyArtistRef> = item["artists"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let name = a["name"].as_str()?.to_string();
                    let spotify_id = a["id"].as_str()?.to_string();
                    Some(SpotifyArtistRef { name, spotify_id })
                })
                .collect()
        })
        .unwrap_or_default();

    let cover_url = item["images"]
        .as_array()
        .and_then(|imgs| imgs.first())
        .and_then(|img| img["url"].as_str())
        .map(String::from);

    let date = item["release_date"].as_str();
    let year = date.and_then(|d| d[..4].parse::<i32>().ok());

    // Tracks — only present in full album objects (GET /albums/{id}), not in search results.
    let tracks: Option<Vec<SpotifyAlbumTrack>> = item["tracks"]["items"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|t| {
                Some(SpotifyAlbumTrack {
                    disc: t["disc_number"].as_i64().unwrap_or(1) as i32,
                    number: t["track_number"].as_i64()? as i32,
                    title: t["name"].as_str()?.to_string(),
                    duration_ms: t["duration_ms"].as_i64().map(|d| d as i32),
                    spotify_id: t["id"].as_str().map(String::from),
                })
            })
            .collect()
    });

    SpotifyAlbumSearchResult {
        spotify_id: item["id"].as_str().unwrap_or_default().to_string(),
        title: item["name"].as_str().unwrap_or_default().to_string(),
        artist: artists.join(", "),
        year,
        release_date: date.map(String::from),
        total_tracks: item["total_tracks"].as_i64().map(|n| n as i32),
        album_type: item["album_type"].as_str().map(String::from),
        cover_url,
        genres: item["genres"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
        artist_refs,
        tracks,
    }
}
