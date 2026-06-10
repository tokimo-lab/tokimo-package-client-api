#![allow(dead_code)]

use crate::error::ClientError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    pub supports_pause: bool,
    pub supports_file_priority: bool,
    pub supports_categories: bool,
    pub supports_torrent_file: bool,
    pub min_poll_interval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentFile {
    pub index: u32,
    pub name: String,
    pub size: u64,
    pub progress: f64,
    pub priority: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AddTorrentOptions {
    pub urls: Option<Vec<String>>,
    pub torrents: Option<Vec<String>>, // base64 encoded
    pub save_path: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub paused: Option<bool>,
    pub skip_hash_check: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryInfo {
    pub name: String,
    pub save_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferInfo {
    pub dl_speed: u64,
    pub up_speed: u64,
    pub free_space: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadClientType {
    QBittorrent,
    Transmission,
    Aria2,
    Deluge,
    RTorrent,
    Xunlei,
    Pan115,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentInfo {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub progress: f64,
    pub dl_speed: u64,
    pub up_speed: u64,
    pub downloaded: u64,
    pub uploaded: u64,
    pub ratio: f64,
    pub state: TorrentState,
    pub category: String,
    pub tags: Vec<String>,
    pub save_path: String,
    pub added_on: i64,
    pub completion_on: Option<i64>,
    pub seeding_time: Option<u64>,
    pub eta: Option<i64>,
    pub seeds: Option<u32>,
    pub peers: Option<u32>,
    pub tracker: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TorrentState {
    Downloading,
    Uploading,
    Seeding,
    PausedDl,
    PausedUp,
    QueuedDl,
    QueuedUp,
    CheckingDl,
    CheckingUp,
    StalledDl,
    StalledUp,
    Error,
    MissingFiles,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientStatus {
    pub connected: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

pub trait DownloadClient {
    fn client_type(&self) -> DownloadClientType;
    fn capabilities(&self) -> ClientCapabilities;

    fn test_connection(&self) -> impl std::future::Future<Output = Result<ClientStatus, ClientError>> + Send;
    fn get_torrents(
        &self,
        filter: Option<&str>,
        category: Option<&str>,
    ) -> impl std::future::Future<Output = Result<Vec<TorrentInfo>, ClientError>> + Send;
    fn get_torrent(
        &self,
        hash: &str,
    ) -> impl std::future::Future<Output = Result<Option<TorrentInfo>, ClientError>> + Send;
    fn add_torrent(
        &self,
        options: AddTorrentOptions,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
    fn pause_torrents(&self, hashes: &[&str]) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
    fn resume_torrents(&self, hashes: &[&str]) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
    fn delete_torrents(
        &self,
        hashes: &[&str],
        delete_files: bool,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
    fn set_category(
        &self,
        hashes: &[&str],
        category: &str,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
    fn get_categories(
        &self,
    ) -> impl std::future::Future<Output = Result<HashMap<String, CategoryInfo>, ClientError>> + Send;
    fn create_category(
        &self,
        name: &str,
        save_path: Option<&str>,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
    fn get_transfer_info(&self) -> impl std::future::Future<Output = Result<TransferInfo, ClientError>> + Send;
    fn get_torrent_files(
        &self,
        hash: &str,
    ) -> impl std::future::Future<Output = Result<Vec<TorrentFile>, ClientError>> + Send;
    fn set_file_priority(
        &self,
        hash: &str,
        file_ids: &[u32],
        priority: u8,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
}
