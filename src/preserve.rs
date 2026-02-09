// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

use std::path::Path;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum PreserveProgress {
    Started { total_paths: usize },
    BackingUp { path: String },
    Restoring { path: String },
    Completed,
    Cancelled,
    #[allow(dead_code)]
    Error(String),
}

/// Recursively copy a file or directory from src to dst, preserving relative structure.
async fn copy_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        tokio::fs::create_dir_all(dst).await
            .map_err(|e| format!("Failed to create directory {:?}: {}", dst, e))?;

        let mut entries = tokio::fs::read_dir(src).await
            .map_err(|e| format!("Failed to read directory {:?}: {}", src, e))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| format!("Failed to read entry in {:?}: {}", src, e))? {
            let entry_src = entry.path();
            let entry_dst = dst.join(entry.file_name());
            Box::pin(copy_recursive(&entry_src, &entry_dst)).await?;
        }
    } else if src.is_file() {
        // Ensure parent directory exists
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| format!("Failed to create parent dir {:?}: {}", parent, e))?;
        }
        tokio::fs::copy(src, dst).await
            .map_err(|e| format!("Failed to copy {:?} to {:?}: {}", src, dst, e))?;
    }
    // Skip symlinks and other special files
    Ok(())
}

/// Backup preserve paths from the SD card to a local temp directory.
/// Each path in `preserve_paths` is relative to `mount_path`.
/// Files are copied to `temp_backup_dir` preserving their relative path structure.
pub async fn backup_preserve_paths(
    mount_path: &Path,
    preserve_paths: &[&str],
    temp_backup_dir: &Path,
    progress_tx: mpsc::UnboundedSender<PreserveProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(PreserveProgress::Cancelled);
        return Err("Backup cancelled".to_string());
    }

    let _ = progress_tx.send(PreserveProgress::Started {
        total_paths: preserve_paths.len(),
    });

    crate::debug::log_section("Backing Up User Data");
    crate::debug::log(&format!("Mount path: {:?}", mount_path));
    crate::debug::log(&format!("Backup dir: {:?}", temp_backup_dir));

    // Create the backup directory
    tokio::fs::create_dir_all(temp_backup_dir).await
        .map_err(|e| format!("Failed to create backup directory: {}", e))?;

    for rel_path in preserve_paths {
        if cancel_token.is_cancelled() {
            let _ = progress_tx.send(PreserveProgress::Cancelled);
            return Err("Backup cancelled".to_string());
        }

        let src = mount_path.join(rel_path);
        let dst = temp_backup_dir.join(rel_path);

        if !src.exists() {
            crate::debug::log(&format!("Path does not exist, skipping: {}", rel_path));
            continue;
        }

        let _ = progress_tx.send(PreserveProgress::BackingUp {
            path: rel_path.to_string(),
        });

        crate::debug::log(&format!("Backing up: {}", rel_path));

        copy_recursive(&src, &dst).await?;
    }

    let _ = progress_tx.send(PreserveProgress::Completed);
    crate::debug::log("User data backup complete");

    Ok(())
}

/// Restore preserved paths from the local temp backup directory back to the SD card.
/// Walks the backup directory recursively and copies all files back to `mount_path`
/// at their original relative paths, overwriting any existing files.
/// Cleans up `temp_backup_dir` when done.
pub async fn restore_preserve_paths(
    mount_path: &Path,
    temp_backup_dir: &Path,
    progress_tx: mpsc::UnboundedSender<PreserveProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(PreserveProgress::Cancelled);
        return Err("Restore cancelled".to_string());
    }

    // Count entries for progress reporting
    let paths = collect_relative_files(temp_backup_dir, temp_backup_dir).await?;

    let _ = progress_tx.send(PreserveProgress::Started {
        total_paths: paths.len(),
    });

    crate::debug::log_section("Restoring User Data");
    crate::debug::log(&format!("Backup dir: {:?}", temp_backup_dir));
    crate::debug::log(&format!("Mount path: {:?}", mount_path));
    crate::debug::log(&format!("Files to restore: {}", paths.len()));

    for rel_path in &paths {
        if cancel_token.is_cancelled() {
            let _ = progress_tx.send(PreserveProgress::Cancelled);
            return Err("Restore cancelled".to_string());
        }

        let src = temp_backup_dir.join(rel_path);
        let dst = mount_path.join(rel_path);

        let _ = progress_tx.send(PreserveProgress::Restoring {
            path: rel_path.clone(),
        });

        crate::debug::log(&format!("Restoring: {}", rel_path));

        // Ensure parent directory exists
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| format!("Failed to create parent dir {:?}: {}", parent, e))?;
        }

        tokio::fs::copy(&src, &dst).await
            .map_err(|e| format!("Failed to restore {:?}: {}", rel_path, e))?;
    }

    // Clean up backup directory
    crate::debug::log("Cleaning up backup directory...");
    let _ = tokio::fs::remove_dir_all(temp_backup_dir).await;

    let _ = progress_tx.send(PreserveProgress::Completed);
    crate::debug::log("User data restore complete");

    Ok(())
}

/// Recursively collect all file paths relative to `base` within `dir`.
async fn collect_relative_files(dir: &Path, base: &Path) -> Result<Vec<String>, String> {
    let mut results = Vec::new();

    if !dir.exists() {
        return Ok(results);
    }

    let mut entries = tokio::fs::read_dir(dir).await
        .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?;

    while let Some(entry) = entries.next_entry().await
        .map_err(|e| format!("Failed to read entry in {:?}: {}", dir, e))? {
        let path = entry.path();

        if path.is_dir() {
            let sub_results = Box::pin(collect_relative_files(&path, base)).await?;
            results.extend(sub_results);
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                results.push(rel.to_string_lossy().to_string());
            }
        }
    }

    Ok(results)
}
