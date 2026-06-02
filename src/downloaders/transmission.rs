#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::Mutex;

use super::traits::*;
use crate::error::ClientError;

#[derive(Debug, Clone)]
pub struct TransmissionConfig {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub http_client: reqwest::Client,
}

pub struct TransmissionClient {
    client: reqwest::Client,
    config: TransmissionConfig,
    session_id: Mutex<Option<String>>,
}

impl TransmissionClient {
    pub fn new(mut config: TransmissionConfig) -> Self {
        config.url = config.url.trim_end_matches('/').to_string();
        let client = config.http_client.clone();
        Self {
            client,
            config,
            session_id: Mutex::new(None),
        }
    }

    async fn rpc_call(
        &self,
        method: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}/transmission/rpc", self.config.url);
        let body = serde_json::json!({ "method": method, "arguments": args });

        let session_id = self.session_id.lock().await.clone();

        let resp = {
            let mut req = self.client.post(&url).json(&body);
            if let Some(ref sid) = session_id {
                req = req.header("X-Transmission-Session-Id", sid.as_str());
            }
            if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
                req = req.basic_auth(user, Some(pass));
            }
            req.send().await?
        };

        if resp.status().as_u16() == 409 {
            let new_sid = resp
                .headers()
                .get("X-Transmission-Session-Id")
                .and_then(|v| v.to_str().ok())
                .map(std::string::ToString::to_string);
            if let Some(sid) = new_sid {
                *self.session_id.lock().await = Some(sid.clone());
                let mut req2 = self.client.post(&url).json(&body);
                req2 = req2.header("X-Transmission-Session-Id", sid);
                if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
                    req2 = req2.basic_auth(user, Some(pass));
                }
                let resp2 = req2.send().await?;
                let result: serde_json::Value = resp2.json().await?;
                return self.check_result(result);
            }
            return Err(ClientError::Other(
                "Got 409 but no session ID header".to_string(),
            ));
        }

        let result: serde_json::Value = resp.json().await?;
        self.check_result(result)
    }

    #[allow(clippy::unused_self)]
    fn check_result(&self, value: serde_json::Value) -> Result<serde_json::Value, ClientError> {
        let result_str = value
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("error");
        if result_str != "success" {
            return Err(ClientError::Api {
                status: 0,
                message: result_str.to_string(),
            });
        }
        Ok(value
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }
}

const TORRENT_FIELDS: &[&str] = &[
    "hashString",
    "name",
    "totalSize",
    "percentDone",
    "rateDownload",
    "rateUpload",
    "downloadedEver",
    "uploadedEver",
    "uploadRatio",
    "status",
    "downloadDir",
    "addedDate",
    "doneDate",
    "error",
    "errorString",
    "isFinished",
    "leftUntilDone",
    "peersConnected",
    "labels",
    "eta",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrTorrent {
    hash_string: String,
    name: String,
    total_size: u64,
    percent_done: f64,
    rate_download: u64,
    rate_upload: u64,
    downloaded_ever: u64,
    uploaded_ever: u64,
    upload_ratio: f64,
    status: u8,
    download_dir: String,
    added_date: i64,
    done_date: Option<i64>,
    error: Option<u8>,
    error_string: Option<String>,
    is_finished: Option<bool>,
    peers_connected: Option<u32>,
    labels: Option<Vec<String>>,
    eta: Option<i64>,
}

fn map_tr_status(status: u8, error: Option<u8>) -> TorrentState {
    if error.unwrap_or(0) != 0 {
        return TorrentState::Error;
    }
    match status {
        0 => TorrentState::PausedDl,
        1 | 2 => TorrentState::CheckingDl,
        3 => TorrentState::QueuedDl,
        4 => TorrentState::Downloading,
        5 => TorrentState::QueuedUp,
        6 => TorrentState::Seeding,
        _ => TorrentState::Unknown,
    }
}

fn tr_to_torrent_info(t: TrTorrent) -> TorrentInfo {
    let completion_on = match t.done_date {
        Some(0) | None => None,
        v => v,
    };
    let tags = t.labels.unwrap_or_default();
    let error_str = t.error_string.filter(|s| !s.is_empty());
    let state = if error_str.is_some() {
        TorrentState::Error
    } else {
        map_tr_status(t.status, t.error)
    };
    let is_finished = t.is_finished.unwrap_or(false);
    let actual_state = if is_finished && state == TorrentState::Seeding {
        TorrentState::Seeding
    } else {
        state
    };
    TorrentInfo {
        hash: t.hash_string,
        name: t.name,
        size: t.total_size,
        progress: t.percent_done,
        dl_speed: t.rate_download,
        up_speed: t.rate_upload,
        downloaded: t.downloaded_ever,
        uploaded: t.uploaded_ever,
        ratio: t.upload_ratio,
        state: actual_state,
        category: String::new(),
        tags,
        save_path: t.download_dir,
        added_on: t.added_date,
        completion_on,
        seeding_time: None,
        eta: t.eta.filter(|&e| e >= 0),
        seeds: None,
        peers: t.peers_connected,
        tracker: None,
    }
}

impl DownloadClient for TransmissionClient {
    fn client_type(&self) -> DownloadClientType {
        DownloadClientType::Transmission
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            supports_pause: true,
            supports_file_priority: false,
            supports_categories: false,
            supports_torrent_file: true,
            min_poll_interval: 5,
        }
    }

    async fn test_connection(&self) -> Result<ClientStatus, ClientError> {
        match self.rpc_call("session-get", serde_json::json!({})).await {
            Ok(args) => {
                let version = args
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(std::string::ToString::to_string);
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

    async fn get_torrents(
        &self,
        _filter: Option<&str>,
        _category: Option<&str>,
    ) -> Result<Vec<TorrentInfo>, ClientError> {
        let args = serde_json::json!({ "fields": TORRENT_FIELDS });
        let result = self.rpc_call("torrent-get", args).await?;
        let torrents: Vec<TrTorrent> = serde_json::from_value(
            result
                .get("torrents")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )?;
        Ok(torrents.into_iter().map(tr_to_torrent_info).collect())
    }

    async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>, ClientError> {
        let torrents = self.get_torrents(None, None).await?;
        Ok(torrents
            .into_iter()
            .find(|t| t.hash.eq_ignore_ascii_case(hash)))
    }

    async fn add_torrent(&self, options: AddTorrentOptions) -> Result<(), ClientError> {
        let mut tasks: Vec<serde_json::Value> = Vec::new();

        if let Some(urls) = &options.urls {
            for url in urls {
                let mut args = serde_json::json!({ "filename": url });
                if let Some(path) = &options.save_path {
                    args["download-dir"] = serde_json::Value::String(path.clone());
                }
                if let Some(paused) = options.paused {
                    args["paused"] = serde_json::Value::Bool(paused);
                }
                tasks.push(args);
            }
        }

        if let Some(torrents) = &options.torrents {
            for t in torrents {
                let mut args = serde_json::json!({ "metainfo": t });
                if let Some(path) = &options.save_path {
                    args["download-dir"] = serde_json::Value::String(path.clone());
                }
                if let Some(paused) = options.paused {
                    args["paused"] = serde_json::Value::Bool(paused);
                }
                tasks.push(args);
            }
        }

        for args in tasks {
            self.rpc_call("torrent-add", args).await?;
        }
        Ok(())
    }

    async fn pause_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        let ids: Vec<serde_json::Value> = hashes
            .iter()
            .map(|h| serde_json::Value::String(h.to_string()))
            .collect();
        self.rpc_call("torrent-stop", serde_json::json!({ "ids": ids }))
            .await?;
        Ok(())
    }

    async fn resume_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        let ids: Vec<serde_json::Value> = hashes
            .iter()
            .map(|h| serde_json::Value::String(h.to_string()))
            .collect();
        self.rpc_call("torrent-start", serde_json::json!({ "ids": ids }))
            .await?;
        Ok(())
    }

    async fn delete_torrents(
        &self,
        hashes: &[&str],
        delete_files: bool,
    ) -> Result<(), ClientError> {
        let ids: Vec<serde_json::Value> = hashes
            .iter()
            .map(|h| serde_json::Value::String(h.to_string()))
            .collect();
        self.rpc_call(
            "torrent-remove",
            serde_json::json!({ "ids": ids, "delete-local-data": delete_files }),
        )
        .await?;
        Ok(())
    }

    async fn set_category(&self, hashes: &[&str], category: &str) -> Result<(), ClientError> {
        let ids: Vec<serde_json::Value> = hashes
            .iter()
            .map(|h| serde_json::Value::String(h.to_string()))
            .collect();
        self.rpc_call(
            "torrent-set",
            serde_json::json!({ "ids": ids, "labels": [category] }),
        )
        .await?;
        Ok(())
    }

    async fn get_categories(&self) -> Result<HashMap<String, CategoryInfo>, ClientError> {
        Ok(HashMap::new())
    }

    async fn create_category(
        &self,
        _name: &str,
        _save_path: Option<&str>,
    ) -> Result<(), ClientError> {
        Ok(())
    }

    async fn get_transfer_info(&self) -> Result<TransferInfo, ClientError> {
        let result = self
            .rpc_call("session-stats", serde_json::json!({}))
            .await?;
        let dl_speed = result
            .get("downloadSpeed")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let up_speed = result
            .get("uploadSpeed")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        Ok(TransferInfo {
            dl_speed,
            up_speed,
            free_space: 0,
        })
    }

    async fn get_torrent_files(&self, hash: &str) -> Result<Vec<TorrentFile>, ClientError> {
        let args = serde_json::json!({
            "ids": [hash],
            "fields": ["files", "fileStats"],
        });
        let result = self.rpc_call("torrent-get", args).await?;
        let torrents = result
            .get("torrents")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let Some(torrent) = torrents.into_iter().next() else {
            return Ok(vec![]);
        };
        let files = torrent
            .get("files")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let file_stats = torrent
            .get("fileStats")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let result_files = files
            .into_iter()
            .enumerate()
            .map(|(i, f)| {
                let name = f
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let size = f
                    .get("length")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let completed = f
                    .get("bytesCompleted")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let progress = if size > 0 {
                    completed as f64 / size as f64
                } else {
                    0.0
                };
                let priority = file_stats
                    .get(i)
                    .and_then(|s| s.get("priority"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0) as i32;
                TorrentFile {
                    index: i as u32,
                    name,
                    size,
                    progress,
                    priority,
                }
            })
            .collect();
        Ok(result_files)
    }

    async fn set_file_priority(
        &self,
        _hash: &str,
        _file_ids: &[u32],
        _priority: u8,
    ) -> Result<(), ClientError> {
        Ok(())
    }
}
