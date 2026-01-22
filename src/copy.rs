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
    #[allow(dead_code)]
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

        // Copy the file (std::fs::copy preserves permissions and timestamps)
        std::fs::copy(file_path, &dest_path)
            .map_err(|e| format!("Failed to copy {:?}: {}", file_path, e))?;

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
