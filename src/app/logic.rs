// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

// ============================================================================
// HIDE UPDATE MODE: Installation logic references
// ============================================================================
// This file contains the core installation logic that checks update_mode:
// - start_installation(): Captures update_mode value for async task
// - Archive mode: Skips formatting if update_mode is true
// - Update mode: Deletes specific directories instead of formatting
//
// If you hide the UI checkbox (see ui.rs), users won't be able to enable
// update mode, so this logic will never execute. Code remains but is unused.
//
// Search for "update_mode" in this file to find all references.
// ============================================================================

use super::{InstallerApp, AppState, ProgressInfo, get_available_disk_space};
use crate::config::{REPO_OPTIONS, TEMP_PREFIX, VOLUME_LABEL};
use crate::burn::{burn_image, BurnProgress};
use crate::copy::{copy_directory_with_progress, CopyProgress};
use crate::delete::{delete_directories, DeleteProgress};
use crate::drives::DriveInfo;
use crate::extract::{extract_7z_with_progress, ExtractProgress};
use crate::format::{format_drive_fat32, FormatProgress};
use crate::github::{download_asset, get_latest_release, DownloadProgress, Asset};
use eframe::egui;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl InstallerApp {
    pub(super) fn ensure_selection_valid(&mut self) {
        if !self.drives.is_empty() && self.selected_drive_idx.is_none() {
            self.selected_drive_idx = Some(0);
        }
        if let Some(idx) = self.selected_drive_idx {
            if idx >= self.drives.len() {
                self.selected_drive_idx = if self.drives.is_empty() {
                    None
                } else {
                    Some(0)
                };
            }
        }
    }

    pub(super) fn log(&self, msg: &str) {
        if let Ok(mut logs) = self.log_messages.lock() {
            logs.push(msg.to_string());
            // Keep only last 100 messages
            if logs.len() > 100 {
                logs.remove(0);
            }
        }
    }

    pub(super) fn cancel_installation(&mut self) {
        if let Some(token) = &self.cancel_token {
            self.log("Cancelling installation...");
            token.cancel();
            self.state = AppState::Cancelling;
            // Clear the cancel token so we don't try to cancel again
            self.cancel_token = None;
        }
    }

    /// Filter out source code archives and apply extension filtering from asset list
    pub(super) fn filter_assets(assets: Vec<Asset>, allowed_extensions: Option<&[&str]>) -> Vec<Asset> {
        assets.into_iter()
            .filter(|a| {
                // Filter out source code archives
                if a.name.starts_with("Source code") ||
                   a.name == "source.zip" ||
                   a.name == "source.tar.gz" {
                    return false;
                }

                // Apply extension filter if provided
                if let Some(extensions) = allowed_extensions {
                    // Asset must end with at least one of the allowed extensions
                    extensions.iter().any(|ext| a.name.ends_with(ext))
                } else {
                    // No extension filter, allow all
                    true
                }
            })
            .collect()
    }

    /// Strip all known extensions from an asset name to get the base name
    fn strip_extensions(name: &str) -> String {
        let mut base = name.to_string();

        // Remove known extensions in order of specificity
        for ext in &[".img.gz", ".img.xz", ".tar.gz", ".7z", ".zip", ".img"] {
            if base.ends_with(ext) {
                base = base.strip_suffix(ext).unwrap_or(&base).to_string();
                break; // Only strip one extension
            }
        }

        base
    }

    /// Check if we should auto-select an asset or show selection UI
    /// Returns (should_auto_select, selected_asset_index_if_auto)
    pub(super) fn should_auto_select(assets: &[Asset]) -> (bool, Option<usize>) {
        // Only one asset? Auto-select it
        if assets.len() == 1 {
            return (true, Some(0));
        }

        // Check if all assets have the same base name (different extensions only)
        let base_names: std::collections::HashSet<_> = assets.iter()
            .map(|a| Self::strip_extensions(&a.name))
            .collect();

        if base_names.len() == 1 {
            // Same base name, different extensions - pick by priority
            // Priority: .7z > .zip > .img.gz > .img.xz > .img
            const PRIORITY: &[&str] = &[".7z", ".zip", ".img.gz", ".img.xz", ".img"];

            for ext in PRIORITY {
                if let Some((idx, _)) = assets.iter()
                    .enumerate()
                    .find(|(_, a)| a.name.ends_with(ext))
                {
                    return (true, Some(idx));
                }
            }

            // Fallback: pick first if no priority match
            return (true, Some(0));
        }

        // Multiple different assets - need user selection
        (false, None)
    }

    pub(super) fn fetch_and_check_assets(&mut self, ctx: egui::Context) {
        self.state = AppState::FetchingAssets;
        self.log("Fetching available downloads...");

        let repo = &REPO_OPTIONS[self.selected_repo_idx];
        let repo_url = repo.url;
        let progress = self.progress.clone();
        let ctx_clone = ctx.clone();

        // Create channel for release result
        let (tx, rx) = mpsc::unbounded_channel();
        self.release_rx = Some(rx);

        // Spawn async task to fetch release
        self.runtime.spawn(async move {
            if let Ok(mut p) = progress.lock() {
                p.message = "Fetching release info...".to_string();
            }
            ctx_clone.request_repaint();

            let result = get_latest_release(repo_url).await;
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    pub(super) fn start_installation(&mut self, ctx: egui::Context) {
        let Some(drive_idx) = self.selected_drive_idx else {
            self.log("No drive selected");
            return;
        };

        let Some(drive) = self.drives.get(drive_idx).cloned() else {
            self.log("Invalid drive selection");
            return;
        };

        // Store the drive for later ejection
        self.installed_drive = Some(drive.clone());

        self.state = AppState::FetchingRelease;
        let repo = &REPO_OPTIONS[self.selected_repo_idx];
        let repo_name = repo.name;
        let repo_url = repo.url;
        self.log(&format!(
            "Starting installation to {} using {}",
            drive.name, repo_name
        ));

        // Log installation start to debug log
        crate::debug::log_section("Installation Started");
        crate::debug::log(&format!("Drive: {} ({})", drive.name, drive.device_path));
        crate::debug::log(&format!("Drive size: {} bytes", drive.size_bytes));
        crate::debug::log(&format!("Mount path: {:?}", drive.mount_path));
        crate::debug::log(&format!("Repository: {} ({})", repo_name, repo_url));

        // Check if running as root on Linux
        #[cfg(target_os = "linux")]
        {
            if unsafe { libc::geteuid() } == 0 {
                crate::debug::log("WARNING: Running as root user");
                if let Ok(sudo_user) = std::env::var("SUDO_USER") {
                    crate::debug::log(&format!("Detected sudo execution by user: {}", sudo_user));
                    self.log("Note: Running with sudo/root privileges");
                } else if let Ok(pkexec_uid) = std::env::var("PKEXEC_UID") {
                    crate::debug::log(&format!("Detected pkexec execution by UID: {}", pkexec_uid));
                    self.log("Note: Running with elevated privileges (pkexec)");
                } else {
                    crate::debug::log("Running as actual root user (not via sudo/pkexec)");
                    self.log("Note: Running as root user");
                }
            }
        }

        // Get the pre-fetched release and selected asset
        let Some(release) = self.fetched_release.take() else {
            self.log("Error: No release data available");
            self.state = AppState::Error;
            return;
        };

        let Some(asset_idx) = self.selected_asset_idx else {
            self.log("Error: No asset selected");
            self.state = AppState::Error;
            return;
        };

        let asset = if asset_idx < self.available_assets.len() {
            self.available_assets[asset_idx].clone()
        } else {
            self.log("Error: Invalid asset selection");
            self.state = AppState::Error;
            return;
        };

        // Clear asset selection data
        self.available_assets.clear();
        self.selected_asset_idx = None;

        let progress = self.progress.clone();
        let log_messages = self.log_messages.clone();
        let ctx_clone = ctx.clone();
        let volume_label = VOLUME_LABEL.to_string();
        let update_mode = self.update_mode;
        let update_directories: Vec<String> = repo.update_directories.iter().map(|s| s.to_string()).collect();

        // Create cancellation token
        let cancel_token = CancellationToken::new();
        self.cancel_token = Some(cancel_token.clone());

        // Channel for state updates
        let (state_tx, mut state_rx) = mpsc::unbounded_channel::<AppState>();

        // Disable drive polling during installation
        let _ = self.drive_poll_tx.send(false);

        // Clone values for the async block
        let state_tx_clone = state_tx.clone();
        let drive_poll_tx_clone = self.drive_poll_tx.clone();
        let cancel_token_clone = cancel_token.clone();

        // Spawn the installation task
        self.runtime.spawn(async move {
            let log = |msg: &str| {
                if let Ok(mut logs) = log_messages.lock() {
                    logs.push(msg.to_string());
                }
                // Also log to debug file/console
                crate::debug::log(msg);
                ctx_clone.request_repaint();
            };

            let set_progress = |current: u64, total: u64, message: &str| {
                if let Ok(mut p) = progress.lock() {
                    p.current = current;
                    p.total = total;
                    p.message = message.to_string();
                }
                ctx_clone.request_repaint();
            };

            // Use pre-fetched release and selected asset
            log(&format!(
                "Installing release: {} ({})",
                release.tag_name, asset.name
            ));
            crate::debug::log_section("Starting Installation");
            crate::debug::log(&format!("Release: {}", release.tag_name));
            crate::debug::log(&format!("Asset: {} ({} bytes)", asset.name, asset.size));

            // Define temp/cache directory for later use
            // On Linux/macOS, use cache dir to avoid temp space issues
            // Linux: ~/.cache, macOS: ~/Library/Caches
            #[cfg(target_os = "linux")]
            let temp_dir = {
                // If running as root via sudo or pkexec, try to use the actual user's cache directory
                if unsafe { libc::geteuid() } == 0 {
                    // First check for SUDO_USER (command-line sudo)
                    if let Ok(sudo_user) = std::env::var("SUDO_USER") {
                        let user_home = std::path::PathBuf::from(format!("/home/{}", sudo_user));
                        if user_home.exists() {
                            let user_cache = user_home.join(".cache");
                            crate::debug::log(&format!("Using cache dir for sudo user {}: {:?}", sudo_user, user_cache));
                            user_cache
                        } else {
                            crate::debug::log(&format!("User home not found at {:?}, using default", user_home));
                            dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                        }
                    }
                    // Check for PKEXEC_UID (GUI elevation via pkexec)
                    else if let Ok(pkexec_uid) = std::env::var("PKEXEC_UID") {
                        if let Ok(uid) = pkexec_uid.parse::<u32>() {
                            // Get username from UID using libc
                            let pwd = unsafe { libc::getpwuid(uid) };
                            if !pwd.is_null() {
                                let username = unsafe {
                                    std::ffi::CStr::from_ptr((*pwd).pw_name)
                                        .to_string_lossy()
                                        .to_string()
                                };
                                let user_home = std::path::PathBuf::from(format!("/home/{}", username));
                                if user_home.exists() {
                                    let user_cache = user_home.join(".cache");
                                    crate::debug::log(&format!("Using cache dir for pkexec user {} (UID {}): {:?}", username, uid, user_cache));
                                    user_cache
                                } else {
                                    crate::debug::log(&format!("User home not found at {:?}, using default", user_home));
                                    dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                                }
                            } else {
                                crate::debug::log(&format!("Failed to get username for UID {}, using default", uid));
                                dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                            }
                        } else {
                            crate::debug::log(&format!("Failed to parse PKEXEC_UID '{}', using default", pkexec_uid));
                            dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                        }
                    }
                    else {
                        crate::debug::log("Running as root, using root's cache dir");
                        dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                    }
                } else {
                    dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                }
            };
            #[cfg(target_os = "macos")]
            let temp_dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            let temp_dir = std::env::temp_dir();

            crate::debug::log(&format!("Cache/temp directory: {:?}", temp_dir));

            // Check available disk space before starting
            // We need space for: download (asset.size) + extraction (~3x asset.size)
            let required_space = asset.size * 4; // 4x for safety margin
            let available_space = get_available_disk_space(&temp_dir);

            crate::debug::log(&format!("Required disk space: {} MB", required_space / 1_048_576));
            crate::debug::log(&format!("Available disk space: {} MB", available_space / 1_048_576));

            if available_space < required_space {
                let required_mb = required_space / 1_048_576;
                let available_mb = available_space / 1_048_576;
                let err_msg = format!(
                    "Insufficient disk space. Need {} MB, but only {} MB available in cache directory. Please free up disk space and try again.",
                    required_mb, available_mb
                );
                log(&err_msg);
                crate::debug::log(&format!("ERROR: {}", err_msg));
                let _ = state_tx_clone.send(AppState::Error);
                let _ = drive_poll_tx_clone.send(true);
                return;
            }

            log(&format!("Disk space check passed: {} MB available", available_space / 1_048_576));

            // Detect installation mode: raw image vs archive
            let is_raw_image = asset.name.ends_with(".img.gz") ||
                               asset.name.ends_with(".img.xz") ||
                               asset.name.ends_with(".img");

            if is_raw_image {
                crate::debug::log("Detected RAW IMAGE mode - will burn image to device");
                log("Note: Raw image mode - this will erase the entire drive");
            } else {
                crate::debug::log("Detected ARCHIVE mode - will format, extract, and copy files");
            }

            // Step 2: Format drive (only for archive mode - skip for raw images and update mode)
            if !is_raw_image && !update_mode {
                // Format drive (do this first so we fail fast if the card has issues)
            let _ = state_tx_clone.send(AppState::Formatting);
            log(&format!("Formatting {}...", drive.name));
            crate::debug::log_section("Formatting Drive");
            set_progress(0, 100, "Formatting drive...");

            let (fmt_tx, mut fmt_rx) = mpsc::unbounded_channel::<FormatProgress>();
            let progress_fmt = progress.clone();
            let ctx_fmt = ctx_clone.clone();

            // Spawn format progress handler
            let fmt_handle = tokio::spawn(async move {
                while let Some(prog) = fmt_rx.recv().await {
                    if let Ok(mut p) = progress_fmt.lock() {
                        match prog {
                            FormatProgress::Started => {
                                p.message = "Starting format...".to_string();
                            }
                            FormatProgress::Unmounting => {
                                p.message = "Unmounting drive...".to_string();
                            }
                            #[cfg(not(target_os = "macos"))]
                            FormatProgress::CleaningDisk => {
                                p.message = "Cleaning disk...".to_string();
                            }
                            #[cfg(not(target_os = "macos"))]
                            FormatProgress::CreatingPartition => {
                                p.message = "Creating partition...".to_string();
                            }
                            FormatProgress::Formatting => {
                                p.message = "Formatting to FAT32...".to_string();
                            }
                            FormatProgress::Progress { percent } => {
                                p.current = percent as u64;
                                p.total = 100;
                                p.message = format!("Formatting... {}%", percent);
                            }
                            FormatProgress::Completed => {
                                p.current = 100;
                                p.total = 100;
                                p.message = "Format complete".to_string();
                            }
                            FormatProgress::Cancelled => {
                                p.message = "Format cancelled".to_string();
                            }
                            FormatProgress::Error(ref e) => {
                                p.message = format!("Format error: {}", e);
                            }
                        }
                    }
                    ctx_fmt.request_repaint();
                }
            });

            // On Windows, format function expects drive letter (e.g., "E:"), not physical drive path
            // On other platforms, device_path is correct (e.g., "/dev/sdb")
            #[cfg(target_os = "windows")]
            let format_path = &drive.name;
            #[cfg(not(target_os = "windows"))]
            let format_path = &drive.device_path;

            if let Err(e) = format_drive_fat32(format_path, &volume_label, fmt_tx, cancel_token_clone.clone()).await {
                if e.contains("cancelled") {
                    log("Format cancelled");
                    let _ = state_tx_clone.send(AppState::Idle);
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }
                log(&format!("Format error: {}", e));
                let _ = state_tx_clone.send(AppState::Error);
                let _ = drive_poll_tx_clone.send(true);
                return;
            }

                let _ = fmt_handle.await;
                log("Format complete");
                crate::debug::log("Format complete");
            } // End of format block for archive mode

            // Step 2.5: Get mount path and delete directories for update mode (only for archive mode)
            let dest_path_from_update = if !is_raw_image && update_mode {
                // First, get mount path for the existing installation
                crate::debug::log("Update mode: Getting existing mount path...");
                let mount_path = match get_mount_path_after_format(&drive, &volume_label).await {
                    Ok(path) => path,
                    Err(e) => {
                        log(&format!("Error getting mount path: {}", e));
                        crate::debug::log(&format!("ERROR getting mount path: {}", e));
                        let _ = state_tx_clone.send(AppState::Error);
                        let _ = drive_poll_tx_clone.send(true);
                        return;
                    }
                };

                // Delete old directories
                let _ = state_tx_clone.send(AppState::Deleting);
                log("Deleting old directories...");
                crate::debug::log_section("Deleting Directories");
                set_progress(0, 100, "Deleting old directories...");

                let (del_tx, mut del_rx) = mpsc::unbounded_channel::<DeleteProgress>();
                let progress_del = progress.clone();
                let ctx_del = ctx_clone.clone();

                // Spawn delete progress handler
                let del_handle = tokio::spawn(async move {
                    while let Some(prog) = del_rx.recv().await {
                        if let Ok(mut p) = progress_del.lock() {
                            match prog {
                                DeleteProgress::Started { total_dirs } => {
                                    p.current = 0;
                                    p.total = total_dirs as u64;
                                    p.message = format!("Deleting {} directories...", total_dirs);
                                }
                                DeleteProgress::DeletingDirectory { ref name } => {
                                    p.message = format!("Deleting: {}", name);
                                }
                                DeleteProgress::Completed => {
                                    p.current = p.total;
                                    p.message = "Directory deletion complete".to_string();
                                }
                                DeleteProgress::Cancelled => {
                                    p.message = "Deletion cancelled".to_string();
                                }
                                DeleteProgress::Error(ref e) => {
                                    p.message = format!("Deletion error: {}", e);
                                }
                            }
                        }
                        ctx_del.request_repaint();
                    }
                });

                let update_dirs_refs: Vec<&str> = update_directories.iter().map(|s| s.as_str()).collect();
                if let Err(e) = delete_directories(&mount_path, &update_dirs_refs, del_tx, cancel_token_clone.clone()).await {
                    if e.contains("cancelled") {
                        log("Deletion cancelled");
                        let _ = state_tx_clone.send(AppState::Idle);
                        let _ = drive_poll_tx_clone.send(true);
                        return;
                    }
                    log(&format!("Deletion error: {}", e));
                    let _ = state_tx_clone.send(AppState::Error);
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }

                let _ = del_handle.await;
                log("Directory deletion complete");
                crate::debug::log("Directory deletion complete");

                Some(mount_path) // Return the mount path for reuse
            } else {
                None
            };

            // Get the destination path for extraction (only for archive mode)
            // For image mode, we don't need this until after burning
            // For update mode, reuse the path from deletion step
            let dest_path = if !is_raw_image {
                if let Some(path) = dest_path_from_update {
                    // Update mode - reuse the mount path from deletion
                    crate::debug::log("Reusing mount path from update...");
                    log(&format!("Destination: {}", path.display()));
                    Some(path)
                } else {
                    // Fresh install - get mount path after format
                    crate::debug::log("Getting mount path after format...");
                    match get_mount_path_after_format(&drive, &volume_label).await {
                        Ok(path) => {
                            log(&format!("Destination: {}", path.display()));
                            crate::debug::log(&format!("Mount path: {:?}", path));
                            Some(path)
                        }
                        Err(e) => {
                            log(&format!("Error getting mount path: {}", e));
                            crate::debug::log(&format!("ERROR getting mount path: {}", e));
                            let _ = state_tx_clone.send(AppState::Error);
                            let _ = drive_poll_tx_clone.send(true);
                            return;
                        }
                    }
                }
            } else {
                None // Image mode doesn't need mount path yet
            };

            // Create a log file on the SD card for debugging (only for archive mode)
            let log_file_path = dest_path.as_ref().map(|p| p.join("install_log.txt"));
            let write_card_log = move |msg: &str| {
                if let Some(ref path) = log_file_path {
                    use std::io::Write;
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let _ = writeln!(file, "[{}] {}", timestamp, msg);
                    }
                }
            };

            if !is_raw_image {
                write_card_log("Format complete, starting download...");
            }

            // Step 3: Download
            let _ = state_tx_clone.send(AppState::Downloading);
            let size_mb = asset.size as f64 / 1_048_576.0;
            log(&format!("Downloading release ({:.1} MB)...", size_mb));
            crate::debug::log_section("Downloading Release");

            let download_path = temp_dir.join(&asset.name);
            crate::debug::log(&format!("Download path: {:?}", download_path));

            let (dl_tx, mut dl_rx) = mpsc::unbounded_channel::<DownloadProgress>();

            let download_path_clone = download_path.clone();
            let asset_clone = asset.clone();
            let progress_clone = progress.clone();
            let ctx_dl = ctx_clone.clone();

            // Spawn download progress handler
            let dl_handle = tokio::spawn(async move {
                while let Some(prog) = dl_rx.recv().await {
                    match prog {
                        DownloadProgress::Started { total_bytes } => {
                            if let Ok(mut p) = progress_clone.lock() {
                                p.total = total_bytes;
                                p.current = 0;
                                p.message = "Downloading...".to_string();
                            }
                        }
                        DownloadProgress::Progress { downloaded, total } => {
                            if let Ok(mut p) = progress_clone.lock() {
                                p.current = downloaded;
                                p.total = total;
                                let pct = (downloaded as f64 / total as f64 * 100.0) as u32;
                                p.message = format!("Downloading... {}%", pct);
                            }
                        }
                        DownloadProgress::Completed => {
                            if let Ok(mut p) = progress_clone.lock() {
                                p.message = "Download complete".to_string();
                            }
                        }
                        DownloadProgress::Cancelled => {
                            if let Ok(mut p) = progress_clone.lock() {
                                p.message = "Download cancelled".to_string();
                            }
                        }
                        DownloadProgress::Error(e) => {
                            if let Ok(mut p) = progress_clone.lock() {
                                p.message = format!("Download error: {}", e);
                            }
                        }
                    }
                    ctx_dl.request_repaint();
                }
            });

            if let Err(e) = download_asset(&asset_clone, &download_path_clone, dl_tx, cancel_token_clone.clone()).await {
                if e.contains("cancelled") {
                    log("Download cancelled");
                    let _ = tokio::fs::remove_file(&download_path_clone).await;
                    crate::debug::log("Cleaned up partial download file");
                    let _ = state_tx_clone.send(AppState::Idle);
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }
                log(&format!("Download error: {}", e));
                let _ = tokio::fs::remove_file(&download_path_clone).await;
                crate::debug::log("Cleaned up partial download file");
                let _ = state_tx_clone.send(AppState::Error);
                let _ = drive_poll_tx_clone.send(true);
                return;
            }

            let _ = dl_handle.await;
            log("Download complete");
            crate::debug::log("Download complete");

            // ======================================================================
            // BRANCHING POINT: Archive mode vs Raw Image mode
            // ======================================================================

            if is_raw_image {
                // ===== RAW IMAGE MODE: Burn (with decompression) → Verify =====
                crate::debug::log_section("Raw Image Mode");
                write_card_log("Download complete, preparing to burn image...");

                // Step 4: Burn image to device (burn.rs handles .gz decompression automatically)
                let _ = state_tx_clone.send(AppState::Burning);
                log(&format!("Burning image to {}...", drive.name));
                crate::debug::log_section("Burning Image");
                crate::debug::log(&format!("Device: {}", drive.device_path));

                let (burn_tx, mut burn_rx) = mpsc::unbounded_channel::<BurnProgress>();
                let progress_burn = progress.clone();
                let ctx_burn = ctx_clone.clone();

                // Spawn burn progress handler
                let burn_handle = tokio::spawn(async move {
                    while let Some(prog) = burn_rx.recv().await {
                        if let Ok(mut p) = progress_burn.lock() {
                            match prog {
                                BurnProgress::Started { total_bytes } => {
                                    p.total = total_bytes;
                                    p.current = 0;
                                    p.message = "Starting burn...".to_string();
                                }
                                BurnProgress::Writing { written, total } => {
                                    p.current = written;
                                    p.total = total;
                                    let pct = (written as f64 / total as f64 * 100.0) as u32;
                                    let mb_written = written / 1_048_576;
                                    let mb_total = total / 1_048_576;
                                    p.message = format!("Writing... {}% ({}/{} MB)", pct, mb_written, mb_total);
                                }
                                BurnProgress::Verifying { verified, total } => {
                                    p.current = verified;
                                    p.total = total;
                                    let pct = (verified as f64 / total as f64 * 100.0) as u32;
                                    p.message = format!("Verifying... {}%", pct);
                                }
                                BurnProgress::Completed => {
                                    p.current = p.total;
                                    p.message = "Burn complete".to_string();
                                }
                                BurnProgress::Cancelled => {
                                    p.message = "Burn cancelled".to_string();
                                }
                                BurnProgress::Error(e) => {
                                    p.message = format!("Burn error: {}", e);
                                }
                            }
                        }
                        ctx_burn.request_repaint();
                    }
                });

                if let Err(e) = burn_image(&download_path, &drive.device_path, burn_tx, cancel_token_clone.clone()).await {
                    if e.contains("cancelled") {
                        log("Burn cancelled");
                        let _ = tokio::fs::remove_file(&download_path).await;
                        let _ = state_tx_clone.send(AppState::Idle);
                        let _ = drive_poll_tx_clone.send(true);
                        return;
                    }
                    log(&format!("Burn error: {}", e));
                    let _ = tokio::fs::remove_file(&download_path).await;
                    let _ = state_tx_clone.send(AppState::Error);
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }

                let _ = burn_handle.await;
                log("Image burn and verification complete");
                crate::debug::log("Image burn and verification complete");

                // Clean up downloaded image
                let _ = tokio::fs::remove_file(&download_path).await;
                crate::debug::log("Cleaned up temp files");

                log("Installation complete! You can now safely eject the drive.");
                crate::debug::log("Installation complete!");
                let _ = state_tx_clone.send(AppState::Complete);
                let _ = drive_poll_tx_clone.send(true);

            } else {
                // ===== ARCHIVE MODE: Format → Extract → Copy =====
                crate::debug::log_section("Archive Mode");
                write_card_log("Download complete, starting extraction...");

            // Step 4: Extract to temp folder on local PC
            // On Linux, use the same temp_dir we already determined
            // On macOS, give cache_dir() another try (original behavior)
            #[cfg(target_os = "linux")]
            let extract_base_dir = temp_dir.clone();
            #[cfg(target_os = "macos")]
            let extract_base_dir = dirs::cache_dir().unwrap_or_else(|| temp_dir.clone());
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            let extract_base_dir = temp_dir.clone();

            let _ = state_tx_clone.send(AppState::Extracting);
            let temp_extract_dir = extract_base_dir.join(format!("{}_extract", TEMP_PREFIX));
            log("Extracting files to local temp folder...");
            crate::debug::log_section("Extracting Files");
            crate::debug::log(&format!("Temp extract dir: {:?}", temp_extract_dir));
            set_progress(0, 100, "Extracting files...");

            // Clean up any previous extraction
            let _ = std::fs::remove_dir_all(&temp_extract_dir);
            if let Err(e) = std::fs::create_dir_all(&temp_extract_dir) {
                let err_msg = format!("Failed to create temp extract dir: {}", e);
                log(&err_msg);
                crate::debug::log(&format!("ERROR: {}", err_msg));
                let _ = state_tx_clone.send(AppState::Error);
                let _ = drive_poll_tx_clone.send(true);
                return;
            }

            let (ext_tx, mut ext_rx) = mpsc::unbounded_channel::<ExtractProgress>();
            let progress_ext = progress.clone();
            let ctx_ext = ctx_clone.clone();

            // Spawn extract progress handler
            let ext_handle = tokio::spawn(async move {
                while let Some(prog) = ext_rx.recv().await {
                    if let Ok(mut p) = progress_ext.lock() {
                        match prog {
                            ExtractProgress::Started => {
                                p.message = "Starting extraction...".to_string();
                            }
                            ExtractProgress::Extracting => {
                                p.message = "Extracting files...".to_string();
                            }
                            ExtractProgress::Progress { percent } => {
                                p.current = percent as u64;
                                p.total = 100;
                                p.message = format!("Extracting... {}%", percent);
                            }
                            ExtractProgress::Completed => {
                                p.current = 100;
                                p.total = 100;
                                p.message = "Extraction complete".to_string();
                            }
                            ExtractProgress::Cancelled => {
                                p.message = "Extraction cancelled".to_string();
                            }
                            ExtractProgress::Error(e) => {
                                p.message = format!("Extract error: {}", e);
                            }
                        }
                    }
                    ctx_ext.request_repaint();
                }
            });

            write_card_log(&format!(
                "Calling 7z extraction: {:?} -> {:?}",
                download_path, temp_extract_dir
            ));

            if let Err(e) = extract_7z_with_progress(&download_path, &temp_extract_dir, ext_tx, cancel_token_clone.clone()).await {
                if e.contains("cancelled") {
                    write_card_log("Extraction cancelled");
                    log("Extraction cancelled");
                    let _ = std::fs::remove_dir_all(&temp_extract_dir);
                    let _ = tokio::fs::remove_file(&download_path).await;
                    crate::debug::log("Cleaned up download file after cancellation");
                    let _ = state_tx_clone.send(AppState::Idle);
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }
                write_card_log(&format!("Extract error: {}", e));
                log(&format!("Extract error: {}", e));
                let _ = std::fs::remove_dir_all(&temp_extract_dir);
                let _ = tokio::fs::remove_file(&download_path).await;
                crate::debug::log("Cleaned up download file after error");
                let _ = state_tx_clone.send(AppState::Error);
                let _ = drive_poll_tx_clone.send(true);
                return;
            }

            let _ = ext_handle.await;
            log("Extraction complete");
            write_card_log("Extraction complete");
            crate::debug::log("Extraction complete");

            // Step 5: Copy files to SD card
            let _ = state_tx_clone.send(AppState::Copying);
            log("Copying files to SD card...");
            crate::debug::log_section("Copying Files");
            set_progress(0, 100, "Copying files...");

            let (copy_tx, mut copy_rx) = mpsc::unbounded_channel::<CopyProgress>();
            let progress_copy = progress.clone();
            let ctx_copy = ctx_clone.clone();

            // Spawn copy progress handler
            let copy_handle = tokio::spawn(async move {
                while let Some(prog) = copy_rx.recv().await {
                    if let Ok(mut p) = progress_copy.lock() {
                        match prog {
                            CopyProgress::Counting => {
                                p.message = "Counting files...".to_string();
                            }
                            CopyProgress::Started { total_bytes, total_files } => {
                                p.total = total_bytes;
                                p.current = 0;
                                p.message = format!("Copying {} files...", total_files);
                            }
                            CopyProgress::Progress { copied_bytes, total_bytes, current_file } => {
                                p.current = copied_bytes;
                                p.total = total_bytes;
                                let pct = if total_bytes > 0 {
                                    (copied_bytes as f64 / total_bytes as f64 * 100.0) as u32
                                } else {
                                    0
                                };
                                if current_file.is_empty() {
                                    p.message = format!("Copying... {}%", pct);
                                } else {
                                    // Truncate filename if too long
                                    let display_file = if current_file.len() > 40 {
                                        format!("...{}", &current_file[current_file.len()-37..])
                                    } else {
                                        current_file
                                    };
                                    p.message = format!("{}% - {}", pct, display_file);
                                }
                            }
                            CopyProgress::Completed => {
                                p.current = p.total;
                                p.message = "Copy complete".to_string();
                            }
                            CopyProgress::Cancelled => {
                                p.message = "Copy cancelled".to_string();
                            }
                            CopyProgress::Error(e) => {
                                p.message = format!("Copy error: {}", e);
                            }
                        }
                    }
                    ctx_copy.request_repaint();
                }
            });

            let dest_path_unwrapped = dest_path.as_ref().expect("dest_path should be Some in archive mode");

            write_card_log(&format!(
                "Copying files: {:?} -> {:?}",
                temp_extract_dir, dest_path_unwrapped
            ));

            if let Err(e) = copy_directory_with_progress(&temp_extract_dir, dest_path_unwrapped, copy_tx, cancel_token_clone.clone()).await {
                if e.contains("cancelled") {
                    write_card_log("Copy cancelled");
                    log("Copy cancelled");
                    let _ = std::fs::remove_dir_all(&temp_extract_dir);
                    let _ = tokio::fs::remove_file(&download_path).await;
                    crate::debug::log("Cleaned up download file after cancellation");
                    let _ = state_tx_clone.send(AppState::Idle);
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }
                write_card_log(&format!("Copy error: {}", e));
                log(&format!("Copy error: {}", e));
                let _ = std::fs::remove_dir_all(&temp_extract_dir);
                let _ = tokio::fs::remove_file(&download_path).await;
                crate::debug::log("Cleaned up download file after error");
                let _ = state_tx_clone.send(AppState::Error);
                let _ = drive_poll_tx_clone.send(true);
                return;
            }

            let _ = copy_handle.await;
            log("Copy complete");
            write_card_log("Copy complete");
            crate::debug::log("Copy complete");

            // Clean up temp extraction folder
            let _ = std::fs::remove_dir_all(&temp_extract_dir);
            crate::debug::log("Cleaned up temp extraction folder");

            // Cleanup temp file
            let _ = tokio::fs::remove_file(&download_path).await;
            write_card_log("Cleaned up temp download file");
            crate::debug::log("Cleaned up temp download file");

            // Copy debug log to SD card
            log("Writing debug log to SD card...");
            crate::debug::log("Copying debug log to SD card...");
            let dest_path_unwrapped = dest_path.expect("dest_path should be Some in archive mode");
            match crate::debug::copy_log_to(&dest_path_unwrapped) {
                Ok(log_path) => {
                    log(&format!("Debug log saved to: {}", log_path.display()));
                    crate::debug::log(&format!("Debug log copied to: {:?}", log_path));
                }
                Err(e) => {
                    log(&format!("Warning: Could not copy debug log: {}", e));
                    crate::debug::log(&format!("Failed to copy debug log: {}", e));
                }
            }

                log("Installation complete! You can now safely eject the SD card.");
                write_card_log("Installation complete!");
                crate::debug::log("Installation complete!");
                let _ = state_tx_clone.send(AppState::Complete);
                let _ = drive_poll_tx_clone.send(true);
            } // End of archive mode
        });

        // Spawn a task to update state from the channel
        let progress = self.progress.clone();
        let ctx_state = ctx.clone();
        self.runtime.spawn(async move {
            while let Some(new_state) = state_rx.recv().await {
                // We can't directly update self.state from here, but we can use the progress message
                // to communicate state. The UI will poll the progress message.
                if let Ok(mut p) = progress.lock() {
                    p.message = match new_state {
                        AppState::FetchingRelease => "Fetching release...".to_string(),
                        AppState::Downloading => "Downloading...".to_string(),
                        AppState::Formatting => "Formatting...".to_string(),
                        AppState::Deleting => "Deleting old directories...".to_string(),
                        AppState::Extracting => "Extracting...".to_string(),
                        AppState::Copying => "Copying...".to_string(),
                        AppState::Burning => "Burning image...".to_string(),
                        AppState::Complete => "COMPLETE".to_string(),
                        AppState::Error => "ERROR".to_string(),
                        AppState::Idle => "CANCELLED".to_string(),
                        _ => p.message.clone(),
                    };
                }
                ctx_state.request_repaint();
            }
        });
    }
}

/// Get the mount path after formatting, handling platform differences
#[cfg(target_os = "windows")]
pub(super) async fn get_mount_path_after_format(drive: &DriveInfo, _volume_label: &str) -> Result<PathBuf, String> {
    // On Windows, the drive letter remains the same after formatting
    // The mount_path should be set (e.g., "E:\")
    drive.mount_path.clone().ok_or_else(|| {
        format!("No mount path available for drive {}", drive.name)
    })
}

#[cfg(target_os = "macos")]
pub(super) async fn get_mount_path_after_format(_drive: &DriveInfo, volume_label: &str) -> Result<PathBuf, String> {
    // macOS automatically mounts at /Volumes/LABEL after diskutil eraseDisk
    // Wait a moment for the mount to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    let mount_path = PathBuf::from(format!("/Volumes/{}", volume_label));

    // Wait for the mount point to appear (up to 10 seconds)
    for _ in 0..20 {
        if mount_path.exists() {
            return Ok(mount_path);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    Err(format!("Mount point {} did not appear after formatting", mount_path.display()))
}

#[cfg(target_os = "linux")]
pub(super) async fn get_mount_path_after_format(drive: &DriveInfo, volume_label: &str) -> Result<PathBuf, String> {
    use tokio::process::Command;

    // Determine the partition path
    let partition_path = if drive.device_path.contains("mmcblk") || drive.device_path.contains("nvme") {
        format!("{}p1", drive.device_path)
    } else {
        format!("{}1", drive.device_path)
    };

    // Use udisksctl to mount - this registers with the udisks2 daemon so it won't
    // auto-remount when we later unmount. The daemon chooses the mount point
    // (typically /media/username/LABEL or /run/media/username/LABEL).
    crate::debug::log(&format!("Mounting {} via udisksctl...", partition_path));
    let output = Command::new("udisksctl")
        .args(["mount", "-b", &partition_path])
        .output()
        .await
        .map_err(|e| format!("Failed to run udisksctl mount: {}", e))?;

    if output.status.success() {
        // Parse mount point from udisksctl output: "Mounted /dev/sdb1 at /media/user/LABEL"
        let stdout = String::from_utf8_lossy(&output.stdout);
        crate::debug::log(&format!("udisksctl output: {}", stdout.trim()));

        if let Some(mount_point) = stdout.split(" at ").nth(1) {
            let mount_path = PathBuf::from(mount_point.trim().trim_end_matches('.'));
            crate::debug::log(&format!("Mount point: {:?}", mount_path));
            return Ok(mount_path);
        }
    }

    // Check stderr for errors
    let stderr = String::from_utf8_lossy(&output.stderr);
    crate::debug::log(&format!("udisksctl error: {}", stderr.trim()));

    // Check if already mounted - extract existing mount path from error message
    // Error format: "...AlreadyMounted: Device /dev/xxx is already mounted at `/path/to/mount'."
    if stderr.contains("AlreadyMounted") {
        if let Some(start) = stderr.find("already mounted at `") {
            let after_prefix = &stderr[start + "already mounted at `".len()..];
            if let Some(end) = after_prefix.find("'") {
                let existing_mount = &after_prefix[..end];
                crate::debug::log(&format!("Device already mounted, using existing mount point: {}", existing_mount));
                return Ok(PathBuf::from(existing_mount));
            }
        }
        // Also try alternate format without backticks
        if let Some(start) = stderr.find("already mounted at ") {
            let after_prefix = &stderr[start + "already mounted at ".len()..];
            // Take until end of line or period
            let mount_path: String = after_prefix
                .chars()
                .take_while(|&c| c != '.' && c != '\n' && c != '\'' && c != '`')
                .collect();
            let mount_path = mount_path.trim();
            if !mount_path.is_empty() {
                crate::debug::log(&format!("Device already mounted, using existing mount point: {}", mount_path));
                return Ok(PathBuf::from(mount_path));
            }
        }
    }

    // Fallback: use raw mount if udisksctl fails (e.g., no udisks2 daemon)
    crate::debug::log("udisksctl mount failed, falling back to raw mount...");

    let cache_dir = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let mount_point = cache_dir.join(format!("{}_{}", TEMP_PREFIX, volume_label));

    // Create the mount directory if it doesn't exist
    let _ = std::fs::create_dir_all(&mount_point);

    // Mount the partition
    let output = Command::new("mount")
        .args([&partition_path, mount_point.to_str().unwrap()])
        .output()
        .await
        .map_err(|e| format!("Failed to mount partition: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to mount partition: {}", stderr));
    }

    Ok(mount_point)
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub(super) async fn get_mount_path_after_format(_drive: &DriveInfo, _volume_label: &str) -> Result<PathBuf, String> {
    Err("Mounting not supported on this platform".to_string())
}
