use sevenz_rust::decompress_file;
use std::path::Path;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum ExtractProgress {
    Started,
    Extracting,
    Completed,
    Error(String),
}

pub async fn extract_7z(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<ExtractProgress>,
) -> Result<(), String> {
    let archive_path = archive_path.to_path_buf();
    let dest_dir = dest_dir.to_path_buf();
    let progress_tx_clone = progress_tx.clone();

    // Run extraction in a blocking task since sevenz-rust is synchronous
    tokio::task::spawn_blocking(move || {
        extract_7z_sync(&archive_path, &dest_dir, progress_tx_clone)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

fn extract_7z_sync(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<ExtractProgress>,
) -> Result<(), String> {
    let _ = progress_tx.send(ExtractProgress::Started);

    // Verify archive exists
    if !archive_path.exists() {
        return Err(format!("Archive not found: {:?}", archive_path));
    }

    // Ensure destination directory exists
    if !dest_dir.exists() {
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Failed to create destination directory: {}", e))?;
    }

    let _ = progress_tx.send(ExtractProgress::Extracting);

    // Use the simple decompress_file function from sevenz-rust
    // This handles the 7z format natively without relying on external tools
    match decompress_file(archive_path, dest_dir) {
        Ok(_) => {
            let _ = progress_tx.send(ExtractProgress::Completed);
            Ok(())
        }
        Err(e) => {
            let err_msg = format!("7z extraction failed: {}", e);
            let _ = progress_tx.send(ExtractProgress::Error(err_msg.clone()));
            Err(err_msg)
        }
    }
}

/// Alias for backward compatibility with app.rs
pub async fn extract_7z_with_progress(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<ExtractProgress>,
) -> Result<(), String> {
    extract_7z(archive_path, dest_dir, progress_tx).await
}
