#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::Mutex;

use super::traits::*;
use crate::error::ClientError;

#[derive(Debug, Clone)]
pub struct QBittorrentConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

pub struct QBittorrentClient {
    client: reqwest::Client,
    config: QBittorrentConfig,
    logged_in: Mutex<bool>,
}

impl QBittorrentClient {
    pub fn new(mut config: QBittorrentConfig) -> Self {
        config.url = config.url.trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .expect("Failed to build reqwest client");
        Self {
            client,
            config,
            logged_in: Mutex::new(false),
        }
    }

    async fn ensure_logged_in(&self) -> Result<(), ClientError> {
        let logged_in = *self.logged_in.lock().await;
        if logged_in {
            return Ok(());
        }
        let url = format!("{}/api/v2/auth/login", self.config.url);
        let resp = self
            .client
            .post(&url)
            .form(&[
                ("username", self.config.username.as_str()),
                ("password", self.config.password.as_str()),
            ])
            .send()
            .await?;
        let text = resp.text().await?;
        if text.trim() == "Ok." {
            *self.logged_in.lock().await = true;
            Ok(())
        } else if text.contains("Fails.") {
            Err(ClientError::Auth("Invalid credentials".to_string()))
        } else {
            Err(ClientError::Other(format!("Login failed: {text}")))
        }
    }
}

#[derive(Debug, Deserialize)]
struct QbtTorrentInfo {
    hash: String,
    name: String,
    size: u64,
    progress: f64,
    dlspeed: u64,
    upspeed: u64,
    downloaded: u64,
    uploaded: u64,
    ratio: f64,
    state: String,
    category: String,
    tags: String,
    save_path: String,
    added_on: i64,
    completion_on: Option<i64>,
    seeding_time: Option<u64>,
    eta: Option<i64>,
    num_seeds: Option<u32>,
    num_peers: Option<u32>,
    tracker: Option<String>,
}

fn map_qbt_state(state: &str) -> TorrentState {
    match state {
        "allocating" | "checkingDL" | "checkingResumeData" => TorrentState::CheckingDl,
        "checkingUP" => TorrentState::CheckingUp,
        "downloading" | "forcedDL" | "metaDL" | "moving" => TorrentState::Downloading,
        "error" => TorrentState::Error,
        "forcedUP" | "uploading" => TorrentState::Seeding,
        "missingFiles" => TorrentState::MissingFiles,
        "pausedDL" => TorrentState::PausedDl,
        "pausedUP" => TorrentState::PausedUp,
        "queuedDL" => TorrentState::QueuedDl,
        "queuedUP" => TorrentState::QueuedUp,
        "stalledDL" => TorrentState::StalledDl,
        "stalledUP" => TorrentState::StalledUp,
        _ => TorrentState::Unknown,
    }
}

impl From<QbtTorrentInfo> for TorrentInfo {
    fn from(q: QbtTorrentInfo) -> Self {
        let tags: Vec<String> = if q.tags.is_empty() {
            vec![]
        } else {
            q.tags.split(',').map(|s| s.trim().to_string()).collect()
        };
        let completion_on = match q.completion_on {
            Some(-1) | None => None,
            v => v,
        };
        TorrentInfo {
            hash: q.hash,
            name: q.name,
            size: q.size,
            progress: q.progress,
            dl_speed: q.dlspeed,
            up_speed: q.upspeed,
            downloaded: q.downloaded,
            uploaded: q.uploaded,
            ratio: q.ratio,
            state: map_qbt_state(&q.state),
            category: q.category,
            tags,
            save_path: q.save_path,
            added_on: q.added_on,
            completion_on,
            seeding_time: q.seeding_time,
            eta: q.eta,
            seeds: q.num_seeds,
            peers: q.num_peers,
            tracker: q.tracker,
        }
    }
}

#[derive(Debug, Deserialize)]
struct QbtCategory {
    name: String,
    #[serde(rename = "savePath")]
    save_path: String,
}

#[derive(Debug, Deserialize)]
struct QbtMaindata {
    server_state: QbtServerState,
}

#[derive(Debug, Deserialize)]
struct QbtServerState {
    dl_info_speed: u64,
    up_info_speed: u64,
    free_space_on_disk: u64,
}

impl DownloadClient for QBittorrentClient {
    fn client_type(&self) -> DownloadClientType {
        DownloadClientType::QBittorrent
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            supports_pause: true,
            supports_file_priority: true,
            supports_categories: true,
            supports_torrent_file: true,
            min_poll_interval: 3,
        }
    }

    async fn test_connection(&self) -> Result<ClientStatus, ClientError> {
        match self.ensure_logged_in().await {
            Ok(()) => {
                let url = format!("{}/api/v2/app/version", self.config.url);
                match self.client.get(&url).send().await {
                    Ok(resp) => {
                        let version = resp.text().await.ok();
                        Ok(ClientStatus {
                            connected: true,
                            version,
                            error: None,
                        })
                    }
                    Err(e) => Ok(ClientStatus {
                        connected: false,
                        version: None,
                        error: Some(e.to_string()),
                    }),
                }
            }
            Err(e) => Ok(ClientStatus {
                connected: false,
                version: None,
                error: Some(e.to_string()),
            }),
        }
    }

    async fn get_torrents(
        &self,
        filter: Option<&str>,
        category: Option<&str>,
    ) -> Result<Vec<TorrentInfo>, ClientError> {
        self.ensure_logged_in().await?;
        let base = format!("{}/api/v2/torrents/info", self.config.url);
        let mut parts: Vec<String> = Vec::new();
        if let Some(f) = filter {
            parts.push(format!("filter={f}"));
        }
        if let Some(c) = category {
            parts.push(format!("category={c}"));
        }
        let url = if parts.is_empty() {
            base
        } else {
            format!("{}?{}", base, parts.join("&"))
        };
        let resp = self.client.get(&url).send().await?;
        let torrents: Vec<QbtTorrentInfo> = resp.json().await?;
        Ok(torrents.into_iter().map(TorrentInfo::from).collect())
    }

    async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>, ClientError> {
        let torrents = self.get_torrents(None, None).await?;
        Ok(torrents.into_iter().find(|t| t.hash.eq_ignore_ascii_case(hash)))
    }

    async fn add_torrent(&self, options: AddTorrentOptions) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/add", self.config.url);
        let mut form: Vec<(String, String)> = Vec::new();
        if let Some(urls) = &options.urls {
            form.push(("urls".to_string(), urls.join("\n")));
        }
        if let Some(torrents) = &options.torrents {
            for t in torrents {
                form.push(("torrents".to_string(), t.clone()));
            }
        }
        if let Some(path) = &options.save_path {
            form.push(("savepath".to_string(), path.clone()));
        }
        if let Some(cat) = &options.category {
            form.push(("category".to_string(), cat.clone()));
        }
        if let Some(tags) = &options.tags {
            form.push(("tags".to_string(), tags.join(",")));
        }
        if let Some(paused) = options.paused {
            form.push(("paused".to_string(), paused.to_string()));
        }
        if let Some(skip) = options.skip_hash_check {
            form.push(("skip_checking".to_string(), skip.to_string()));
        }
        let form_vec: Vec<(&str, &str)> = form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        self.client.post(&url).form(form_vec.as_slice()).send().await?;
        Ok(())
    }

    async fn pause_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/pause", self.config.url);
        let hashes_str = hashes.join("|");
        self.client
            .post(&url)
            .form(&[("hashes", hashes_str.as_str())])
            .send()
            .await?;
        Ok(())
    }

    async fn resume_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/resume", self.config.url);
        let hashes_str = hashes.join("|");
        self.client
            .post(&url)
            .form(&[("hashes", hashes_str.as_str())])
            .send()
            .await?;
        Ok(())
    }

    async fn delete_torrents(&self, hashes: &[&str], delete_files: bool) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/delete", self.config.url);
        let hashes_str = hashes.join("|");
        let delete_str = if delete_files { "true" } else { "false" };
        self.client
            .post(&url)
            .form(&[("hashes", hashes_str.as_str()), ("deleteFiles", delete_str)])
            .send()
            .await?;
        Ok(())
    }

    async fn set_category(&self, hashes: &[&str], category: &str) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/setCategory", self.config.url);
        let hashes_str = hashes.join("|");
        self.client
            .post(&url)
            .form(&[("hashes", hashes_str.as_str()), ("category", category)])
            .send()
            .await?;
        Ok(())
    }

    async fn get_categories(&self) -> Result<HashMap<String, CategoryInfo>, ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/categories", self.config.url);
        let resp = self.client.get(&url).send().await?;
        let raw: HashMap<String, QbtCategory> = resp.json().await?;
        Ok(raw
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    CategoryInfo {
                        name: v.name,
                        save_path: v.save_path,
                    },
                )
            })
            .collect())
    }

    async fn create_category(&self, name: &str, save_path: Option<&str>) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/createCategory", self.config.url);
        let path = save_path.unwrap_or("");
        self.client
            .post(&url)
            .form(&[("category", name), ("savePath", path)])
            .send()
            .await?;
        Ok(())
    }

    async fn get_transfer_info(&self) -> Result<TransferInfo, ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/sync/maindata", self.config.url);
        let resp = self.client.get(&url).send().await?;
        let data: QbtMaindata = resp.json().await?;
        Ok(TransferInfo {
            dl_speed: data.server_state.dl_info_speed,
            up_speed: data.server_state.up_info_speed,
            free_space: data.server_state.free_space_on_disk,
        })
    }

    async fn get_torrent_files(&self, hash: &str) -> Result<Vec<TorrentFile>, ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/files?hash={hash}", self.config.url);
        let resp = self.client.get(&url).send().await?;
        let files: Vec<serde_json::Value> = resp.json().await?;
        Ok(files
            .into_iter()
            .enumerate()
            .map(|(i, f)| TorrentFile {
                index: f.get("index").and_then(serde_json::Value::as_u64).unwrap_or(i as u64) as u32,
                name: f.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                size: f.get("size").and_then(serde_json::Value::as_u64).unwrap_or(0),
                progress: f.get("progress").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                priority: f.get("priority").and_then(serde_json::Value::as_i64).unwrap_or(0) as i32,
            })
            .collect())
    }

    async fn set_file_priority(&self, hash: &str, file_ids: &[u32], priority: u8) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let url = format!("{}/api/v2/torrents/filePrio", self.config.url);
        let ids_str: String = file_ids
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join("|");
        let priority_str = priority.to_string();
        self.client
            .post(&url)
            .form(&[("hash", hash), ("id", &ids_str), ("priority", &priority_str)])
            .send()
            .await?;
        Ok(())
    }
}
