// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

use crate::config::USER_AGENT;
use crate::manifest::Manifest;
use futures_util::StreamExt;
use serde::Deserialize;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize, Clone)]
pub struct Release {
    pub tag_name: String,
    #[allow(dead_code)]
    pub name: Option<String>,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,

    // Optional fields populated from manifest.json (not present in GitHub API responses)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub display_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub devices: Option<String>,
}

#[derive(Debug)]
pub enum DownloadProgress {
    Started { total_bytes: u64 },
    Progress { downloaded: u64, total: u64 },
    Completed,
    Cancelled,
    #[allow(dead_code)]
    Error(String),
}

pub async fn get_latest_release(repo_url: &str) -> Result<Release, String> {
    let (owner, repo) = parse_github_url(repo_url)?;
    let api_url = format!("https://api.github.com/repos/{}/{}/releases/latest", owner, repo);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(&api_url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "Connection timed out. Please check your internet connection and try again.".to_string()
            } else if e.is_connect() {
                "Cannot reach GitHub. Please check your internet connection and firewall settings.".to_string()
            } else {
                format!("Failed to fetch release: {}", e)
            }
        })?;

    // Check for rate limiting (HTTP 403)
    if response.status() == 403 {
        return Err("GitHub API rate limit exceeded. Please wait an hour and try again, or check your internet connection.".to_string());
    }

    if !response.status().is_success() {
        return Err(format!("GitHub API returned error: {}. Please try again later.", response.status()));
    }

    response
        .json::<Release>()
        .await
        .map_err(|e| format!("Failed to parse release data: {}. The release format may be invalid.", e))
}

/// Check if a release contains a manifest.json file and fetch it
/// Returns Some(Manifest) if found and successfully parsed, None otherwise
pub async fn get_manifest_from_release(release: &Release) -> Option<Manifest> {
    // Look for manifest.json in the release assets
    let manifest_asset = release.assets.iter()
        .find(|asset| asset.name.eq_ignore_ascii_case("manifest.json"))?;

    crate::debug::log("Found manifest.json in release, fetching...");
    crate::debug::log(&format!("Manifest URL: {}", manifest_asset.browser_download_url));

    // Fetch the manifest file
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .ok()?;

    let response = client
        .get(&manifest_asset.browser_download_url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        crate::debug::log(&format!("Failed to fetch manifest: HTTP {}", response.status()));
        return None;
    }

    // Parse the JSON
    let manifest_text = response.text().await.ok()?;
    crate::debug::log(&format!("Manifest content length: {} bytes", manifest_text.len()));

    match serde_json::from_str::<Manifest>(&manifest_text) {
        Ok(manifest) => {
            crate::debug::log(&format!("Manifest parsed successfully: {} assets found", manifest.assets.len()));
            Some(manifest)
        }
        Err(e) => {
            crate::debug::log(&format!("Failed to parse manifest JSON: {}", e));
            None
        }
    }
}

/// Convert a ManifestAsset to an Asset structure
/// This allows manifest-based assets to work with the existing installation pipeline
impl From<crate::manifest::ManifestAsset> for Asset {
    fn from(manifest_asset: crate::manifest::ManifestAsset) -> Self {
        Asset {
            name: manifest_asset.name,
            size: manifest_asset.size,
            browser_download_url: manifest_asset.url,
            display_name: manifest_asset.display_name,
            devices: manifest_asset.devices,
        }
    }
}

pub async fn download_asset(
    asset: &Asset,
    dest_path: &Path,
    progress_tx: mpsc::UnboundedSender<DownloadProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(DownloadProgress::Cancelled);
        return Err("Download cancelled".to_string());
    }

    // Create client with connection timeout (but no overall timeout for large downloads)
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Send HEAD request to check for Range support and get file size
    crate::debug::log("Checking if server supports parallel downloads (Range requests)...");
    let head_response = client
        .head(&asset.browser_download_url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("Failed to check download server capabilities: {}", e))?;

    let total_size = head_response.content_length().unwrap_or(asset.size);
    let accepts_ranges = head_response
        .headers()
        .get("accept-ranges")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "bytes")
        .unwrap_or(false);

    let size_mb = total_size as f64 / 1_048_576.0;
    crate::debug::log(&format!("Download size: {:.1} MB ({} bytes)", size_mb, total_size));

    // Use parallel download if server supports Range requests and file is large enough
    const MIN_SIZE_FOR_PARALLEL: u64 = 10 * 1024 * 1024; // 10 MB
    if accepts_ranges && total_size > MIN_SIZE_FOR_PARALLEL {
        crate::debug::log("Server supports Range requests - using parallel chunked download (8 connections)");
        download_parallel(
            &client,
            &asset.browser_download_url,
            dest_path,
            total_size,
            progress_tx,
            cancel_token,
        ).await
    } else {
        if !accepts_ranges {
            crate::debug::log("Server doesn't support Range requests - using single-connection download");
        } else {
            crate::debug::log("File too small for parallel download - using single-connection download");
        }
        download_single(
            &client,
            &asset.browser_download_url,
            dest_path,
            total_size,
            progress_tx,
            cancel_token,
        ).await
    }
}

/// Download using parallel connections (8 chunks)
async fn download_parallel(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    total_size: u64,
    progress_tx: mpsc::UnboundedSender<DownloadProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    const NUM_CHUNKS: u64 = 8;
    let chunk_size = total_size / NUM_CHUNKS;

    let _ = progress_tx.send(DownloadProgress::Started { total_bytes: total_size });

    // Create file and pre-allocate space
    let file = std::fs::File::create(dest_path)
        .map_err(|e| format!("Failed to create file: {}", e))?;
    file.set_len(total_size)
        .map_err(|e| format!("Failed to allocate file space: {}", e))?;
    drop(file);

    // Shared progress tracking
    let downloaded = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Spawn chunk download tasks
    let mut tasks = Vec::new();
    for i in 0..NUM_CHUNKS {
        let start = i * chunk_size;
        let end = if i == NUM_CHUNKS - 1 {
            total_size - 1 // Last chunk gets remainder
        } else {
            (i + 1) * chunk_size - 1
        };

        let client = client.clone();
        let url = url.to_string();
        let dest_path = dest_path.to_path_buf();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();
        let downloaded = downloaded.clone();

        let task = tokio::spawn(async move {
            download_chunk(
                &client,
                &url,
                &dest_path,
                start,
                end,
                total_size,
                progress_tx,
                cancel_token,
                downloaded,
            ).await
        });

        tasks.push(task);
    }

    // Wait for all chunks to complete
    for (i, task) in tasks.into_iter().enumerate() {
        task.await
            .map_err(|e| format!("Chunk {} task failed: {}", i, e))?
            .map_err(|e| format!("Chunk {} download failed: {}", i, e))?;
    }

    let _ = progress_tx.send(DownloadProgress::Completed);
    crate::debug::log("Parallel download complete");
    Ok(())
}

/// Download a single chunk of the file
async fn download_chunk(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    start: u64,
    end: u64,
    total_size: u64,
    progress_tx: mpsc::UnboundedSender<DownloadProgress>,
    cancel_token: CancellationToken,
    downloaded: std::sync::Arc<std::sync::atomic::AtomicU64>,
) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};

    if cancel_token.is_cancelled() {
        return Err("Download cancelled".to_string());
    }

    let range_header = format!("bytes={}-{}", start, end);
    crate::debug::log(&format!("Downloading chunk: {}", range_header));

    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Range", range_header)
        .send()
        .await
        .map_err(|e| format!("Failed to start chunk download: {}", e))?;

    if !response.status().is_success() && response.status() != 206 {
        return Err(format!("Chunk download failed with status: {}", response.status()));
    }

    let bytes = response.bytes().await
        .map_err(|e| format!("Failed to download chunk data: {}", e))?;

    // Write chunk to file at correct position
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(dest_path)
        .map_err(|e| format!("Failed to open file for writing chunk: {}", e))?;

    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("Failed to seek to chunk position: {}", e))?;

    file.write_all(&bytes)
        .map_err(|e| format!("Failed to write chunk data: {}", e))?;

    // Update progress
    let chunk_downloaded = downloaded.fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed) + bytes.len() as u64;
    let _ = progress_tx.send(DownloadProgress::Progress {
        downloaded: chunk_downloaded,
        total: total_size,
    });

    crate::debug::log(&format!("Chunk complete: bytes {}-{}", start, end));
    Ok(())
}

/// Fallback: Download using single connection
async fn download_single(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    total_size: u64,
    progress_tx: mpsc::UnboundedSender<DownloadProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "Connection timed out while starting download. Please check your internet connection.".to_string()
            } else if e.is_connect() {
                "Cannot reach download server. Please check your internet connection and firewall settings.".to_string()
            } else {
                format!("Failed to start download: {}", e)
            }
        })?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status {}: Please try again later.", response.status()));
    }

    let _ = progress_tx.send(DownloadProgress::Started { total_bytes: total_size });

    let mut file = File::create(dest_path)
        .await
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                // Clean up partial file
                drop(file);
                let _ = tokio::fs::remove_file(dest_path).await;
                let _ = progress_tx.send(DownloadProgress::Cancelled);
                return Err("Download cancelled".to_string());
            }
            chunk_result = stream.next() => {
                match chunk_result {
                    Some(Ok(chunk)) => {
                        file.write_all(&chunk)
                            .await
                            .map_err(|e| format!("Write error: {}", e))?;

                        downloaded += chunk.len() as u64;
                        let _ = progress_tx.send(DownloadProgress::Progress {
                            downloaded,
                            total: total_size,
                        });
                    }
                    Some(Err(e)) => {
                        return Err(format!("Download error: {}", e));
                    }
                    None => {
                        // Stream complete
                        break;
                    }
                }
            }
        }
    }

    file.flush().await.map_err(|e| format!("Flush error: {}", e))?;
    let _ = progress_tx.send(DownloadProgress::Completed);

    Ok(())
}

fn parse_github_url(url: &str) -> Result<(String, String), String> {
    // Handle various GitHub URL formats:
    // https://github.com/owner/repo
    // https://github.com/owner/repo.git
    // owner/repo

    let url = url.trim();

    // Remove .git suffix if present
    let url = url.strip_suffix(".git").unwrap_or(url);

    // Try to extract owner/repo from full URL
    if url.contains("github.com") {
        let parts: Vec<&str> = url.split("github.com").collect();
        if parts.len() >= 2 {
            let path = parts[1].trim_start_matches('/').trim_start_matches(':');
            let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            if segments.len() >= 2 {
                return Ok((segments[0].to_string(), segments[1].to_string()));
            }
        }
    }

    // Try owner/repo format
    let segments: Vec<&str> = url.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() == 2 {
        return Ok((segments[0].to_string(), segments[1].to_string()));
    }

    Err("Invalid GitHub repository URL. Use format: owner/repo or https://github.com/owner/repo".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url() {
        assert_eq!(
            parse_github_url("https://github.com/owner/repo").unwrap(),
            ("owner".to_string(), "repo".to_string())
        );
        assert_eq!(
            parse_github_url("https://github.com/owner/repo.git").unwrap(),
            ("owner".to_string(), "repo".to_string())
        );
        assert_eq!(
            parse_github_url("owner/repo").unwrap(),
            ("owner".to_string(), "repo".to_string())
        );
    }
}
