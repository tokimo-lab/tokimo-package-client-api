#![allow(dead_code)]

use std::collections::HashMap;

use tracing::warn;

use super::traits::*;
use crate::error::ClientError;

#[derive(Debug, Clone)]
pub struct RTorrentConfig {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub http_client: reqwest::Client,
}

pub struct RTorrentClient {
    client: reqwest::Client,
    config: RTorrentConfig,
}

impl RTorrentClient {
    pub fn new(mut config: RTorrentConfig) -> Self {
        config.url = config.url.trim_end_matches('/').to_string();
        let client = config.http_client.clone();
        Self { client, config }
    }

    async fn xmlrpc_call(&self, xml: &str) -> Result<String, ClientError> {
        let url = format!("{}/RPC2", self.config.url);
        let mut builder = self
            .client
            .post(&url)
            .header("Content-Type", "text/xml")
            .body(xml.to_string());
        if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
            builder = builder.basic_auth(user, Some(pass));
        }
        let resp = builder.send().await?;
        Ok(resp.text().await?)
    }
}

fn strip_xml_tags(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result
}

fn xmlrpc_string(val: &str) -> String {
    let escaped = val
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<string>{escaped}</string>")
}

fn xmlrpc_base64(data: &str) -> String {
    format!("<base64>{data}</base64>")
}

fn build_call(method: &str, params: &[String]) -> String {
    let params_xml: String = params.iter().fold(String::new(), |mut acc, p| {
        use std::fmt::Write as _;
        let _ = write!(acc, "<param><value>{p}</value></param>");
        acc
    });
    format!(
        r#"<?xml version="1.0"?><methodCall><methodName>{method}</methodName><params>{params_xml}</params></methodCall>"#
    )
}

fn parse_single_value(xml: &str) -> Option<String> {
    let start = xml.find("<value>")? + 7;
    let remaining = &xml[start..];
    let end = remaining.find("</value>")?;
    let content = &remaining[..end];
    Some(strip_xml_tags(content).trim().to_string())
}

fn parse_nested_array(xml: &str) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    let parts: Vec<&str> = xml.split("<value><array><data>").collect();
    for part in parts.iter().skip(1) {
        let end = part.find("</data></array></value>").unwrap_or(part.len());
        let inner = &part[..end];
        let mut row: Vec<String> = Vec::new();
        let mut remaining = inner;
        while let Some(start) = remaining.find("<value>") {
            remaining = &remaining[start + 7..];
            if remaining.starts_with("<array>") {
                if let Some(skip_end) = remaining.find("</array>") {
                    remaining = &remaining[skip_end + 8..];
                }
                continue;
            }
            if let Some(end_tag) = remaining.find("</value>") {
                let content = &remaining[..end_tag];
                row.push(strip_xml_tags(content).trim().to_string());
                remaining = &remaining[end_tag + 8..];
            } else {
                break;
            }
        }
        if !row.is_empty() {
            result.push(row);
        }
    }
    result
}

const RTORRENT_FIELDS: &[&str] = &[
    "d.hash=",
    "d.name=",
    "d.size_bytes=",
    "d.completed_bytes=",
    "d.down.rate=",
    "d.up.rate=",
    "d.down.total=",
    "d.up.total=",
    "d.ratio=",
    "d.state=",
    "d.is_active=",
    "d.is_hash_checking=",
    "d.complete=",
    "d.directory=",
    "d.timestamp.started=",
    "d.timestamp.finished=",
    "d.peers_complete=",
    "d.peers_accounted=",
    "d.hashing=",
    "d.message=",
    "d.custom1=",
];

fn rtorrent_row_to_torrent_info(row: &[String]) -> Option<TorrentInfo> {
    if row.len() < 21 {
        return None;
    }
    let hash = row[0].clone();
    let name = row[1].clone();
    let size = row[2].parse::<u64>().unwrap_or(0);
    let completed = row[3].parse::<u64>().unwrap_or(0);
    let dl_speed = row[4].parse::<u64>().unwrap_or(0);
    let up_speed = row[5].parse::<u64>().unwrap_or(0);
    let downloaded = row[6].parse::<u64>().unwrap_or(0);
    let uploaded = row[7].parse::<u64>().unwrap_or(0);
    let ratio_int = row[8].parse::<i64>().unwrap_or(0);
    let ratio = ratio_int as f64 / 1000.0;
    let state = row[9].parse::<u8>().unwrap_or(0);
    let is_active = row[10].parse::<u8>().unwrap_or(0);
    let is_hash_checking = row[11].parse::<u8>().unwrap_or(0);
    let complete = row[12].parse::<u8>().unwrap_or(0);
    let save_path = row[13].clone();
    let added_on = row[14].parse::<i64>().unwrap_or(0);
    let finished_on = row[15].parse::<i64>().unwrap_or(0);
    let seeds = row[16].parse::<u32>().ok();
    let peers = row[17].parse::<u32>().ok();
    let hashing = row[18].parse::<u8>().unwrap_or(0);
    let message = row[19].clone();
    let category = row[20].clone();

    let progress = if size > 0 {
        completed as f64 / size as f64
    } else {
        0.0
    };
    let completion_on = if finished_on > 0 {
        Some(finished_on)
    } else {
        None
    };

    let torrent_state = if hashing != 0 {
        TorrentState::CheckingDl
    } else if !message.is_empty() && state == 0 {
        TorrentState::Error
    } else if state == 0 && is_active == 0 {
        TorrentState::PausedDl
    } else if state == 1 && is_hash_checking != 0 {
        TorrentState::CheckingDl
    } else if state == 1 && complete == 0 && is_active == 1 {
        TorrentState::Downloading
    } else if state == 1 && complete == 1 && is_active == 1 {
        TorrentState::Seeding
    } else if state == 1 && is_active == 0 && complete == 1 {
        TorrentState::PausedUp
    } else if state == 1 && is_active == 0 {
        TorrentState::PausedDl
    } else {
        TorrentState::Unknown
    };

    Some(TorrentInfo {
        hash,
        name,
        size,
        progress,
        dl_speed,
        up_speed,
        downloaded,
        uploaded,
        ratio,
        state: torrent_state,
        category,
        tags: vec![],
        save_path,
        added_on,
        completion_on,
        seeding_time: None,
        eta: None,
        seeds,
        peers,
        tracker: None,
    })
}

impl DownloadClient for RTorrentClient {
    fn client_type(&self) -> DownloadClientType {
        DownloadClientType::RTorrent
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
        let xml = build_call("system.listMethods", &[]);
        match self.xmlrpc_call(&xml).await {
            Ok(resp) => {
                let connected = resp.contains("<methodResponse>");
                Ok(ClientStatus {
                    connected,
                    version: None,
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
        let mut params = vec![xmlrpc_string(""), xmlrpc_string("main")];
        for field in RTORRENT_FIELDS {
            params.push(xmlrpc_string(field));
        }
        let xml = build_call("d.multicall2", &params);
        let resp = self.xmlrpc_call(&xml).await?;
        let rows = parse_nested_array(&resp);
        Ok(rows
            .iter()
            .filter_map(|row| rtorrent_row_to_torrent_info(row))
            .collect())
    }

    async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>, ClientError> {
        let torrents = self.get_torrents(None, None).await?;
        Ok(torrents
            .into_iter()
            .find(|t| t.hash.eq_ignore_ascii_case(hash)))
    }

    async fn add_torrent(&self, options: AddTorrentOptions) -> Result<(), ClientError> {
        if let Some(urls) = &options.urls {
            for url in urls {
                let xml = build_call(
                    "load.start_verbose",
                    &[xmlrpc_string(""), xmlrpc_string(url)],
                );
                self.xmlrpc_call(&xml).await?;
            }
        }
        if let Some(torrents) = &options.torrents {
            for torrent in torrents {
                let xml = build_call(
                    "load.raw_start_verbose",
                    &[xmlrpc_string(""), xmlrpc_base64(torrent)],
                );
                self.xmlrpc_call(&xml).await?;
            }
        }
        Ok(())
    }

    async fn pause_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        for hash in hashes {
            let xml = build_call("d.stop", &[xmlrpc_string(hash)]);
            self.xmlrpc_call(&xml).await?;
        }
        Ok(())
    }

    async fn resume_torrents(&self, hashes: &[&str]) -> Result<(), ClientError> {
        for hash in hashes {
            let xml = build_call("d.start", &[xmlrpc_string(hash)]);
            self.xmlrpc_call(&xml).await?;
        }
        Ok(())
    }

    async fn delete_torrents(
        &self,
        hashes: &[&str],
        delete_files: bool,
    ) -> Result<(), ClientError> {
        for hash in hashes {
            if delete_files {
                let xml = build_call("d.delete_tied", &[xmlrpc_string(hash)]);
                if let Err(e) = self.xmlrpc_call(&xml).await {
                    warn!("rtorrent xmlrpc d.delete_tied failed for {hash}: {e}");
                }
            }
            let xml = build_call("d.erase", &[xmlrpc_string(hash)]);
            self.xmlrpc_call(&xml).await?;
        }
        Ok(())
    }

    async fn set_category(&self, hashes: &[&str], category: &str) -> Result<(), ClientError> {
        for hash in hashes {
            let xml = build_call(
                "d.custom1.set",
                &[xmlrpc_string(hash), xmlrpc_string(category)],
            );
            self.xmlrpc_call(&xml).await?;
        }
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
        let dl_xml = build_call("throttle.global_down.rate", &[xmlrpc_string("")]);
        let up_xml = build_call("throttle.global_up.rate", &[xmlrpc_string("")]);
        let dl_resp = self.xmlrpc_call(&dl_xml).await.unwrap_or_default();
        let up_resp = self.xmlrpc_call(&up_xml).await.unwrap_or_default();
        let dl_speed = parse_single_value(&dl_resp)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let up_speed = parse_single_value(&up_resp)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        Ok(TransferInfo {
            dl_speed,
            up_speed,
            free_space: 0,
        })
    }

    async fn get_torrent_files(&self, hash: &str) -> Result<Vec<TorrentFile>, ClientError> {
        let xml = build_call(
            "f.multicall",
            &[
                xmlrpc_string(hash),
                xmlrpc_string(""),
                xmlrpc_string("f.path="),
                xmlrpc_string("f.size_bytes="),
                xmlrpc_string("f.completed_chunks="),
                xmlrpc_string("f.size_chunks="),
                xmlrpc_string("f.priority="),
            ],
        );
        let resp = self.xmlrpc_call(&xml).await?;
        let rows = parse_nested_array(&resp);
        Ok(rows
            .into_iter()
            .enumerate()
            .filter_map(|(i, row)| {
                if row.len() < 5 {
                    return None;
                }
                let path = row[0].clone();
                let size = row[1].parse::<u64>().unwrap_or(0);
                let completed_chunks = row[2].parse::<u64>().unwrap_or(0);
                let total_chunks = row[3].parse::<u64>().unwrap_or(1).max(1);
                let priority = row[4].parse::<i32>().unwrap_or(1);
                let progress = completed_chunks as f64 / total_chunks as f64;
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path)
                    .to_string();
                Some(TorrentFile {
                    index: i as u32,
                    name,
                    size,
                    progress,
                    priority,
                })
            })
            .collect())
    }

    async fn set_file_priority(
        &self,
        hash: &str,
        file_ids: &[u32],
        priority: u8,
    ) -> Result<(), ClientError> {
        for &id in file_ids {
            let file_key = format!("{hash}:f{id}");
            let priority_xml = format!("<i4>{priority}</i4>");
            let xml = build_call("f.priority.set", &[xmlrpc_string(&file_key), priority_xml]);
            self.xmlrpc_call(&xml).await?;
        }
        Ok(())
    }
}
