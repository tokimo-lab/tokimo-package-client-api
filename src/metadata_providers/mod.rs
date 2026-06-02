pub mod bangumi;
pub mod deezer;
pub mod douban;
pub mod fanart;
pub mod javbus;
pub mod javdb;
pub mod lrclib;
pub mod lyrics;
pub mod musicbrainz;
pub mod nager_date;
pub mod netease;
pub mod omdb;
pub mod qidian;
pub mod qqmusic;
pub mod registry;
pub mod selector;
pub mod spotify;
pub mod stashdb;
pub mod thetvdb;
pub mod tmdb;
pub mod tpdb;
pub mod wikipedia;

// Re-export the provider trait and registry
pub use registry::ProviderRegistry;
pub use selector::ProviderSelector;

use crate::error::ClientError;
use crate::types::{AlbumDetail, AlbumSearchResult, ArtistInfo, MetadataSource};
use async_trait::async_trait;

/// Abstract interface for music metadata providers.
///
/// Each provider (MusicBrainz, Netease, QQ Music, etc.) implements this trait
/// to provide a unified interface for searching and retrieving music metadata.
#[async_trait]
pub trait MusicMetadataProvider: Send + Sync {
    /// The metadata source identifier.
    fn source(&self) -> MetadataSource;

    /// Search for albums by artist and album name.
    async fn search_albums(
        &self,
        artist: &str,
        album: &str,
        limit: u32,
    ) -> Result<Vec<AlbumSearchResult>, ClientError>;

    /// Search for albums by keyword only.
    async fn search_albums_by_keyword(
        &self,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<AlbumSearchResult>, ClientError>;

    /// Get full album details by external ID.
    async fn get_album_detail(&self, external_id: &str) -> Result<AlbumDetail, ClientError>;

    /// Get artist information by external ID.
    async fn get_artist_info(&self, external_id: &str) -> Result<ArtistInfo, ClientError>;

    /// Health check (default: always ok).
    async fn health_check(&self) -> Result<bool, ClientError> {
        Ok(true)
    }
}
