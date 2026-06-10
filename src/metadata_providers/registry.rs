//! Provider registry — manages multiple metadata providers.

use super::MusicMetadataProvider;
use crate::error::ClientError;
use crate::types::{AlbumDetail, AlbumSearchResult, ArtistInfo, MetadataSource};
use std::sync::Arc;

/// Registry of music metadata providers.
///
/// Manages multiple providers and provides methods to search across all of them
/// or query a specific provider by source.
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn MusicMetadataProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self { providers: Vec::new() }
    }

    /// Register a new provider.
    pub fn register(&mut self, provider: Arc<dyn MusicMetadataProvider>) {
        self.providers.push(provider);
    }

    /// Get a list of all registered sources.
    pub fn sources(&self) -> Vec<MetadataSource> {
        self.providers.iter().map(|p| p.source()).collect()
    }

    /// Find a provider by source.
    pub fn get(&self, source: &MetadataSource) -> Option<&Arc<dyn MusicMetadataProvider>> {
        self.providers.iter().find(|p| p.source() == *source)
    }

    /// Search all providers concurrently and merge results.
    ///
    /// Returns results sorted by provider registration order, then by score.
    pub async fn search_albums(&self, artist: &str, album: &str, limit: u32) -> Vec<AlbumSearchResult> {
        let futures: Vec<_> = self
            .providers
            .iter()
            .map(|p| {
                let artist = artist.to_string();
                let album = album.to_string();
                let p = Arc::clone(p);
                async move { p.search_albums(&artist, &album, limit).await.unwrap_or_default() }
            })
            .collect();

        let results = futures::future::join_all(futures).await;
        let mut all: Vec<AlbumSearchResult> = results.into_iter().flatten().collect();

        // Sort by score descending (higher is better)
        all.sort_by_key(|b| std::cmp::Reverse(b.score.unwrap_or(0)));
        all
    }

    /// Search all providers by keyword and merge results.
    pub async fn search_albums_by_keyword(&self, keyword: &str, limit: u32) -> Vec<AlbumSearchResult> {
        let futures: Vec<_> = self
            .providers
            .iter()
            .map(|p| {
                let keyword = keyword.to_string();
                let p = Arc::clone(p);
                async move { p.search_albums_by_keyword(&keyword, limit).await.unwrap_or_default() }
            })
            .collect();

        let results = futures::future::join_all(futures).await;
        let mut all: Vec<AlbumSearchResult> = results.into_iter().flatten().collect();

        all.sort_by_key(|b| std::cmp::Reverse(b.score.unwrap_or(0)));
        all
    }

    /// Get album detail from a specific provider.
    pub async fn get_album_detail(
        &self,
        source: &MetadataSource,
        external_id: &str,
    ) -> Result<AlbumDetail, ClientError> {
        let provider = self
            .get(source)
            .ok_or_else(|| ClientError::Other(format!("provider not found: {source}")))?;
        provider.get_album_detail(external_id).await
    }

    /// Get artist info from a specific provider.
    pub async fn get_artist_info(&self, source: &MetadataSource, external_id: &str) -> Result<ArtistInfo, ClientError> {
        let provider = self
            .get(source)
            .ok_or_else(|| ClientError::Other(format!("provider not found: {source}")))?;
        provider.get_artist_info(external_id).await
    }

    /// Health check all providers.
    pub async fn health_check_all(&self) -> Vec<(MetadataSource, bool)> {
        let futures: Vec<_> = self
            .providers
            .iter()
            .map(|p| {
                let p = Arc::clone(p);
                async move {
                    let source = p.source();
                    let ok = p.health_check().await.unwrap_or(false);
                    (source, ok)
                }
            })
            .collect();

        futures::future::join_all(futures).await
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
