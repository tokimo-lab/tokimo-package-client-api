use serde::{Deserialize, Serialize};

// ── Multi-source music metadata types ────────────────────────────────────────

/// Metadata source identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataSource {
    MusicBrainz,
    Netease,
    QQMusic,
    LastFm,
    Spotify,
}

impl std::fmt::Display for MetadataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MusicBrainz => write!(f, "musicbrainz"),
            Self::Netease => write!(f, "netease"),
            Self::QQMusic => write!(f, "qqmusic"),
            Self::LastFm => write!(f, "lastfm"),
            Self::Spotify => write!(f, "spotify"),
        }
    }
}

impl std::str::FromStr for MetadataSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "musicbrainz" => Ok(Self::MusicBrainz),
            "netease" => Ok(Self::Netease),
            "qqmusic" => Ok(Self::QQMusic),
            "lastfm" => Ok(Self::LastFm),
            "spotify" => Ok(Self::Spotify),
            _ => Err(format!("unknown metadata source: {s}")),
        }
    }
}

/// Unified album search result from any metadata source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSearchResult {
    pub source: MetadataSource,
    pub external_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub track_count: Option<i32>,
    pub cover_url: Option<String>,
    /// Match confidence 0-100.
    pub score: Option<i32>,
}

/// Unified album detail from any metadata source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetail {
    pub source: MetadataSource,
    pub external_id: String,
    pub title: String,
    pub artist: String,
    pub artist_external_id: Option<String>,
    pub year: Option<i32>,
    pub release_date: Option<String>,
    pub album_type: Option<String>,
    pub genres: Option<Vec<String>>,
    pub total_tracks: Option<i32>,
    pub total_discs: Option<i32>,
    pub cover_url: Option<String>,
    pub overview: Option<String>,
    pub tracks: Option<Vec<TrackInfo>>,
    pub artist_credits: Vec<ArtistCreditInfo>,
}

/// Unified track info from any metadata source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub number: i32,
    pub title: String,
    pub duration: Option<i32>,
}

/// Unified artist credit info.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistCreditInfo {
    pub source: MetadataSource,
    pub external_id: String,
    pub name: String,
}

/// Unified artist info from any metadata source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistInfo {
    pub source: MetadataSource,
    pub external_id: String,
    pub name: String,
    pub profile_url: Option<String>,
    pub biography: Option<String>,
    pub genres: Option<Vec<String>>,
}

// ── Legacy MusicBrainz-specific types (kept for backward compat) ─────────────

/// Adult metadata shared across `JavBus`, `JavDB`, `StashDB`, TPDB clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdultMetadata {
    pub video_id: String,
    pub title: Option<String>,
    pub poster_url: Option<String>,
    pub cover_url: Option<String>,
    pub source_url: Option<String>,
    pub actors: Option<Vec<String>>,
    pub genres: Option<Vec<String>>,
    pub release_date: Option<String>,
    pub studio: Option<String>,
    pub duration: Option<u32>,
    pub rating: Option<f64>,
    pub source: String,
}

/// Adult series video item from prefix search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdultSeriesVideo {
    pub video_id: String,
    pub title: Option<String>,
    pub poster_url: Option<String>,
    pub release_date: Option<String>,
}

/// Individual artist credit (name + `MusicBrainz` artist ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistCredit {
    pub name: String,
    pub mb_id: String,
}

/// Music release match candidate from `MusicBrainz` search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicMatchCandidate {
    pub mb_release_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub track_count: Option<i32>,
    pub country: Option<String>,
    pub format: Option<String>,
    pub score: Option<i32>,
}

/// Music release detail from `MusicBrainz`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicMatchDetail {
    pub mb_release_id: String,
    pub mb_release_group_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub artist_mb_id: Option<String>,
    pub year: Option<i32>,
    pub release_date: Option<String>,
    pub album_type: Option<String>,
    pub genres: Option<Vec<String>>,
    pub total_tracks: Option<i32>,
    pub total_discs: Option<i32>,
    pub cover_url: Option<String>,
    pub overview: Option<String>,
    pub spotify_id: Option<String>,
    pub tracks: Option<Vec<MusicTrack>>,
    /// Individual artist credits — one entry per artist.
    pub artist_credits: Vec<ArtistCredit>,
}

/// Track info within a music release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicTrack {
    pub number: i32,
    pub title: String,
    pub duration: Option<i32>,
}

/// Lyrics result from LRCLIB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricsResult {
    /// Synced lyrics in .lrc format (with timestamps).
    pub synced_lyrics: Option<String>,
    /// Plain text lyrics.
    pub plain_lyrics: Option<String>,
    /// Whether this is an instrumental track.
    pub instrumental: bool,
}
