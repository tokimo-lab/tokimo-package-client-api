//! QQ Music (QQ音乐) metadata provider.
//!
//! Uses QQ Music's public search API.

use crate::error::ClientError;
use crate::types::{AlbumDetail, AlbumSearchResult, ArtistInfo, MetadataSource};

const QQMUSIC_API_BASE: &str = "https://c.y.qq.com/soso/fcgi-bin";

pub struct QQMusicClient {
    http: reqwest::Client,
}

impl QQMusicClient {
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
        let url = format!("{QQMUSIC_API_BASE}/client_search_cp");
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("w", keyword),
                ("format", "json"),
                ("p", "1"),
                ("n", &limit.to_string()),
                ("cr", "1"), // Chinese results
                ("new_json", "1"),
                ("remoteplace", "txt.yqq.album"),
                ("searchid", "64228457034997984"),
                ("t", "8"), // 8 = album search
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "QQ Music search failed".into(),
            });
        }

        let data: serde_json::Value = resp.json().await?;
        let albums = data["data"]["album"]["list"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        Ok(albums
            .into_iter()
            .map(|a| {
                let artist = a["singer_name"].as_str().unwrap_or("").to_string();
                let album_mid = a["album_mid"].as_str().unwrap_or("");
                let cover_url = if album_mid.is_empty() {
                    None
                } else {
                    Some(format!(
                        "https://y.qq.com/music/photo_new/T002R500x500M000{album_mid}.jpg"
                    ))
                };

                AlbumSearchResult {
                    source: MetadataSource::QQMusic,
                    external_id: a["album_mid"].as_str().unwrap_or("").to_string(),
                    title: a["album_name"].as_str().unwrap_or("").to_string(),
                    artist,
                    year: a["public_time"]
                        .as_str()
                        .and_then(|s| s.parse::<i32>().ok()),
                    track_count: a["total"].as_i64().map(|n| n as i32),
                    cover_url,
                    score: None,
                }
            })
            .collect())
    }

    /// Get album detail by album MID.
    async fn album_detail(&self, album_mid: &str) -> Result<AlbumDetail, ClientError> {
        let url = "https://c.y.qq.com/v8/fcg-bin/fcg_v8_album_detail_cp.fcg".to_string();
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("albummid", album_mid),
                ("format", "json"),
                ("new_json", "1"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "QQ Music album detail failed".into(),
            });
        }

        let data: serde_json::Value = resp.json().await?;
        let album = &data["data"];
        let songs = album["list"].as_array().cloned().unwrap_or_default();

        let artist_name = album["singername"].as_str().unwrap_or("").to_string();
        let cover_url = format!("https://y.qq.com/music/photo_new/T002R500x500M000{album_mid}.jpg");

        let year = album["a_date"]
            .as_str()
            .and_then(|s| s.split('-').next())
            .and_then(|y| y.parse::<i32>().ok());

        let tracks = songs
            .into_iter()
            .enumerate()
            .map(|(i, s)| crate::types::TrackInfo {
                number: (i + 1) as i32,
                title: s["songname"].as_str().unwrap_or("").to_string(),
                duration: s["interval"].as_i64().map(|n| n as i32),
            })
            .collect();

        Ok(AlbumDetail {
            source: MetadataSource::QQMusic,
            external_id: album_mid.to_string(),
            title: album["albumname"].as_str().unwrap_or("").to_string(),
            artist: artist_name.clone(),
            artist_external_id: None,
            year,
            release_date: album["a_date"].as_str().map(String::from),
            album_type: None,
            genres: None,
            total_tracks: album["total"].as_i64().map(|n| n as i32),
            total_discs: None,
            cover_url: Some(cover_url),
            overview: album["desc"].as_str().map(String::from),
            tracks: Some(tracks),
            artist_credits: vec![crate::types::ArtistCreditInfo {
                source: MetadataSource::QQMusic,
                external_id: album["singerid"].as_i64().unwrap_or(0).to_string(),
                name: artist_name,
            }],
        })
    }
}

impl Default for QQMusicClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::MusicMetadataProvider for QQMusicClient {
    fn source(&self) -> MetadataSource {
        MetadataSource::QQMusic
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
        // QQ Music artist API
        let url = "https://c.y.qq.com/v8/fcg-bin/fcg_v8_singer_track_cp.fcg".to_string();
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("singerid", external_id),
                ("format", "json"),
                ("new_json", "1"),
                ("order", "listen"),
                ("begin", "0"),
                ("num", "1"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "QQ Music artist detail failed".into(),
            });
        }

        let data: serde_json::Value = resp.json().await?;
        let singer = &data["data"];

        Ok(ArtistInfo {
            source: MetadataSource::QQMusic,
            external_id: external_id.to_string(),
            name: singer["singer_name"].as_str().unwrap_or("").to_string(),
            profile_url: None,
            biography: singer["SingerDesc"].as_str().map(String::from),
            genres: None,
        })
    }
}
