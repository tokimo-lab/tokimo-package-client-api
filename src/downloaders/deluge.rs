#![allow(dead_code)]

use std::collections::HashMap;
use tokio::sync::Mutex;

use super::traits::*;
use crate::error::ClientError;

#[derive(Debug, Clone)]
pub struct DelugeConfig {
    pub url: String,
    pub password: String,
}

pub struct DelugeClient {
    client: reqwest::Client,
    config: DelugeConfig,
    logged_in: Mutex<bool>,
}

impl DelugeClient {
    pub fn new(mut config: DelugeConfig) -> Self {
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

    async fn rpc_call(
        &self,
        method: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}/json", self.config.url);
        let body = serde_json::json!({
            "method": method,
            "params": params,
            "id": 1,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let result: serde_json::Value = resp.json().await?;
        if let Some(error) = result.get("error")
            && !error.is_null()
        {
            let msg = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(ClientError::Api {
                status: 0,
                message: msg.to_string(),
            });
        }
        Ok(result
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    async fn ensure_logged_in(&self) -> Result<(), ClientError> {
        let logged_in = *self.logged_in.lock().await;
        if logged_in {
            return Ok(());
        }
        let result = self
            .rpc_call(
                "auth.login",
                vec![serde_json::Value::String(self.config.password.clone())],
            )
            .await?;
        if result.as_bool() == Some(true) {
            *self.logged_in.lock().await = true;
            Ok(())
        } else {
            Err(ClientError::Auth(
                "Deluge authentication failed".to_string(),
            ))
        }
    }
}

const DELUGE_FIELDS: &[&str] = &[
    "hash",
    "name",
    "total_size",
    "progress",
    "download_payload_rate",
    "upload_payload_rate",
    "all_time_download",
    "total_uploaded",
    "ratio",
    "state",
    "label",
    "save_path",
    "time_added",
    "completed_time",
    "num_seeds",
    "num_peers",
    "tracker_host",
    "eta",
    "seeding_time",
];

fn map_deluge_state(state: &str) -> TorrentState {
    match state {
        "Downloading" => TorrentState::Downloading,
        "Seeding" => TorrentState::Seeding,
        "Paused" => TorrentState::PausedDl,
        "Checking" | "Allocating" => TorrentState::CheckingDl,
        "Error" => TorrentState::Error,
        "Queued" => TorrentState::QueuedDl,
        _ => TorrentState::Unknown,
    }
}

fn deluge_entry_to_torrent_info(hash: &str, v: &serde_json::Value) -> TorrentInfo {
    let progress_raw = v
        .get("progress")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let completion_on = v
        .get("completed_time")
        .and_then(serde_json::Value::as_f64)
        .map(|f| f as i64)
        .filter(|&t| t > 0);
    let state_str = v.get("state").and_then(|x| x.as_str()).unwrap_or("");
    TorrentInfo {
        hash: hash.to_string(),
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        size: v
            .get("total_size")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        progress: progress_raw / 100.0,
        dl_speed: v
            .get("download_payload_rate")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        up_speed: v
            .get("upload_payload_rate")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        downloaded: v
            .get("all_time_download")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        uploaded: v
            .get("total_uploaded")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        ratio: v
            .get("ratio")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0),
        state: map_deluge_state(state_str),
        category: v
            .get("label")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        tags: vec![],
        save_path: v
            .get("save_path")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        added_on: v
            .get("time_added")
            .and_then(serde_json::Value::as_f64)
            .map_or(0, |f| f as i64),
        completion_on,
        seeding_time: v.get("seeding_time").and_then(serde_json::Value::as_u64),
        eta: v
            .get("eta")
            .and_then(serde_json::Value::as_i64)
            .filter(|&e| e > 0),
        seeds: v
            .get("num_seeds")
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as u32),
        peers: v
            .get("num_peers")
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as u32),
        tracker: v
            .get("tracker_host")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string),
    }
}

impl DownloadClient for DelugeClient {
    fn client_type(&self) -> DownloadClientType {
        DownloadClientType::Deluge
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            supports_pause: true,
            supports_file_priority: true,
            supports_categories: true,
            supports_torrent_file: true,
            min_poll_interval: 5,
        }
    }

    async fn test_connection(&self) -> Result<ClientStatus, ClientError> {
        match self.ensure_logged_in().await {
            Ok(()) => match self.rpc_call("daemon.info", vec![]).await {
                Ok(v) => {
                    let version = v.as_str().map(std::string::ToString::to_string);
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
            },
            Err(e) => Ok(ClientStatus {
                connected: false,
                version: None,
                error: Some(e.to_string()),
            }),
        }
    }

    async fn get_torrents(
        &self,
        _filter: Option<&str>,
        _category: Option<&str>,
    ) -> Result<Vec<TorrentInfo>, ClientError> {
        self.ensure_logged_in().await?;
        let fields: Vec<serde_json::Value> = DELUGE_FIELDS
            .iter()
            .map(|f| serde_json::Value::String(f.to_string()))
            .collect();
        let result = self
            .rpc_call(
                "core.get_torrents_status",
                vec![serde_json::json!({}), serde_json::Value::Array(fields)],
            )
            .await?;
        let Some(map) = result.as_object() else {
            return Ok(vec![]);
        };
        Ok(map
            .iter()
            .map(|(hash, v)| deluge_entry_to_torrent_info(hash, v))
            .collect())
    }

    async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>, ClientError> {
        let torrents = self.get_torrents(None, None).await?;
        Ok(torrents
            .into_iter()
            .find(|t| t.hash.eq_ignore_ascii_case(hash)))
    }

    async fn add_torrent(&self, options: AddTorrentOptions) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let mut add_opts = serde_json::json!({});
        if let Some(path) = &options.save_path {
            add_opts["save_path"] = serde_json::Value::String(path.clone());
        }
        if let Some(paused) = options.paused {
            add_opts["add_paused"] = serde_json::Value::Bool(paused);
        }

        if let Some(urls) = &options.urls {
            for url in urls {
                self.rpc_call(
                    "core.add_torrent_url",
                    vec![serde_json::Value::String(url.clone()), add_opts.clone()],
                )
                .await?;
            }
        }

        if let Some(torrents) = &options.torrents {
            for t in torrents {
                self.rpc_call(
                    "core.add_torrent_file",
                    vec![
                        serde_json::Value::String("torrent.torrent".to_string()),
                        serde_json::Value::String(t.clone()),
                        add_opts.clone(),
                    ],
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn pause_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let ids: Vec<serde_json::Value> = hashes
            .iter()
            .map(|h| serde_json::Value::String(h.to_string()))
            .collect();
        self.rpc_call("core.pause_torrent", vec![serde_json::Value::Array(ids)])
            .await?;
        Ok(())
    }

    async fn resume_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let ids: Vec<serde_json::Value> = hashes
            .iter()
            .map(|h| serde_json::Value::String(h.to_string()))
            .collect();
        self.rpc_call("core.resume_torrent", vec![serde_json::Value::Array(ids)])
            .await?;
        Ok(())
    }

    async fn delete_torrents(
        &self,
        hashes: &[&str],
        delete_files: bool,
    ) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        for hash in hashes {
            self.rpc_call(
                "core.remove_torrent",
                vec![
                    serde_json::Value::String(hash.to_string()),
                    serde_json::Value::Bool(delete_files),
                ],
            )
            .await?;
        }
        Ok(())
    }

    async fn set_category(&self, hashes: &[&str], category: &str) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        for hash in hashes {
            let _ = self
                .rpc_call(
                    "label.set_torrent",
                    vec![
                        serde_json::Value::String(hash.to_string()),
                        serde_json::Value::String(category.to_string()),
                    ],
                )
                .await;
        }
        Ok(())
    }

    async fn get_categories(&self) -> Result<HashMap<String, CategoryInfo>, ClientError> {
        self.ensure_logged_in().await?;
        match self.rpc_call("label.get_labels", vec![]).await {
            Ok(v) => {
                let labels = v.as_array().cloned().unwrap_or_default();
                Ok(labels
                    .into_iter()
                    .filter_map(|l| {
                        let name = l.as_str()?.to_string();
                        Some((
                            name.clone(),
                            CategoryInfo {
                                name,
                                save_path: String::new(),
                            },
                        ))
                    })
                    .collect())
            }
            Err(_) => Ok(HashMap::new()),
        }
    }

    async fn create_category(
        &self,
        name: &str,
        _save_path: Option<&str>,
    ) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let _ = self
            .rpc_call(
                "label.add",
                vec![serde_json::Value::String(name.to_string())],
            )
            .await;
        Ok(())
    }

    async fn get_transfer_info(&self) -> Result<TransferInfo, ClientError> {
        self.ensure_logged_in().await?;
        let stats = self
            .rpc_call(
                "core.get_session_status",
                vec![serde_json::json!(["download_rate", "upload_rate"])],
            )
            .await?;
        let dl_speed = stats
            .get("download_rate")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let up_speed = stats
            .get("upload_rate")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let free = self
            .rpc_call("core.get_free_space", vec![])
            .await
            .ok()
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Ok(TransferInfo {
            dl_speed,
            up_speed,
            free_space: free,
        })
    }

    async fn get_torrent_files(&self, hash: &str) -> Result<Vec<TorrentFile>, ClientError> {
        self.ensure_logged_in().await?;
        let result = self
            .rpc_call(
                "core.get_torrent_status",
                vec![
                    serde_json::Value::String(hash.to_string()),
                    serde_json::json!(["files"]),
                ],
            )
            .await?;
        let files = result
            .get("files")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(files
            .into_iter()
            .map(|f| TorrentFile {
                index: f
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32,
                name: f
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                size: f
                    .get("size")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                progress: f
                    .get("progress")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0),
                priority: f
                    .get("priority")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1) as i32,
            })
            .collect())
    }

    async fn set_file_priority(
        &self,
        hash: &str,
        file_ids: &[u32],
        priority: u8,
    ) -> Result<(), ClientError> {
        self.ensure_logged_in().await?;
        let status = self
            .rpc_call(
                "core.get_torrent_status",
                vec![
                    serde_json::Value::String(hash.to_string()),
                    serde_json::json!(["num_files"]),
                ],
            )
            .await?;
        let num_files = status
            .get("num_files")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let mut priorities: Vec<u8> = vec![1; num_files];
        for &id in file_ids {
            if (id as usize) < num_files {
                priorities[id as usize] = priority;
            }
        }
        self.rpc_call(
            "core.set_torrent_file_priorities",
            vec![
                serde_json::Value::String(hash.to_string()),
                serde_json::to_value(priorities)?,
            ],
        )
        .await?;
        Ok(())
    }
}
