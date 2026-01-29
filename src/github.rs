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
    Paused { downloaded: u64, total: u64 },
    Resuming { downloaded: u64, total: u64 },
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
    pause_token: CancellationToken,
) -> Result<(), String> {
    use crate::download_state::DownloadState;

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

    // Check for existing partial download
    let existing_state = DownloadState::load(dest_path).ok();

    if let Some(ref state) = existing_state {
        // Verify URL matches
        if state.url == asset.browser_download_url {
            crate::debug::log(&format!(
                "Found partial download: {:.1}% complete ({} / {} bytes)",
                state.completion_percentage(),
                state.downloaded_bytes,
                state.total_size
            ));
            let _ = progress_tx.send(DownloadProgress::Resuming {
                downloaded: state.downloaded_bytes,
                total: state.total_size,
            });
        } else {
            crate::debug::log("Partial download URL mismatch - starting fresh");
            DownloadState::delete_state_file(dest_path);
        }
    }

    // Send HEAD request to check for Range support and get file size
    crate::debug::log("Checking if server supports parallel downloads (Range requests)...");
    let head_response = client
        .head(&asset.browser_download_url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("Failed to check download server capabilities: {}", e))?;

    // Get file size from HEAD response, fallback to asset.size if not available or zero
    let total_size = match head_response.content_length() {
        Some(size) if size > 0 => size,
        _ => {
            crate::debug::log(&format!("HEAD request returned invalid size, using asset.size: {}", asset.size));
            asset.size
        }
    };

    // Verify size matches if resuming
    if let Some(ref state) = existing_state {
        if state.total_size != total_size {
            crate::debug::log("File size mismatch - starting fresh");
            DownloadState::delete_state_file(dest_path);
        }
    }

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
    let result = if accepts_ranges && total_size > MIN_SIZE_FOR_PARALLEL {
        crate::debug::log("Server supports Range requests - using parallel chunked download (8 connections)");
        download_parallel(
            &client,
            &asset.browser_download_url,
            dest_path,
            total_size,
            progress_tx.clone(),
            cancel_token.clone(),
            pause_token.clone(),
            existing_state,
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
            progress_tx.clone(),
            cancel_token.clone(),
            pause_token.clone(),
        ).await
    };

    // Clean up state file on successful completion
    if result.is_ok() {
        DownloadState::delete_state_file(dest_path);
    }

    result
}

/// Download using parallel connections (8 chunks)
async fn download_parallel(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    total_size: u64,
    progress_tx: mpsc::UnboundedSender<DownloadProgress>,
    cancel_token: CancellationToken,
    pause_token: CancellationToken,
    existing_state: Option<crate::download_state::DownloadState>,
) -> Result<(), String> {
    use crate::download_state::DownloadState;

    const NUM_CHUNKS: u64 = 8;

    // Use existing state or create new
    let mut state = existing_state.unwrap_or_else(|| {
        DownloadState::new(url.to_string(), total_size, dest_path.to_path_buf(), NUM_CHUNKS)
    });

    // Create or open file for writing
    if !dest_path.exists() {
        let file = std::fs::File::create(dest_path)
            .map_err(|e| format!("Failed to create file: {}", e))?;
        file.set_len(total_size)
            .map_err(|e| format!("Failed to allocate file space: {}", e))?;
    }

    // Shared progress tracking
    let downloaded = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(state.downloaded_bytes));

    // Get incomplete chunks to download
    let incomplete_chunks = state.get_incomplete_chunks();
    if incomplete_chunks.is_empty() {
        let _ = progress_tx.send(DownloadProgress::Completed);
        return Ok(());
    }

    let _ = progress_tx.send(DownloadProgress::Started { total_bytes: total_size });

    crate::debug::log(&format!("Downloading {} incomplete chunks", incomplete_chunks.len()));

    // Spawn chunk download tasks for incomplete chunks only
    let mut tasks = Vec::new();
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(state));

    for &chunk_index in &incomplete_chunks {
        let chunk = state.lock().await.chunks[chunk_index].clone();

        let client = client.clone();
        let url = url.to_string();
        let dest_path = dest_path.to_path_buf();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();
        let pause_token = pause_token.clone();
        let downloaded = downloaded.clone();
        let state = state.clone();

        let task = tokio::spawn(async move {
            let result = download_chunk(
                &client,
                &url,
                &dest_path,
                chunk.start,
                chunk.end,
                total_size,
                progress_tx,
                cancel_token,
                pause_token,
                downloaded,
            ).await;

            // Mark chunk as complete if successful
            if result.is_ok() {
                let chunk_size = chunk.end - chunk.start + 1;
                state.lock().await.mark_chunk_complete(chunk_index, chunk_size);
            }

            result
        });

        tasks.push((chunk_index, task));
    }

    // Wait for all chunks to complete or pause/cancel
    let mut paused = false;
    for (chunk_index, task) in tasks {
        match task.await {
            Ok(Ok(())) => {
                // Chunk completed successfully
            }
            Ok(Err(e)) => {
                if e.contains("paused") {
                    paused = true;
                    crate::debug::log("Download paused");
                    break;
                } else if e.contains("cancelled") {
                    return Err("Download cancelled".to_string());
                } else {
                    return Err(format!("Chunk {} download failed: {}", chunk_index, e));
                }
            }
            Err(e) => {
                return Err(format!("Chunk {} task failed: {}", chunk_index, e));
            }
        }
    }

    if paused {
        // Save state and return paused status
        let final_state = state.lock().await;
        final_state.save(dest_path)?;
        let _ = progress_tx.send(DownloadProgress::Paused {
            downloaded: final_state.downloaded_bytes,
            total: total_size,
        });
        return Err("Download paused".to_string());
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
    pause_token: CancellationToken,
    downloaded: std::sync::Arc<std::sync::atomic::AtomicU64>,
) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};

    if cancel_token.is_cancelled() {
        return Err("Download cancelled".to_string());
    }

    if pause_token.is_cancelled() {
        return Err("Download paused".to_string());
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

    // Open file for writing at correct position
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(dest_path)
        .map_err(|e| format!("Failed to open file for writing chunk: {}", e))?;

    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("Failed to seek to chunk position: {}", e))?;

    // Stream the chunk and report progress as we go
    let mut stream = response.bytes_stream();
    let mut chunk_bytes_written = 0u64;

    while let Some(chunk_result) = stream.next().await {
        if cancel_token.is_cancelled() {
            return Err("Download cancelled".to_string());
        }

        if pause_token.is_cancelled() {
            return Err("Download paused".to_string());
        }

        let chunk = chunk_result
            .map_err(|e| format!("Failed to download chunk data: {}", e))?;

        file.write_all(&chunk)
            .map_err(|e| format!("Failed to write chunk data: {}", e))?;

        chunk_bytes_written += chunk.len() as u64;

        // Update global progress
        let total_downloaded = downloaded.fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed) + chunk.len() as u64;
        let _ = progress_tx.send(DownloadProgress::Progress {
            downloaded: total_downloaded,
            total: total_size,
        });
    }

    file.flush()
        .map_err(|e| format!("Failed to flush chunk data: {}", e))?;

    crate::debug::log(&format!("Chunk complete: bytes {}-{} ({} bytes written)", start, end, chunk_bytes_written));
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
    pause_token: CancellationToken,
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
            _ = pause_token.cancelled() => {
                // Flush and keep partial file for resume
                let _ = file.flush().await;
                let _ = progress_tx.send(DownloadProgress::Paused {
                    downloaded,
                    total: total_size,
                });
                return Err("Download paused".to_string());
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
