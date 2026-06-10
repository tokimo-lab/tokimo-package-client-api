//! Provider selector — chooses the best search result based on context.

use crate::types::{AlbumSearchResult, MetadataSource};

/// Strategy for selecting the best metadata match.
///
/// Uses library configuration (country, language, source priority)
/// and match quality signals (title similarity, artist match, track count)
/// to pick the best result from multiple providers.
pub struct ProviderSelector {
    /// Library country (e.g., "CN", "US", "JP").
    pub country: Option<String>,
    /// Library language preference (e.g., "zh", "en", "ja").
    pub language: Option<String>,
    /// Ordered list of preferred metadata sources.
    pub source_priority: Vec<MetadataSource>,
}

impl ProviderSelector {
    pub fn new() -> Self {
        Self {
            country: None,
            language: None,
            source_priority: vec![
                MetadataSource::Netease,
                MetadataSource::QQMusic,
                MetadataSource::MusicBrainz,
                MetadataSource::LastFm,
                MetadataSource::Spotify,
            ],
        }
    }

    /// Create a selector for Chinese music.
    pub fn chinese() -> Self {
        Self {
            country: Some("CN".to_string()),
            language: Some("zh".to_string()),
            source_priority: vec![
                MetadataSource::Netease,
                MetadataSource::QQMusic,
                MetadataSource::MusicBrainz,
                MetadataSource::LastFm,
            ],
        }
    }

    /// Create a selector for Western music.
    pub fn western() -> Self {
        Self {
            country: Some("US".to_string()),
            language: Some("en".to_string()),
            source_priority: vec![
                MetadataSource::MusicBrainz,
                MetadataSource::LastFm,
                MetadataSource::Spotify,
                MetadataSource::Netease,
            ],
        }
    }

    /// Select the best match from a list of candidates.
    ///
    /// Scoring:
    /// - Source priority: higher priority sources get a bonus
    /// - Title similarity: exact match > contains > fuzzy
    /// - Artist match: exact match > contains
    /// - Track count: matching track count gets a bonus
    pub fn select_best(
        &self,
        candidates: &[AlbumSearchResult],
        artist: &str,
        album: &str,
        track_count: Option<i32>,
    ) -> Option<AlbumSearchResult> {
        if candidates.is_empty() {
            return None;
        }

        let artist_norm = normalize(artist);
        let album_norm = normalize(album);

        let scored: Vec<(usize, &AlbumSearchResult, i32)> = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut score = c.score.unwrap_or(0);

                // Source priority bonus (higher priority = lower index = higher bonus)
                let source_idx = self.source_priority.iter().position(|s| *s == c.source);
                let source_bonus = match source_idx {
                    Some(idx) => 100 - (idx as i32 * 20), // 100, 80, 60, 40, 20
                    None => 0,
                };
                score += source_bonus;

                // Title similarity
                let c_title_norm = normalize(&c.title);
                if c_title_norm == album_norm {
                    score += 200;
                } else if c_title_norm.contains(&album_norm) || album_norm.contains(&c_title_norm) {
                    score += 100;
                }

                // Artist match
                let c_artist_norm = normalize(&c.artist);
                if c_artist_norm == artist_norm {
                    score += 150;
                } else if c_artist_norm.contains(&artist_norm) || artist_norm.contains(&c_artist_norm) {
                    score += 50;
                }

                // Track count match
                if let (Some(tc), Some(c_tc)) = (track_count, c.track_count)
                    && (tc - c_tc).abs() <= 1
                {
                    score += 50;
                }

                (i, c, score)
            })
            .collect();

        // Find the best scoring candidate
        scored
            .iter()
            .max_by_key(|(_, _, score)| *score)
            .filter(|(_, _, score)| *score >= 50) // Minimum confidence threshold
            .map(|(_, c, _)| (*c).clone())
    }

    /// Auto-detect the best selector based on artist/album text.
    ///
    /// If the text contains CJK characters, use Chinese strategy.
    /// Otherwise, use Western strategy.
    pub fn auto_detect(artist: &str, album: &str) -> Self {
        let has_cjk = artist.chars().any(is_cjk) || album.chars().any(is_cjk);
        if has_cjk { Self::chinese() } else { Self::western() }
    }
}

impl Default for ProviderSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize a string for fuzzy matching: lowercase, alphanumeric only.
fn normalize(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next()
            } else {
                None
            }
        })
        .collect()
}

/// Check if a character is CJK (Chinese, Japanese, Korean).
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4e00}'..='\u{9fff}' |  // CJK Unified Ideographs
        '\u{3400}'..='\u{4dbf}' |  // CJK Unified Ideographs Extension A
        '\u{f900}'..='\u{faff}' |  // CJK Compatibility Ideographs
        '\u{3000}'..='\u{303f}' |  // CJK Symbols and Punctuation
        '\u{3040}'..='\u{309f}' |  // Hiragana
        '\u{30a0}'..='\u{30ff}' |  // Katakana
        '\u{ac00}'..='\u{d7af}'    // Hangul Syllables
    )
}
