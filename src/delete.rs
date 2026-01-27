// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

use std::path::Path;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum DeleteProgress {
    Started { total_dirs: usize },
    DeletingDirectory { name: String },
    Completed,
    Cancelled,
    #[allow(dead_code)]
    Error(String),
}

/// Delete specified directories from the SD card during update mode
pub async fn delete_directories(
    mount_path: &Path,
    directories: &[&str],
    progress_tx: mpsc::UnboundedSender<DeleteProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(DeleteProgress::Cancelled);
        return Err("Deletion cancelled".to_string());
    }

    let _ = progress_tx.send(DeleteProgress::Started {
        total_dirs: directories.len(),
    });

    crate::debug::log_section("Deleting Old Directories");
    crate::debug::log(&format!("Mount path: {:?}", mount_path));

    for dir_name in directories {
        // Check for cancellation
        if cancel_token.is_cancelled() {
            let _ = progress_tx.send(DeleteProgress::Cancelled);
            return Err("Deletion cancelled".to_string());
        }

        let dir_path = mount_path.join(dir_name);

        crate::debug::log(&format!("Checking directory: {:?}", dir_path));

        // Check if directory exists
        if !dir_path.exists() {
            crate::debug::log(&format!("Directory does not exist, skipping: {}", dir_name));
            continue;
        }

        // Send progress update
        let _ = progress_tx.send(DeleteProgress::DeletingDirectory {
            name: dir_name.to_string(),
        });

        crate::debug::log(&format!("Deleting directory: {}", dir_name));

        // Delete the directory recursively
        match tokio::fs::remove_dir_all(&dir_path).await {
            Ok(_) => {
                crate::debug::log(&format!("Successfully deleted: {}", dir_name));
            }
            Err(e) => {
                let err_msg = format!("Failed to delete {}: {}", dir_name, e);
                crate::debug::log(&format!("ERROR: {}", err_msg));
                return Err(err_msg);
            }
        }
    }

    let _ = progress_tx.send(DeleteProgress::Completed);
    crate::debug::log("Directory deletion complete");

    Ok(())
}
