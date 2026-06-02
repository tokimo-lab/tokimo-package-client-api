use std::sync::OnceLock;
use std::time::Duration;

use crate::error::ClientError;
use crate::types::{ArtistCredit, MusicMatchCandidate, MusicMatchDetail, MusicTrack};

const MB_BASE_URL: &str = "https://musicbrainz.org/ws/2";
const CAA_BASE_URL: &str = "https://coverartarchive.org";
const USER_AGENT: &str = "tokimo/1.0 (https://github.com/tokimo)";
const MIN_INTERVAL: Duration = Duration::from_millis(1200);
const MAX_RETRIES: u32 = 3;

/// Process-level rate limiter shared across all `MusicBrainzClient` instances.
/// MusicBrainz requires at most 1 req/sec regardless of how many concurrent
/// jobs are running — enforcing this here means callers need no coordination.
static RATE_LIMITER: OnceLock<tokio::sync::Mutex<std::time::Instant>> = OnceLock::new();

fn rate_limiter() -> &'static tokio::sync::Mutex<std::time::Instant> {
    RATE_LIMITER.get_or_init(|| {
        tokio::sync::Mutex::new(
            std::time::Instant::now()
                .checked_sub(MIN_INTERVAL)
                .unwrap_or_else(std::time::Instant::now),
        )
    })
}

pub struct MusicBrainzClient {
    http: reqwest::Client,
}

impl MusicBrainzClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .unwrap_or_default(),
        }
    }

    async fn rate_limit(&self) {
        let mut last = rate_limiter().lock().await;
        let elapsed = last.elapsed();
        if elapsed < MIN_INTERVAL {
            tokio::time::sleep(MIN_INTERVAL.checked_sub(elapsed).unwrap()).await;
        }
        *last = std::time::Instant::now();
    }

    async fn mb_fetch(&self, path: &str, params: &[(&str, &str)]) -> Result<serde_json::Value, ClientError> {
        let mut url =
            url::Url::parse(&format!("{MB_BASE_URL}{path}")).map_err(|e| ClientError::Other(e.to_string()))?;

        url.query_pairs_mut().append_pair("fmt", "json");
        for (k, v) in params {
            url.query_pairs_mut().append_pair(k, v);
        }

        for attempt in 0..=MAX_RETRIES {
            self.rate_limit().await;

            let resp = self
                .http
                .get(url.clone())
                .header("Accept", "application/json")
                .send()
                .await?;

            if (resp.status().as_u16() == 503 || resp.status().as_u16() == 429) && attempt < MAX_RETRIES {
                tokio::time::sleep(Duration::from_millis(2000 * 2u64.pow(attempt))).await;
                continue;
            }

            if !resp.status().is_success() {
                return Err(ClientError::Api {
                    status: resp.status().as_u16(),
                    message: resp.text().await.unwrap_or_default(),
                });
            }

            return Ok(resp.json().await?);
        }

        Err(ClientError::Other("MusicBrainz: max retries exceeded".into()))
    }

    /// Search releases by artist and album name.
    pub async fn search_release(
        &self,
        artist: &str,
        album: &str,
        limit: u32,
    ) -> Result<Vec<MusicMatchCandidate>, ClientError> {
        let query = format!("release:\"{album}\" AND artist:\"{artist}\"");
        let limit_str = limit.to_string();
        let data = self
            .mb_fetch("/release", &[("query", &query), ("limit", &limit_str)])
            .await?;

        Ok(parse_release_list(&data))
    }

    /// Search releases by keyword.
    pub async fn search_release_by_keyword(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<MusicMatchCandidate>, ClientError> {
        let limit_str = limit.to_string();
        let data = self
            .mb_fetch("/release", &[("query", keyword), ("limit", &limit_str)])
            .await?;

        Ok(parse_release_list(&data))
    }

    /// Get release detail (tracks, genres, cover art).
    pub async fn get_release(&self, mb_release_id: &str) -> Result<MusicMatchDetail, ClientError> {
        let data = self
            .mb_fetch(
                &format!("/release/{mb_release_id}"),
                &[("inc", "recordings+artist-credits+release-groups+genres+labels")],
            )
            .await?;

        let artist_info = extract_artist_name(&data["artist-credit"]);
        let artist_credits = extract_artist_credits(&data["artist-credit"]);
        let rg = &data["release-group"];
        let date = data["date"].as_str();
        let year = date.and_then(|d| d.get(..4)).and_then(|y| y.parse::<i32>().ok());

        // Genres
        let mut genres: Vec<String> = Vec::new();
        if let Some(rg_genres) = rg["genres"].as_array() {
            for g in rg_genres {
                if let Some(name) = g["name"].as_str() {
                    genres.push(name.to_string());
                }
            }
        }
        if let Some(rel_genres) = data["genres"].as_array() {
            for g in rel_genres {
                if let Some(name) = g["name"].as_str()
                    && !genres.contains(&name.to_string())
                {
                    genres.push(name.to_string());
                }
            }
        }

        // Tracks
        let media = data["media"].as_array();
        let mut total_tracks: i32 = 0;
        let mut total_discs: i32 = 0;
        let mut tracks: Vec<MusicTrack> = Vec::new();

        if let Some(media) = media {
            total_discs = media.len() as i32;
            for disc in media {
                total_tracks += disc["track-count"].as_i64().unwrap_or(0) as i32;
                if let Some(disc_tracks) = disc["tracks"].as_array() {
                    for t in disc_tracks {
                        tracks.push(MusicTrack {
                            number: t["position"].as_i64().unwrap_or(0) as i32,
                            title: t["title"].as_str().unwrap_or("").to_string(),
                            duration: t["length"].as_i64().map(|ms| (ms / 1000) as i32),
                        });
                    }
                }
            }
        }

        let rg_id = rg["id"].as_str();
        let cover_url = rg_id.map(|id| format!("{CAA_BASE_URL}/release-group/{id}/front-500"));

        Ok(MusicMatchDetail {
            mb_release_id: data["id"].as_str().unwrap_or("").to_string(),
            mb_release_group_id: rg_id.map(String::from),
            title: data["title"].as_str().unwrap_or("").to_string(),
            artist: artist_info.0,
            artist_mb_id: artist_info.1,
            year,
            release_date: date.map(String::from),
            album_type: rg["primary-type"].as_str().map(str::to_lowercase),
            genres: if genres.is_empty() { None } else { Some(genres) },
            total_tracks: if total_tracks > 0 { Some(total_tracks) } else { None },
            total_discs: if total_discs > 0 { Some(total_discs) } else { None },
            cover_url,
            overview: None,
            spotify_id: None,
            tracks: if tracks.is_empty() { None } else { Some(tracks) },
            artist_credits,
        })
    }

    /// Search artists.
    pub async fn search_artist(&self, name: &str, limit: u32) -> Result<Vec<ArtistSearchResult>, ClientError> {
        let query = format!("artist:\"{name}\"");
        let limit_str = limit.to_string();
        let data = self
            .mb_fetch("/artist", &[("query", &query), ("limit", &limit_str)])
            .await?;

        let artists = data["artists"].as_array().cloned().unwrap_or_default();
        Ok(artists
            .into_iter()
            .map(|a| ArtistSearchResult {
                mb_id: a["id"].as_str().unwrap_or("").to_string(),
                name: a["name"].as_str().unwrap_or("").to_string(),
                artist_type: a["type"].as_str().map(String::from),
            })
            .collect())
    }

    /// Get Cover Art Archive URL for a release group.
    pub fn get_release_group_cover_url(mb_release_group_id: &str) -> String {
        format!("{CAA_BASE_URL}/release-group/{mb_release_group_id}/front-500")
    }

    /// Fetch artist details: birthday, birthplace, and external URLs (Spotify, Wikipedia, etc).
    pub async fn get_artist_detail(&self, mb_id: &str) -> Result<ArtistDetail, ClientError> {
        // Note: mb_fetch already appends fmt=json — do NOT include it in params
        let data = self
            .mb_fetch(&format!("/artist/{mb_id}"), &[("inc", "url-rels")])
            .await?;

        let birthday = data["life-span"]["begin"].as_str().map(String::from);

        // Get area ID and English name — then try to resolve a Chinese alias
        let area_id = data["begin-area"]["id"]
            .as_str()
            .or_else(|| data["area"]["id"].as_str());
        let area_name_en = data["begin-area"]["name"]
            .as_str()
            .or_else(|| data["area"]["name"].as_str())
            .map(String::from);

        let birthplace = if let Some(area_id) = area_id {
            // Try to get the Chinese (zh) alias from the area endpoint
            match self.get_area_zh_name(area_id).await {
                Some(zh_name) => Some(zh_name),
                None => area_name_en,
            }
        } else {
            area_name_en
        };

        let gender = data["gender"].as_str().map(String::from);

        // Extract Spotify artist ID and Wikipedia URL from url-rels
        let mut spotify_id = None;
        let mut wikipedia_url = None;
        if let Some(rels) = data["relations"].as_array() {
            for rel in rels {
                let rel_type = rel["type"].as_str().unwrap_or("");
                if let Some(url) = rel["url"]["resource"].as_str() {
                    match rel_type {
                        "streaming music" | "free streaming" if url.contains("open.spotify.com/artist/") => {
                            spotify_id = url.split('/').next_back().map(String::from);
                        }
                        "wikipedia" if wikipedia_url.is_none() => {
                            wikipedia_url = Some(url.to_string());
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(ArtistDetail {
            mb_id: mb_id.to_string(),
            name: data["name"].as_str().unwrap_or("").to_string(),
            gender,
            birthday,
            birthplace,
            spotify_id,
            wikipedia_url,
        })
    }

    /// Fetch Chinese (zh) locale alias for a `MusicBrainz` area.
    /// Returns None if no zh alias exists or on any error.
    async fn get_area_zh_name(&self, area_id: &str) -> Option<String> {
        let data = self
            .mb_fetch(&format!("/area/{area_id}"), &[("inc", "aliases")])
            .await
            .ok()?;

        let aliases = data["aliases"].as_array()?;
        // MusicBrainz uses underscores: zh_Hans, zh_Hant (and sometimes zh-Hans / zh-Hant)
        let is_zh = |locale: &str| -> bool {
            matches!(
                locale,
                "zh" | "zh_Hans" | "zh_Hant" | "zh-Hans" | "zh-Hant" | "zh_CN" | "zh_TW" | "zh_HK"
            ) || locale.starts_with("zh_")
                || locale.starts_with("zh-")
        };

        aliases
            .iter()
            .find(|a| a["locale"].as_str().is_some_and(is_zh))
            .and_then(|a| a["name"].as_str())
            .map(String::from)
    }

    pub async fn test_connection(&self) -> Result<bool, ClientError> {
        self.mb_fetch("/release", &[("query", "test"), ("limit", "1")]).await?;
        Ok(true)
    }
}

impl Default for MusicBrainzClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ArtistSearchResult {
    pub mb_id: String,
    pub name: String,
    pub artist_type: Option<String>,
}

/// Artist detail from `MusicBrainz` artist endpoint.
#[derive(Debug, Clone)]
pub struct ArtistDetail {
    pub mb_id: String,
    pub name: String,
    pub gender: Option<String>,
    pub birthday: Option<String>,
    pub birthplace: Option<String>,
    /// Spotify artist ID extracted from url-rels.
    pub spotify_id: Option<String>,
    /// Wikipedia article URL (any language) extracted from url-rels.
    pub wikipedia_url: Option<String>,
}

/// Build display artist name from MB artist-credits (with joinphrases).
fn extract_artist_name(credits: &serde_json::Value) -> (String, Option<String>) {
    let arr = match credits.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return ("Unknown Artist".to_string(), None),
    };

    let mut display = String::new();
    for c in arr {
        let name = c["name"]
            .as_str()
            .or_else(|| c["artist"]["name"].as_str())
            .unwrap_or("");
        let joinphrase = c["joinphrase"].as_str().unwrap_or("");
        display.push_str(name);
        display.push_str(joinphrase);
    }

    let mb_id = arr.first().and_then(|c| c["artist"]["id"].as_str()).map(String::from);

    (display, mb_id)
}

/// Extract individual artist credits from MB artist-credits array.
/// Returns one entry per artist with their name and MB artist ID.
fn extract_artist_credits(credits: &serde_json::Value) -> Vec<ArtistCredit> {
    let Some(arr) = credits.as_array() else { return vec![] };

    arr.iter()
        .filter_map(|c| {
            let name = c["name"]
                .as_str()
                .or_else(|| c["artist"]["name"].as_str())
                .unwrap_or("")
                .to_string();
            let mb_id = c["artist"]["id"].as_str().unwrap_or("").to_string();
            if mb_id.is_empty() {
                None
            } else {
                Some(ArtistCredit { name, mb_id })
            }
        })
        .collect()
}

fn parse_release_list(data: &serde_json::Value) -> Vec<MusicMatchCandidate> {
    let releases = data["releases"].as_array().cloned().unwrap_or_default();
    releases
        .into_iter()
        .map(|r| {
            let artist_info = extract_artist_name(&r["artist-credit"]);
            let date = r["date"].as_str();
            let year = date.and_then(|d| d.get(..4)).and_then(|y| y.parse::<i32>().ok());

            MusicMatchCandidate {
                mb_release_id: r["id"].as_str().unwrap_or("").to_string(),
                title: r["title"].as_str().unwrap_or("").to_string(),
                artist: artist_info.0,
                year,
                track_count: r["track-count"].as_i64().map(|n| n as i32),
                country: r["country"].as_str().map(String::from),
                format: None,
                score: r["score"].as_i64().map(|n| n as i32),
            }
        })
        .collect()
}

// ── MusicMetadataProvider trait implementation ────────────────────────────────

#[async_trait::async_trait]
impl super::MusicMetadataProvider for MusicBrainzClient {
    fn source(&self) -> crate::types::MetadataSource {
        crate::types::MetadataSource::MusicBrainz
    }

    async fn search_albums(
        &self,
        artist: &str,
        album: &str,
        limit: u32,
    ) -> Result<Vec<crate::types::AlbumSearchResult>, ClientError> {
        let candidates = self.search_release(artist, album, limit).await?;
        Ok(candidates
            .into_iter()
            .map(|c| crate::types::AlbumSearchResult {
                source: crate::types::MetadataSource::MusicBrainz,
                external_id: c.mb_release_id,
                title: c.title,
                artist: c.artist,
                year: c.year,
                track_count: c.track_count,
                cover_url: None,
                score: c.score,
            })
            .collect())
    }

    async fn search_albums_by_keyword(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<crate::types::AlbumSearchResult>, ClientError> {
        let candidates = self.search_release_by_keyword(keyword, limit).await?;
        Ok(candidates
            .into_iter()
            .map(|c| crate::types::AlbumSearchResult {
                source: crate::types::MetadataSource::MusicBrainz,
                external_id: c.mb_release_id,
                title: c.title,
                artist: c.artist,
                year: c.year,
                track_count: c.track_count,
                cover_url: None,
                score: c.score,
            })
            .collect())
    }

    async fn get_album_detail(&self, external_id: &str) -> Result<crate::types::AlbumDetail, ClientError> {
        let detail = self.get_release(external_id).await?;
        Ok(crate::types::AlbumDetail {
            source: crate::types::MetadataSource::MusicBrainz,
            external_id: detail.mb_release_id,
            title: detail.title,
            artist: detail.artist,
            artist_external_id: detail.artist_mb_id,
            year: detail.year,
            release_date: detail.release_date,
            album_type: detail.album_type,
            genres: detail.genres,
            total_tracks: detail.total_tracks,
            total_discs: detail.total_discs,
            cover_url: detail.cover_url,
            overview: detail.overview,
            tracks: detail.tracks.map(|ts| {
                ts.into_iter()
                    .map(|t| crate::types::TrackInfo {
                        number: t.number,
                        title: t.title,
                        duration: t.duration,
                    })
                    .collect()
            }),
            artist_credits: detail
                .artist_credits
                .into_iter()
                .map(|ac| crate::types::ArtistCreditInfo {
                    source: crate::types::MetadataSource::MusicBrainz,
                    external_id: ac.mb_id,
                    name: ac.name,
                })
                .collect(),
        })
    }

    async fn get_artist_info(&self, external_id: &str) -> Result<crate::types::ArtistInfo, ClientError> {
        let detail = self.get_artist_detail(external_id).await?;
        Ok(crate::types::ArtistInfo {
            source: crate::types::MetadataSource::MusicBrainz,
            external_id: detail.mb_id,
            name: detail.name,
            profile_url: None,
            biography: None,
            genres: None,
        })
    }
}
