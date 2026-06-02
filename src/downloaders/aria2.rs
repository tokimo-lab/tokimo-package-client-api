#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;

use super::traits::*;
use crate::error::ClientError;

#[derive(Debug, Clone)]
pub struct Aria2Config {
    pub url: String,
    pub secret: Option<String>,
    pub http_client: reqwest::Client,
}

pub struct Aria2Client {
    client: reqwest::Client,
    config: Aria2Config,
}

impl Aria2Client {
    pub fn new(mut config: Aria2Config) -> Self {
        config.url = config.url.trim_end_matches('/').to_string();
        let client = config.http_client.clone();
        Self { client, config }
    }

    fn build_params(&self, extra: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        let mut params: Vec<serde_json::Value> = Vec::new();
        if let Some(secret) = &self.config.secret {
            params.push(serde_json::Value::String(format!("token:{secret}")));
        }
        params.extend(extra);
        params
    }

    async fn rpc_call(
        &self,
        method: &str,
        extra_params: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, ClientError> {
        let url = format!("{}/jsonrpc", self.config.url);
        let params = self.build_params(extra_params);
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": format!("aria2.{method}"),
            "id": "1",
            "params": params,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let result: serde_json::Value = resp.json().await?;
        if let Some(error) = result.get("error")
            && !error.is_null()
        {
            let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
            return Err(ClientError::Api {
                status: 0,
                message: msg.to_string(),
            });
        }
        Ok(result.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    async fn collect_all_tasks(&self) -> Result<Vec<Aria2Task>, ClientError> {
        let keys = serde_json::json!([
            "gid",
            "status",
            "totalLength",
            "completedLength",
            "downloadSpeed",
            "uploadSpeed",
            "uploadLength",
            "connections",
            "bittorrent",
            "files",
            "dir",
            "errorCode",
            "errorMessage"
        ]);
        let active = self
            .rpc_call("tellActive", vec![keys.clone()])
            .await
            .unwrap_or_default();
        let waiting = self
            .rpc_call(
                "tellWaiting",
                vec![serde_json::json!(0), serde_json::json!(1000), keys.clone()],
            )
            .await
            .unwrap_or_default();
        let stopped = self
            .rpc_call("tellStopped", vec![serde_json::json!(0), serde_json::json!(1000), keys])
            .await
            .unwrap_or_default();

        let mut tasks: Vec<Aria2Task> = Vec::new();
        for value in [active, waiting, stopped] {
            if let Some(arr) = value.as_array() {
                for item in arr {
                    if let Ok(task) = serde_json::from_value::<Aria2Task>(item.clone()) {
                        tasks.push(task);
                    }
                }
            }
        }
        Ok(tasks)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Aria2BtInfo {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Aria2BitTorrent {
    info_hash: Option<String>,
    info: Option<Aria2BtInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Aria2File {
    index: String,
    path: String,
    length: String,
    completed_length: String,
    selected: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Aria2Task {
    gid: String,
    status: String,
    total_length: String,
    completed_length: String,
    download_speed: String,
    upload_speed: String,
    upload_length: Option<String>,
    bittorrent: Option<Aria2BitTorrent>,
    files: Option<Vec<Aria2File>>,
    dir: Option<String>,
    error_message: Option<String>,
}

fn map_aria2_state(status: &str) -> TorrentState {
    match status {
        "active" => TorrentState::Downloading,
        "waiting" => TorrentState::QueuedDl,
        "paused" => TorrentState::PausedDl,
        "error" => TorrentState::Error,
        "complete" => TorrentState::Seeding,
        _ => TorrentState::Unknown,
    }
}

fn task_to_torrent_info(task: &Aria2Task) -> TorrentInfo {
    let hash = task.gid.clone();
    let name = task
        .bittorrent
        .as_ref()
        .and_then(|bt| bt.info.as_ref())
        .and_then(|i| i.name.as_deref())
        .map(std::string::ToString::to_string)
        .or_else(|| {
            task.files.as_ref().and_then(|f| f.first()).map(|f| {
                std::path::Path::new(&f.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&f.path)
                    .to_string()
            })
        })
        .unwrap_or_else(|| task.gid.clone());

    let size = task.total_length.parse::<u64>().unwrap_or(0);
    let completed = task.completed_length.parse::<u64>().unwrap_or(0);
    let progress = if size > 0 { completed as f64 / size as f64 } else { 0.0 };
    let dl_speed = task.download_speed.parse::<u64>().unwrap_or(0);
    let up_speed = task.upload_speed.parse::<u64>().unwrap_or(0);
    let uploaded = task.upload_length.as_deref().unwrap_or("0").parse::<u64>().unwrap_or(0);
    let ratio = if completed > 0 {
        uploaded as f64 / completed as f64
    } else {
        0.0
    };
    let state = map_aria2_state(&task.status);
    let save_path = task.dir.clone().unwrap_or_default();

    TorrentInfo {
        hash,
        name,
        size,
        progress,
        dl_speed,
        up_speed,
        downloaded: completed,
        uploaded,
        ratio,
        state,
        category: String::new(),
        tags: vec![],
        save_path,
        added_on: 0,
        completion_on: None,
        seeding_time: None,
        eta: None,
        seeds: None,
        peers: None,
        tracker: None,
    }
}

impl DownloadClient for Aria2Client {
    fn client_type(&self) -> DownloadClientType {
        DownloadClientType::Aria2
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            supports_pause: true,
            supports_file_priority: true,
            supports_categories: false,
            supports_torrent_file: true,
            min_poll_interval: 3,
        }
    }

    async fn test_connection(&self) -> Result<ClientStatus, ClientError> {
        match self.rpc_call("getVersion", vec![]).await {
            Ok(v) => {
                let version = v
                    .get("version")
                    .and_then(|val| val.as_str())
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
        let tasks = self.collect_all_tasks().await?;
        Ok(tasks.iter().map(task_to_torrent_info).collect())
    }

    async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>, ClientError> {
        let tasks = self.collect_all_tasks().await?;
        Ok(tasks.iter().find(|t| t.gid == hash).map(task_to_torrent_info))
    }

    async fn add_torrent(&self, options: AddTorrentOptions) -> Result<(), ClientError> {
        let mut add_opts = serde_json::json!({});
        if let Some(path) = &options.save_path {
            add_opts["dir"] = serde_json::Value::String(path.clone());
        }
        if let Some(true) = options.paused {
            add_opts["pause"] = serde_json::Value::String("true".to_string());
        }

        if let Some(urls) = &options.urls {
            for url in urls {
                self.rpc_call("addUri", vec![serde_json::json!([url]), add_opts.clone()])
                    .await?;
            }
        }

        if let Some(torrents) = &options.torrents {
            for t in torrents {
                self.rpc_call(
                    "addTorrent",
                    vec![
                        serde_json::Value::String(t.clone()),
                        serde_json::json!([]),
                        add_opts.clone(),
                    ],
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn pause_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        for gid in hashes {
            self.rpc_call("pause", vec![serde_json::Value::String(gid.to_string())])
                .await?;
        }
        Ok(())
    }

    async fn resume_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        for gid in hashes {
            self.rpc_call("unpause", vec![serde_json::Value::String(gid.to_string())])
                .await?;
        }
        Ok(())
    }

    async fn delete_torrents(&self, hashes: &[&str], _delete_files: bool) -> Result<(), ClientError> {
        for gid in hashes {
            let _ = self
                .rpc_call("remove", vec![serde_json::Value::String(gid.to_string())])
                .await;
            let _ = self
                .rpc_call("forceRemove", vec![serde_json::Value::String(gid.to_string())])
                .await;
        }
        Ok(())
    }

    async fn set_category(&self, _hashes: &[&str], _category: &str) -> Result<(), ClientError> {
        Ok(())
    }

    async fn get_categories(&self) -> Result<HashMap<String, CategoryInfo>, ClientError> {
        Ok(HashMap::new())
    }

    async fn create_category(&self, _name: &str, _save_path: Option<&str>) -> Result<(), ClientError> {
        Ok(())
    }

    async fn get_transfer_info(&self) -> Result<TransferInfo, ClientError> {
        let stat = self.rpc_call("getGlobalStat", vec![]).await?;
        let dl_speed = stat
            .get("downloadSpeed")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let up_speed = stat
            .get("uploadSpeed")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        Ok(TransferInfo {
            dl_speed,
            up_speed,
            free_space: 0,
        })
    }

    async fn get_torrent_files(&self, hash: &str) -> Result<Vec<TorrentFile>, ClientError> {
        let result = self
            .rpc_call("getFiles", vec![serde_json::Value::String(hash.to_string())])
            .await?;
        let files: Vec<Aria2File> = serde_json::from_value(result).unwrap_or_default();
        Ok(files
            .into_iter()
            .map(|f| {
                let size = f.length.parse::<u64>().unwrap_or(0);
                let completed = f.completed_length.parse::<u64>().unwrap_or(0);
                let progress = if size > 0 { completed as f64 / size as f64 } else { 0.0 };
                let index = f.index.parse::<u32>().unwrap_or(1).saturating_sub(1);
                let name = std::path::Path::new(&f.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&f.path)
                    .to_string();
                let priority = i32::from(f.selected.as_deref() == Some("true"));
                TorrentFile {
                    index,
                    name,
                    size,
                    progress,
                    priority,
                }
            })
            .collect())
    }

    async fn set_file_priority(&self, hash: &str, file_ids: &[u32], priority: u8) -> Result<(), ClientError> {
        if priority == 0 {
            return Ok(());
        }
        let selected = file_ids
            .iter()
            .map(|id| (id + 1).to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.rpc_call(
            "changeOption",
            vec![
                serde_json::Value::String(hash.to_string()),
                serde_json::json!({ "select-file": selected }),
            ],
        )
        .await?;
        Ok(())
    }
}
