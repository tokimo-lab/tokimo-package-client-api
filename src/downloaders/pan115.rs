#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;

use super::traits::*;
use crate::error::ClientError;

#[derive(Debug, Clone)]
pub struct Pan115Config {
    pub url: Option<String>,
    pub cookies: String,
    pub http_client: reqwest::Client,
}

impl Pan115Config {
    fn base_url(&self) -> &str {
        self.url
            .as_deref()
            .unwrap_or("https://lixian.115.com")
            .trim_end_matches('/')
    }
}

pub struct Pan115Client {
    client: reqwest::Client,
    config: Pan115Config,
}

impl Pan115Client {
    pub fn new(config: Pan115Config) -> Self {
        let client = config.http_client.clone();
        Self { client, config }
    }

    async fn get_tasks_page(&self, page: u32) -> Result<serde_json::Value, ClientError> {
        let url = format!(
            "{}/lixian/?ct=lixian&ac=task_lists&page={page}&page_size=40",
            self.config.base_url()
        );
        let resp = self
            .client
            .get(&url)
            .header("Cookie", &self.config.cookies)
            .send()
            .await?;
        Ok(resp.json().await?)
    }
}

#[derive(Debug, Deserialize)]
struct Pan115Task {
    info_hash: String,
    name: String,
    #[serde(rename = "size")]
    file_size: Option<u64>,
    pct: u64,
    status: u8,
    peers: Option<u32>,
    speed: Option<u64>,
    #[serde(rename = "rtime")]
    add_time: Option<i64>,
    file_id: Option<String>,
}

fn map_pan115_status(status: u8) -> TorrentState {
    match status {
        1 => TorrentState::Downloading,
        2 => TorrentState::Seeding,
        3 => TorrentState::PausedDl,
        4 => TorrentState::QueuedDl,
        5 => TorrentState::Error,
        _ => TorrentState::Unknown,
    }
}

fn pan115_task_to_torrent_info(task: Pan115Task) -> TorrentInfo {
    let progress = task.pct as f64 / 10000.0;
    let size = task.file_size.unwrap_or(0);
    TorrentInfo {
        hash: task.info_hash,
        name: task.name,
        size,
        progress,
        dl_speed: task.speed.unwrap_or(0),
        up_speed: 0,
        downloaded: (size as f64 * progress) as u64,
        uploaded: 0,
        ratio: 0.0,
        state: map_pan115_status(task.status),
        category: String::new(),
        tags: vec![],
        save_path: String::new(),
        added_on: task.add_time.unwrap_or(0),
        completion_on: None,
        seeding_time: None,
        eta: None,
        seeds: None,
        peers: task.peers,
        tracker: None,
    }
}

impl DownloadClient for Pan115Client {
    fn client_type(&self) -> DownloadClientType {
        DownloadClientType::Pan115
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            supports_pause: false,
            supports_file_priority: false,
            supports_categories: false,
            supports_torrent_file: true,
            min_poll_interval: 10,
        }
    }

    async fn test_connection(&self) -> Result<ClientStatus, ClientError> {
        match self.get_tasks_page(1).await {
            Ok(v) => {
                let ok = v.get("state").and_then(serde_json::Value::as_bool).unwrap_or(false);
                if ok {
                    Ok(ClientStatus {
                        connected: true,
                        version: None,
                        error: None,
                    })
                } else {
                    Ok(ClientStatus {
                        connected: false,
                        version: None,
                        error: Some("Pan115 returned state=false".to_string()),
                    })
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
        _filter: Option<&str>,
        _category: Option<&str>,
    ) -> Result<Vec<TorrentInfo>, ClientError> {
        let mut page = 1u32;
        let mut all_tasks: Vec<TorrentInfo> = Vec::new();
        loop {
            let data = self.get_tasks_page(page).await?;
            let tasks_val = data
                .get("tasks")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let count = tasks_val.len();
            for t in tasks_val {
                if let Ok(task) = serde_json::from_value::<Pan115Task>(t) {
                    all_tasks.push(pan115_task_to_torrent_info(task));
                }
            }
            if count < 40 {
                break;
            }
            page += 1;
        }
        Ok(all_tasks)
    }

    async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>, ClientError> {
        let torrents = self.get_torrents(None, None).await?;
        Ok(torrents.into_iter().find(|t| t.hash.eq_ignore_ascii_case(hash)))
    }

    async fn add_torrent(&self, options: AddTorrentOptions) -> Result<(), ClientError> {
        if let Some(urls) = &options.urls {
            if urls.is_empty() {
                return Ok(());
            }
            let url = format!("{}/lixian/?ct=lixian&ac=add_task_urls", self.config.base_url());
            let mut form: Vec<(String, String)> = Vec::new();
            for (i, task_url) in urls.iter().enumerate() {
                form.push((format!("url[{i}]"), task_url.clone()));
            }
            let form_refs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            self.client
                .post(&url)
                .header("Cookie", &self.config.cookies)
                .form(form_refs.as_slice())
                .send()
                .await?;
        }
        Ok(())
    }

    async fn pause_torrents(&self, _hashes: &[&str]) -> Result<(), ClientError> {
        Ok(())
    }

    async fn resume_torrents(&self, _hashes: &[&str]) -> Result<(), ClientError> {
        Ok(())
    }

    async fn delete_torrents(&self, hashes: &[&str], delete_files: bool) -> Result<(), ClientError> {
        if hashes.is_empty() {
            return Ok(());
        }
        let url = format!("{}/lixian/?ct=lixian&ac=task_del", self.config.base_url());
        let mut form: Vec<(String, String)> = Vec::new();
        for (i, hash) in hashes.iter().enumerate() {
            form.push((format!("hash[{i}]"), hash.to_string()));
        }
        form.push((
            "delete_file".to_string(),
            if delete_files { "1".to_string() } else { "0".to_string() },
        ));
        let form_refs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        self.client
            .post(&url)
            .header("Cookie", &self.config.cookies)
            .form(form_refs.as_slice())
            .send()
            .await?;
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
        Ok(TransferInfo {
            dl_speed: 0,
            up_speed: 0,
            free_space: 0,
        })
    }

    async fn get_torrent_files(&self, _hash: &str) -> Result<Vec<TorrentFile>, ClientError> {
        Ok(vec![])
    }

    async fn set_file_priority(&self, _hash: &str, _file_ids: &[u32], _priority: u8) -> Result<(), ClientError> {
        Ok(())
    }
}
