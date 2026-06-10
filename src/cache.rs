use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

struct CacheEntry {
    data: Vec<u8>,
    expire_at: Instant,
}

/// In-memory cache with TTL, matching the TS clients' caching pattern.
pub struct RequestCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

impl RequestCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Get a cached value by key. Returns `None` if missing or expired.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(key)
            && entry.expire_at > Instant::now()
        {
            return serde_json::from_slice(&entry.data).ok();
        }
        None
    }

    /// Store a value in the cache with the configured TTL.
    pub async fn set<T: Serialize>(&self, key: &str, data: &T) {
        if let Ok(bytes) = serde_json::to_vec(data) {
            let mut entries = self.entries.write().await;
            entries.insert(
                key.to_string(),
                CacheEntry {
                    data: bytes,
                    expire_at: Instant::now() + self.ttl,
                },
            );
        }
    }

    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }

    pub async fn size(&self) -> usize {
        let entries = self.entries.read().await;
        entries.values().filter(|e| e.expire_at > Instant::now()).count()
    }
}
