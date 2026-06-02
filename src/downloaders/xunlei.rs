#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::Mutex;

use super::traits::*;
use crate::error::ClientError;

#[derive(Debug, Clone)]
pub struct XunleiConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub http_client: reqwest::Client,
}

pub struct XunleiClient {
    client: reqwest::Client,
    config: XunleiConfig,
    token: Mutex<Option<String>>,
}

impl XunleiClient {
    pub fn new(mut config: XunleiConfig) -> Self {
        config.url = config.url.trim_end_matches('/').to_string();
        let client = config.http_client.clone();
        Self {
            client,
            config,
            token: Mutex::new(None),
        }
    }

    async fn ensure_authenticated(&self) -> Result<String, ClientError> {
        let existing = self.token.lock().await.clone();
        if let Some(t) = existing {
            return Ok(t);
        }
        let url = format!(
            "{}/webman/3rdparty/pan-xunlei-com/index.cgi/device/token",
            self.config.url
        );
        let body = serde_json::json!({
            "username": self.config.username,
            "password": self.config.password,
            "login_type": "regular",
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let data: serde_json::Value = resp.json().await?;
        let token = data
            .get("token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ClientError::Auth("No token in Xunlei login response".to_string()))?
            .to_string();
        *self.token.lock().await = Some(token.clone());
        Ok(token)
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, ClientError> {
        let token = self.ensure_authenticated().await?;
        let url = format!("{}{path}", self.config.url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        Ok(resp.json().await?)
    }

    async fn post_json(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let token = self.ensure_authenticated().await?;
        let url = format!("{}{path}", self.config.url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&body)
            .send()
            .await?;
        Ok(resp.json().await?)
    }
}

#[derive(Debug, Deserialize)]
struct XunleiTask {
    id: String,
    #[serde(rename = "type")]
    task_type: Option<String>,
    name: String,
    state: String,
    progress: f64,
    file_size: Option<u64>,
    speed: Option<u64>,
    real_path: Option<String>,
}

fn map_xunlei_state(state: &str) -> TorrentState {
    match state {
        "running" => TorrentState::Downloading,
        "paused" => TorrentState::PausedDl,
        "completed" => TorrentState::Seeding,
        "error" => TorrentState::Error,
        "pending" => TorrentState::QueuedDl,
        _ => TorrentState::Unknown,
    }
}

fn xunlei_task_to_torrent_info(task: XunleiTask) -> TorrentInfo {
    let progress = task.progress / 100.0;
    let save_path = task.real_path.clone().unwrap_or_default();
    TorrentInfo {
        hash: task.id,
        name: task.name,
        size: task.file_size.unwrap_or(0),
        progress,
        dl_speed: task.speed.unwrap_or(0),
        up_speed: 0,
        downloaded: (task.file_size.unwrap_or(0) as f64 * progress) as u64,
        uploaded: 0,
        ratio: 0.0,
        state: map_xunlei_state(&task.state),
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

impl DownloadClient for XunleiClient {
    fn client_type(&self) -> DownloadClientType {
        DownloadClientType::Xunlei
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            supports_pause: true,
            supports_file_priority: false,
            supports_categories: false,
            supports_torrent_file: false,
            min_poll_interval: 5,
        }
    }

    async fn test_connection(&self) -> Result<ClientStatus, ClientError> {
        match self.ensure_authenticated().await {
            Ok(_) => Ok(ClientStatus {
                connected: true,
                version: None,
                error: None,
            }),
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
        let data = self
            .get_json("/webman/3rdparty/pan-xunlei-com/index.cgi/device/list_running_tasks")
            .await?;
        let tasks_val = data
            .get("tasks")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut result = Vec::new();
        for t in tasks_val {
            if let Ok(task) = serde_json::from_value::<XunleiTask>(t) {
                result.push(xunlei_task_to_torrent_info(task));
            }
        }
        Ok(result)
    }

    async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>, ClientError> {
        let torrents = self.get_torrents(None, None).await?;
        Ok(torrents.into_iter().find(|t| t.hash == hash))
    }

    async fn add_torrent(&self, options: AddTorrentOptions) -> Result<(), ClientError> {
        if let Some(urls) = &options.urls {
            for url in urls {
                let body = serde_json::json!({
                    "type": "user/download",
                    "url": url,
                    "file_name": "",
                    "dir_id": "0",
                });
                self.post_json(
                    "/webman/3rdparty/pan-xunlei-com/index.cgi/device/create_task",
                    body,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn pause_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        for hash in hashes {
            let body = serde_json::json!({ "task_id": hash });
            let _ = self
                .post_json(
                    "/webman/3rdparty/pan-xunlei-com/index.cgi/device/pause_task",
                    body,
                )
                .await;
        }
        Ok(())
    }

    async fn resume_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        for hash in hashes {
            let body = serde_json::json!({ "task_id": hash });
            let _ = self
                .post_json(
                    "/webman/3rdparty/pan-xunlei-com/index.cgi/device/resume_task",
                    body,
                )
                .await;
        }
        Ok(())
    }

    async fn delete_torrents(
        &self,
        hashes: &[&str],
        delete_files: bool,
    ) -> Result<(), ClientError> {
        for hash in hashes {
            let body = serde_json::json!({ "task_id": hash, "delete_files": delete_files });
            let _ = self
                .post_json(
                    "/webman/3rdparty/pan-xunlei-com/index.cgi/device/delete_task",
                    body,
                )
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

    async fn create_category(
        &self,
        _name: &str,
        _save_path: Option<&str>,
    ) -> Result<(), ClientError> {
        Ok(())
    }

    async fn get_transfer_info(&self) -> Result<TransferInfo, ClientError> {
        Ok(TransferInfo {
            dl_speed: 0,
            up_speed: 0,
            free_space: 0,
        })
    }

    async fn get_torrent_files(&self, _hash: &str) -> Result<Vec<TorrentFile>, ClientError> {
        Ok(vec![])
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
