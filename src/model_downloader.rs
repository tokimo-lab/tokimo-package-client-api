use std::path::Path;

use futures_util::StreamExt;

use crate::error::ClientError;

/// Progress callback: (label, status, percent 0-100, downloaded_bytes, total_bytes)
pub type ProgressFn = Box<dyn Fn(&str, &str, u8, u64, u64) + Send + Sync>;

pub struct ModelDownloader {
    http: reqwest::Client,
}

impl ModelDownloader {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    pub async fn download_file(
        &self,
        url: &str,
        dest: &str,
        label: &str,
        on_progress: &Option<ProgressFn>,
    ) -> Result<(), ClientError> {
        let parent = Path::new(dest)
            .parent()
            .ok_or_else(|| ClientError::Other("Invalid path".into()))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ClientError::Other(format!("mkdir failed: {e}")))?;

        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: format!("HTTP {}: {}", resp.status(), url),
            });
        }

        let total_size = resp.content_length().unwrap_or(0);
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last_pct: u8 = 0;
        let mut buf = Vec::with_capacity(total_size as usize);

        if let Some(cb) = on_progress.as_ref() {
            cb(label, "downloading", 0, 0, total_size);
        }

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ClientError::Other(format!("Download stream error: {e}")))?;
            downloaded += chunk.len() as u64;
            buf.extend_from_slice(&chunk);

            if let Some(cb) = on_progress.as_ref() {
                let pct = if total_size > 0 {
                    ((downloaded as f64 / total_size as f64) * 100.0).min(100.0) as u8
                } else {
                    0
                };
                if pct != last_pct {
                    last_pct = pct;
                    cb(label, "downloading", pct, downloaded, total_size);
                }
            }
        }

        tokio::fs::write(dest, &buf)
            .await
            .map_err(|e| ClientError::Other(format!("Write failed: {e}")))?;

        let size_mb = buf.len() as f64 / (1024.0 * 1024.0);
        tracing::info!("  Done: {size_mb:.1} MB");

        if let Some(cb) = on_progress.as_ref() {
            cb(label, "ready", 100, downloaded, total_size);
        }

        Ok(())
    }

    pub async fn download_and_extract_zip(
        &self,
        url: &str,
        dest_dir: &str,
        label: &str,
        on_progress: &Option<ProgressFn>,
    ) -> Result<(), ClientError> {
        tokio::fs::create_dir_all(dest_dir)
            .await
            .map_err(|e| ClientError::Other(format!("mkdir failed: {e}")))?;

        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: format!("HTTP {}: {}", resp.status(), url),
            });
        }

        let total_size = resp.content_length().unwrap_or(0);
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last_pct: u8 = 0;
        let mut buf = Vec::with_capacity(total_size as usize);

        if let Some(cb) = on_progress.as_ref() {
            cb(label, "downloading", 0, 0, total_size);
        }

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ClientError::Other(format!("Download stream error: {e}")))?;
            downloaded += chunk.len() as u64;
            buf.extend_from_slice(&chunk);

            if let Some(cb) = on_progress.as_ref() {
                let pct = if total_size > 0 {
                    ((downloaded as f64 / total_size as f64) * 100.0).min(100.0) as u8
                } else {
                    0
                };
                if pct != last_pct {
                    last_pct = pct;
                    cb(label, "downloading", pct, downloaded, total_size);
                }
            }
        }

        let size_mb = buf.len() as f64 / (1024.0 * 1024.0);
        tracing::info!("  Downloaded: {size_mb:.1} MB, extracting...");

        if let Some(cb) = on_progress.as_ref() {
            cb(label, "extracting", 0, downloaded, total_size);
        }

        let cursor = std::io::Cursor::new(buf);
        let dest_dir_owned = dest_dir.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), ClientError> {
            let mut archive = zip::ZipArchive::new(cursor)
                .map_err(|e| ClientError::Other(format!("ZIP open failed: {e}")))?;

            for i in 0..archive.len() {
                let mut file = archive
                    .by_index(i)
                    .map_err(|e| ClientError::Other(format!("ZIP entry error: {e}")))?;
                let name = file.name().to_string();
                if !std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("onnx"))
                {
                    continue;
                }
                let out_path = format!("{dest_dir_owned}/{name}");
                if let Some(parent) = Path::new(&out_path).parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ClientError::Other(format!("mkdir failed: {e}")))?;
                }
                let mut out_file = std::fs::File::create(&out_path)
                    .map_err(|e| ClientError::Other(format!("File create failed: {e}")))?;
                std::io::copy(&mut file, &mut out_file)
                    .map_err(|e| ClientError::Other(format!("Extract failed: {e}")))?;
                tracing::info!("  Extracted: {name}");
            }
            Ok(())
        })
        .await
        .map_err(|e| ClientError::Other(format!("zip extract join failed: {e}")))??;

        Ok(())
    }
}
