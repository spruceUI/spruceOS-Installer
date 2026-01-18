use crate::config::{ASSET_EXTENSION, USER_AGENT};
use futures_util::StreamExt;
use serde::Deserialize;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
}

#[derive(Debug)]
pub enum DownloadProgress {
    Started { total_bytes: u64 },
    Progress { downloaded: u64, total: u64 },
    Completed,
    Cancelled,
    Error(String),
}

pub async fn get_latest_release(repo_url: &str) -> Result<Release, String> {
    let (owner, repo) = parse_github_url(repo_url)?;
    let api_url = format!("https://api.github.com/repos/{}/{}/releases/latest", owner, repo);

    let client = reqwest::Client::new();
    let response = client
        .get(&api_url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API error: {}", response.status()));
    }

    response
        .json::<Release>()
        .await
        .map_err(|e| format!("Failed to parse release: {}", e))
}

pub fn find_release_asset(release: &Release) -> Option<&Asset> {
    // Find the largest file with the matching extension
    // (handles cases where multiple files have the same extension)
    release.assets.iter()
        .filter(|a| a.name.ends_with(ASSET_EXTENSION))
        .max_by_key(|a| a.size)
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

    let client = reqwest::Client::new();
    let response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: {}", response.status()));
    }

    let total_size = response.content_length().unwrap_or(asset.size);
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
