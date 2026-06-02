use serde::Deserialize;

use crate::error::ClientError;
use crate::types::LyricsResult;

const LRCLIB_BASE_URL: &str = "https://lrclib.net/api";

#[derive(Deserialize)]
struct LrclibResponse {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    instrumental: bool,
}

fn to_lyrics_result(data: LrclibResponse) -> LyricsResult {
    LyricsResult {
        synced_lyrics: data.synced_lyrics,
        plain_lyrics: data.plain_lyrics,
        instrumental: data.instrumental,
    }
}

/// Fetch lyrics from LRCLIB.
///
/// Strategy:
/// 1. Try `/api/get` with artist + title + album + duration (exact match)
/// 2. If 404, fall back to `/api/search` with artist + title (lenient match, picks first result)
///
/// Parameters:
/// - `artist`: Artist name
/// - `title`: Track title
/// - `album`: Album name (optional, improves exact-match accuracy)
/// - `duration`: Track duration in seconds (optional, improves exact-match accuracy)
pub async fn fetch_lyrics(
    http: &reqwest::Client,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<u32>,
) -> Result<Option<LyricsResult>, ClientError> {
    // Step 1: exact match via /api/get
    let mut get_url = url::Url::parse(&format!("{LRCLIB_BASE_URL}/get"))
        .map_err(|e| ClientError::Other(e.to_string()))?;
    get_url
        .query_pairs_mut()
        .append_pair("artist_name", artist)
        .append_pair("track_name", title);
    if let Some(album) = album {
        get_url.query_pairs_mut().append_pair("album_name", album);
    }
    if let Some(dur) = duration {
        get_url
            .query_pairs_mut()
            .append_pair("duration", &dur.to_string());
    }

    let resp = http
        .get(get_url)
        .header("User-Agent", "tokimo/1.0")
        .send()
        .await?;

    if resp.status().is_success() {
        let data: LrclibResponse = resp.json().await?;
        return Ok(Some(to_lyrics_result(data)));
    }

    if resp.status().as_u16() != 404 {
        return Err(ClientError::Api {
            status: resp.status().as_u16(),
            message: resp.text().await.unwrap_or_default(),
        });
    }

    // Step 2: fallback to /api/search (lenient, no duration constraint)
    let mut search_url = url::Url::parse(&format!("{LRCLIB_BASE_URL}/search"))
        .map_err(|e| ClientError::Other(e.to_string()))?;
    search_url
        .query_pairs_mut()
        .append_pair("artist_name", artist)
        .append_pair("track_name", title);

    let search_resp = http
        .get(search_url)
        .header("User-Agent", "tokimo/1.0")
        .send()
        .await?;

    if !search_resp.status().is_success() {
        return Ok(None);
    }

    let results: Vec<LrclibResponse> = search_resp.json().await.unwrap_or_default();
    Ok(results.into_iter().next().map(to_lyrics_result))
}
