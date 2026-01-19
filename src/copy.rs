use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum CopyProgress {
    Counting,
    Started { total_bytes: u64, total_files: u64 },
    Progress { copied_bytes: u64, total_bytes: u64, current_file: String },
    Completed,
    Cancelled,
    Error(String),
}

/// Recursively collect all files in a directory (including hidden files)
fn collect_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files_recursive(dir, &mut files)?;
    Ok(files)
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, files)?;
            } else {
                files.push(path);
            }
        }
    }
    Ok(())
}

/// Calculate total size of all files
fn calculate_total_size(files: &[PathBuf]) -> u64 {
    files.iter()
        .filter_map(|f| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum()
}

/// Copy all files from source to destination with progress reporting
pub async fn copy_directory_with_progress(
    source_dir: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<CopyProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    crate::debug::log_section("Copy Files");
    crate::debug::log(&format!("Source: {:?}", source_dir));
    crate::debug::log(&format!("Destination: {:?}", dest_dir));

    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(CopyProgress::Cancelled);
        return Err("Copy cancelled".to_string());
    }

    let _ = progress_tx.send(CopyProgress::Counting);

    // Collect all files
    let files = collect_files(source_dir)
        .map_err(|e| format!("Failed to scan source directory: {}", e))?;

    let total_files = files.len() as u64;
    let total_bytes = calculate_total_size(&files);

    crate::debug::log(&format!("Found {} files, {} bytes total", total_files, total_bytes));

    let _ = progress_tx.send(CopyProgress::Started { total_bytes, total_files });

    // Ensure destination exists
    if !dest_dir.exists() {
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Failed to create destination directory: {}", e))?;
    }

    let mut copied_bytes: u64 = 0;

    for file_path in &files {
        // Check for cancellation
        if cancel_token.is_cancelled() {
            crate::debug::log("Copy cancelled by user");
            let _ = progress_tx.send(CopyProgress::Cancelled);
            return Err("Copy cancelled".to_string());
        }

        // Calculate relative path
        let relative_path = file_path.strip_prefix(source_dir)
            .map_err(|e| format!("Failed to get relative path: {}", e))?;

        let dest_path = dest_dir.join(relative_path);

        // Create parent directories if needed
        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
            }
        }

        // Get file size for progress
        let file_size = std::fs::metadata(file_path)
            .map(|m| m.len())
            .unwrap_or(0);

        // Send progress update with current file name
        let file_name = relative_path.to_string_lossy().to_string();
        let _ = progress_tx.send(CopyProgress::Progress {
            copied_bytes,
            total_bytes,
            current_file: file_name,
        });

        // Yield to allow UI to update
        tokio::task::yield_now().await;

        // Copy the file
        // On macOS, handle various permission/attribute issues when copying to FAT32
        #[cfg(target_os = "macos")]
        {
            match std::fs::copy(file_path, &dest_path) {
                Ok(_) => {},
                Err(e) if e.raw_os_error() == Some(1) => {
                    // Error 1 = "Operation not permitted"
                    crate::debug::log(&format!("Copy failed for {:?}, trying workarounds...", file_path.file_name().unwrap_or_default()));

                    // Try to fix source file permissions first
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(metadata) = std::fs::metadata(file_path) {
                            let mut perms = metadata.permissions();
                            perms.set_mode(0o644); // rw-r--r--
                            let _ = std::fs::set_permissions(file_path, perms);
                        }
                    }

                    // Fallback 1: Try standard copy again after permission fix
                    match std::fs::copy(file_path, &dest_path) {
                        Ok(_) => {
                            crate::debug::log("Copy succeeded after permission fix");
                        },
                        Err(_) => {
                            // Fallback 2: Manual read/write (no attributes preserved)
                            crate::debug::log("Trying manual read/write...");
                            match std::fs::read(file_path) {
                                Ok(contents) => {
                                    std::fs::write(&dest_path, contents)
                                        .map_err(|e2| {
                                            crate::debug::log(&format!("Manual write failed: {:?}", e2));
                                            format!("Failed to copy {:?}: original error: {}, write error: {}", file_path, e, e2)
                                        })?;
                                    crate::debug::log("Manual read/write succeeded");
                                },
                                Err(read_err) => {
                                    crate::debug::log(&format!("Manual read failed: {:?}", read_err));
                                    // Skip this file and continue
                                    crate::debug::log(&format!("SKIPPING file {:?} - cannot copy", file_path));
                                    continue;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("Failed to copy {:?}: {}", file_path, e));
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            std::fs::copy(file_path, &dest_path)
                .map_err(|e| format!("Failed to copy {:?}: {}", file_path, e))?;
        }

        copied_bytes += file_size;
    }

    // Final progress update
    let _ = progress_tx.send(CopyProgress::Progress {
        copied_bytes: total_bytes,
        total_bytes,
        current_file: String::new(),
    });

    crate::debug::log("Copy completed successfully");
    let _ = progress_tx.send(CopyProgress::Completed);

    Ok(())
}
