//! Netease Cloud Music (网易云音乐) metadata provider.
//!
//! API documentation: https://github.com/Binaryify/NeteaseCloudMusicApi

use crate::error::ClientError;
use crate::types::{AlbumDetail, AlbumSearchResult, ArtistInfo, MetadataSource};

const NETEASE_API_BASE: &str = "https://music.163.com/api";

pub struct NeteaseClient {
    http: reqwest::Client,
}

impl NeteaseClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                .build()
                .unwrap_or_default(),
        }
    }

    /// Search albums by keyword.
    async fn search(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<AlbumSearchResult>, ClientError> {
        let url = format!("{NETEASE_API_BASE}/search/get");
        let resp = self
            .http
            .post(&url)
            .form(&[
                ("s", keyword),
                ("type", "10"), // 10 = album
                ("limit", &limit.to_string()),
                ("offset", "0"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "Netease search failed".into(),
            });
        }

        let data: serde_json::Value = resp.json().await?;
        let albums = data["result"]["albums"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        Ok(albums
            .into_iter()
            .map(|a| {
                let artist = a["artist"]["name"].as_str().unwrap_or("").to_string();
                let cover_url = a["picUrl"].as_str().map(|u| format!("{u}?param=500y500"));

                AlbumSearchResult {
                    source: MetadataSource::Netease,
                    external_id: a["id"].as_i64().unwrap_or(0).to_string(),
                    title: a["name"].as_str().unwrap_or("").to_string(),
                    artist,
                    year: a["publishTime"]
                        .as_i64()
                        .and_then(|ts| {
                            // Timestamp to year
                            let secs = ts / 1000;
                            chrono::DateTime::from_timestamp(secs, 0)
                                .map(|dt| dt.format("%Y").to_string().parse::<i32>().ok())
                        })
                        .flatten(),
                    track_count: a["size"].as_i64().map(|n| n as i32),
                    cover_url,
                    score: None, // Will be scored by selector
                }
            })
            .collect())
    }

    /// Get album detail by ID.
    async fn album_detail(&self, album_id: &str) -> Result<AlbumDetail, ClientError> {
        let url = format!("{NETEASE_API_BASE}/album/{album_id}");
        let resp = self.http.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "Netease album detail failed".into(),
            });
        }

        let data: serde_json::Value = resp.json().await?;
        let album = &data["album"];
        let songs = data["songs"].as_array().cloned().unwrap_or_default();

        let artist_name = album["artist"]["name"].as_str().unwrap_or("").to_string();
        let artist_id = album["artist"]["id"].as_i64().map(|id| id.to_string());
        let cover_url = album["picUrl"]
            .as_str()
            .map(|u| format!("{u}?param=500y500"));

        let year = album["publishTime"]
            .as_i64()
            .and_then(|ts| {
                let secs = ts / 1000;
                chrono::DateTime::from_timestamp(secs, 0)
                    .map(|dt| dt.format("%Y").to_string().parse::<i32>().ok())
            })
            .flatten();

        let tracks = songs
            .into_iter()
            .enumerate()
            .map(|(i, s)| crate::types::TrackInfo {
                number: (i + 1) as i32,
                title: s["name"].as_str().unwrap_or("").to_string(),
                duration: s["dt"].as_i64().map(|ms| (ms / 1000) as i32),
            })
            .collect();

        Ok(AlbumDetail {
            source: MetadataSource::Netease,
            external_id: album_id.to_string(),
            title: album["name"].as_str().unwrap_or("").to_string(),
            artist: artist_name.clone(),
            artist_external_id: artist_id,
            year,
            release_date: album["publishTime"].as_i64().and_then(|ts| {
                let secs = ts / 1000;
                chrono::DateTime::from_timestamp(secs, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
            }),
            album_type: album["type"].as_str().map(String::from),
            genres: None, // Netease doesn't provide genres in album detail
            total_tracks: album["size"].as_i64().map(|n| n as i32),
            total_discs: None,
            cover_url,
            overview: album["description"].as_str().map(String::from),
            tracks: Some(tracks),
            artist_credits: vec![crate::types::ArtistCreditInfo {
                source: MetadataSource::Netease,
                external_id: album["artist"]["id"].as_i64().unwrap_or(0).to_string(),
                name: artist_name,
            }],
        })
    }
}

impl Default for NeteaseClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::MusicMetadataProvider for NeteaseClient {
    fn source(&self) -> MetadataSource {
        MetadataSource::Netease
    }

    async fn search_albums(
        &self,
        artist: &str,
        album: &str,
        limit: u32,
    ) -> Result<Vec<AlbumSearchResult>, ClientError> {
        // Try artist + album first
        let keyword = format!("{artist} {album}");
        let results = self.search(&keyword, limit).await?;

        // If no results, try album only
        if results.is_empty() {
            return self.search(album, limit).await;
        }

        Ok(results)
    }

    async fn search_albums_by_keyword(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<AlbumSearchResult>, ClientError> {
        self.search(keyword, limit).await
    }

    async fn get_album_detail(&self, external_id: &str) -> Result<AlbumDetail, ClientError> {
        self.album_detail(external_id).await
    }

    async fn get_artist_info(&self, external_id: &str) -> Result<ArtistInfo, ClientError> {
        let url = format!("{NETEASE_API_BASE}/artist/{external_id}");
        let resp = self.http.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "Netease artist detail failed".into(),
            });
        }

        let data: serde_json::Value = resp.json().await?;
        let artist = &data["artist"];

        Ok(ArtistInfo {
            source: MetadataSource::Netease,
            external_id: external_id.to_string(),
            name: artist["name"].as_str().unwrap_or("").to_string(),
            profile_url: artist["picUrl"].as_str().map(String::from),
            biography: artist["briefDesc"].as_str().map(String::from),
            genres: None,
        })
    }
}
