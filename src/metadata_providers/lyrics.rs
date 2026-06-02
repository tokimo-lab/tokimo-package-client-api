//! Multi-source lyrics fetching.
//!
//! Tries multiple providers in order: LrcLib → QQ Music → Netease.
//! Returns the first successful result.

use crate::error::ClientError;
use crate::types::LyricsResult;

/// Fetch lyrics from multiple sources, returning the first successful result.
///
/// Priority: LrcLib (best synced lyrics) → QQ Music (good Chinese coverage) → Netease
pub async fn fetch_lyrics_multi(
    http: &reqwest::Client,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<u32>,
) -> Result<Option<LyricsResult>, ClientError> {
    // 1. Try LrcLib (best for synced lyrics)
    match super::lrclib::fetch_lyrics(http, artist, title, album, duration).await {
        Ok(Some(l)) if l.synced_lyrics.is_some() || l.plain_lyrics.is_some() => {
            return Ok(Some(l));
        }
        _ => {}
    }

    // 2. Try QQ Music (excellent Chinese coverage)
    if let Ok(Some(l)) = fetch_qqmusic_lyrics(http, artist, title).await {
        return Ok(Some(l));
    }

    // 3. Try LrcLib without album (fallback)
    match super::lrclib::fetch_lyrics(http, artist, title, None, duration).await {
        Ok(Some(l)) if l.synced_lyrics.is_some() || l.plain_lyrics.is_some() => {
            return Ok(Some(l));
        }
        _ => {}
    }

    Ok(None)
}

/// Fetch lyrics from QQ Music.
///
/// Flow: search for song → get lyrics by songmid.
async fn fetch_qqmusic_lyrics(
    http: &reqwest::Client,
    artist: &str,
    title: &str,
) -> Result<Option<LyricsResult>, ClientError> {
    // Step 1: Search for the song
    let search_url = "https://c.y.qq.com/soso/fcgi-bin/client_search_cp";
    let query = format!("{artist} {title}");
    let search_resp = http
        .get(search_url)
        .query(&[
            ("w", query.as_str()),
            ("format", "json"),
            ("p", "1"),
            ("n", "1"),
            ("t", "0"),
        ])
        .send()
        .await?;

    if !search_resp.status().is_success() {
        return Ok(None);
    }

    let search_data: serde_json::Value = search_resp.json().await?;
    let song_mid = search_data["data"]["song"]["list"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|song| song["songmid"].as_str());

    let Some(mid) = song_mid else {
        return Ok(None);
    };

    // Step 2: Fetch lyrics
    let lyrics_url = "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg";
    let lyrics_resp = http
        .get(lyrics_url)
        .header("Referer", "https://y.qq.com/")
        .query(&[("songmid", mid), ("format", "json"), ("nobase64", "1")])
        .send()
        .await?;

    if !lyrics_resp.status().is_success() {
        return Ok(None);
    }

    let lyrics_data: serde_json::Value = lyrics_resp.json().await?;
    let lyric_raw = lyrics_data["lyric"].as_str().unwrap_or("");

    if lyric_raw.is_empty() {
        return Ok(None);
    }

    // Parse LRC format lyrics
    let has_timestamps = lyric_raw.contains('[') && lyric_raw.contains(']');
    let synced = if has_timestamps {
        Some(lyric_raw.to_string())
    } else {
        None
    };
    let plain = if has_timestamps {
        // Extract plain text from LRC
        Some(
            lyric_raw
                .lines()
                .filter_map(|line| {
                    if let Some(end) = line.find(']') {
                        let text = &line[end + 1..];
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(text.trim().to_string())
                        }
                    } else {
                        Some(line.to_string())
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    } else {
        Some(lyric_raw.to_string())
    };

    Ok(Some(LyricsResult {
        synced_lyrics: synced,
        plain_lyrics: plain,
        instrumental: false,
    }))
}
