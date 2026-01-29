// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Download state for pause/resume functionality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    /// Original download URL
    pub url: String,
    /// Total file size in bytes
    pub total_size: u64,
    /// Destination file path
    pub dest_path: String,
    /// Chunk completion status
    pub chunks: Vec<ChunkState>,
    /// Total bytes downloaded so far
    pub downloaded_bytes: u64,
}

/// State of an individual download chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkState {
    /// Starting byte position
    pub start: u64,
    /// Ending byte position
    pub end: u64,
    /// Whether this chunk is fully downloaded
    pub completed: bool,
}

impl DownloadState {
    /// Create a new download state for a fresh download
    pub fn new(url: String, total_size: u64, dest_path: PathBuf, num_chunks: u64) -> Self {
        let chunk_size = total_size / num_chunks;
        let mut chunks = Vec::new();

        for i in 0..num_chunks {
            let start = i * chunk_size;
            let end = if i == num_chunks - 1 {
                total_size - 1
            } else {
                (i + 1) * chunk_size - 1
            };

            chunks.push(ChunkState {
                start,
                end,
                completed: false,
            });
        }

        DownloadState {
            url,
            total_size,
            dest_path: dest_path.to_string_lossy().to_string(),
            chunks,
            downloaded_bytes: 0,
        }
    }

    /// Get the state file path for a given destination
    pub fn get_state_file_path(dest_path: &Path) -> PathBuf {
        let mut state_path = dest_path.to_path_buf();
        let mut filename = state_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        filename.push_str(".partial");
        state_path.set_file_name(filename);
        state_path
    }

    /// Save state to disk
    pub fn save(&self, dest_path: &Path) -> Result<(), String> {
        let state_path = Self::get_state_file_path(dest_path);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize download state: {}", e))?;

        std::fs::write(&state_path, json)
            .map_err(|e| format!("Failed to write download state: {}", e))?;

        crate::debug::log(&format!("Saved download state to: {:?}", state_path));
        Ok(())
    }

    /// Load state from disk
    pub fn load(dest_path: &Path) -> Result<Self, String> {
        let state_path = Self::get_state_file_path(dest_path);

        if !state_path.exists() {
            return Err("No partial download state found".to_string());
        }

        let json = std::fs::read_to_string(&state_path)
            .map_err(|e| format!("Failed to read download state: {}", e))?;

        let state: DownloadState = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse download state: {}", e))?;

        crate::debug::log(&format!("Loaded download state from: {:?}", state_path));
        crate::debug::log(&format!("Resume info: {} / {} bytes ({:.1}%)",
            state.downloaded_bytes,
            state.total_size,
            (state.downloaded_bytes as f64 / state.total_size as f64 * 100.0)
        ));

        Ok(state)
    }

    /// Delete the state file
    pub fn delete_state_file(dest_path: &Path) {
        let state_path = Self::get_state_file_path(dest_path);
        if state_path.exists() {
            let _ = std::fs::remove_file(&state_path);
            crate::debug::log(&format!("Deleted download state file: {:?}", state_path));
        }
    }

    /// Check if a partial download exists
    pub fn exists(dest_path: &Path) -> bool {
        Self::get_state_file_path(dest_path).exists()
    }

    /// Mark a chunk as completed and update downloaded bytes
    pub fn mark_chunk_complete(&mut self, chunk_index: usize, bytes_downloaded: u64) {
        if let Some(chunk) = self.chunks.get_mut(chunk_index) {
            if !chunk.completed {
                chunk.completed = true;
                self.downloaded_bytes += bytes_downloaded;
            }
        }
    }

    /// Get list of incomplete chunk indices
    pub fn get_incomplete_chunks(&self) -> Vec<usize> {
        self.chunks
            .iter()
            .enumerate()
            .filter(|(_, chunk)| !chunk.completed)
            .map(|(i, _)| i)
            .collect()
    }

    /// Get completion percentage
    pub fn completion_percentage(&self) -> f64 {
        if self.total_size == 0 {
            return 0.0;
        }
        (self.downloaded_bytes as f64 / self.total_size as f64 * 100.0)
    }
}
