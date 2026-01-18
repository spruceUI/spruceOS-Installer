use crate::config::{
    setup_theme, APP_NAME, ASSET_EXTENSION, COLOR_ACCENT, COLOR_ACCENT_DIM, COLOR_BG_DARK,
    COLOR_BG_LIGHT, COLOR_ERROR, COLOR_SUCCESS, COLOR_TEXT, COLOR_WARNING, DEFAULT_REPO_INDEX,
    REPO_OPTIONS, TEMP_PREFIX, VOLUME_LABEL,
};
use crate::copy::{copy_directory_with_progress, CopyProgress};
use crate::drives::{get_removable_drives, DriveInfo};
use crate::eject::eject_drive;
use crate::extract::{extract_7z_with_progress, ExtractProgress};
use crate::format::{format_drive_fat32, FormatProgress};
use crate::github::{download_asset, find_release_asset, get_latest_release, DownloadProgress, Release};
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
enum AppState {
    Idle,
    AwaitingConfirmation,
    FetchingRelease,
    Downloading,
    Formatting,
    Extracting,
    Copying,
    Complete,
    Ejecting,
    Ejected,
    Cancelling,
    Error,
}

#[derive(Debug, Clone)]
struct ProgressInfo {
    current: u64,
    total: u64,
    message: String,
}

pub struct InstallerApp {
    // Runtime for async operations
    runtime: Runtime,

    // UI State
    drives: Vec<DriveInfo>,
    selected_drive_idx: Option<usize>,
    selected_repo_idx: usize,
    release_info: Option<Release>,

    // Progress tracking
    state: AppState,
    progress: Arc<Mutex<ProgressInfo>>,
    log_messages: Arc<Mutex<Vec<String>>>,

    // Temp file for downloads
    temp_download_path: Option<PathBuf>,

    // Drive that was installed to (for eject)
    installed_drive: Option<DriveInfo>,

    // Cancellation token for aborting installation
    cancel_token: Option<CancellationToken>,
}

impl InstallerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Apply theme from config
        setup_theme(&cc.egui_ctx);

        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        let mut app = Self {
            runtime,
            drives: Vec::new(),
            selected_drive_idx: None,
            selected_repo_idx: DEFAULT_REPO_INDEX,
            release_info: None,
            state: AppState::Idle,
            progress: Arc::new(Mutex::new(ProgressInfo {
                current: 0,
                total: 100,
                message: String::new(),
            })),
            log_messages: Arc::new(Mutex::new(Vec::new())),
            temp_download_path: None,
            installed_drive: None,
            cancel_token: None,
        };

        app.refresh_drives();
        app
    }

    fn refresh_drives(&mut self) {
        self.drives = get_removable_drives();
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

    fn log(&self, msg: &str) {
        if let Ok(mut logs) = self.log_messages.lock() {
            logs.push(msg.to_string());
            // Keep only last 100 messages
            if logs.len() > 100 {
                logs.remove(0);
            }
        }
    }

    fn cancel_installation(&mut self) {
        if let Some(token) = &self.cancel_token {
            self.log("Cancelling installation...");
            token.cancel();
            self.state = AppState::Cancelling;
        }
    }

    fn start_installation(&mut self, ctx: egui::Context) {
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
        let (repo_name, repo_url) = REPO_OPTIONS[self.selected_repo_idx];
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

        let repo_url = repo_url.to_string();
        let progress = self.progress.clone();
        let log_messages = self.log_messages.clone();
        let ctx_clone = ctx.clone();
        let volume_label = VOLUME_LABEL.to_string();

        // Create cancellation token
        let cancel_token = CancellationToken::new();
        self.cancel_token = Some(cancel_token.clone());

        // Channel for state updates
        let (state_tx, mut state_rx) = mpsc::unbounded_channel::<AppState>();

        // Clone values for the async block
        let state_tx_clone = state_tx.clone();
        let cancel_token_clone = cancel_token.clone();

        // Spawn the installation task
        self.runtime.spawn(async move {
            let log = |msg: &str| {
                if let Ok(mut logs) = log_messages.lock() {
                    logs.push(msg.to_string());
                }
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

            // Step 1: Fetch release
            log("Fetching latest release from GitHub...");
            crate::debug::log_section("Fetching Release");
            crate::debug::log(&format!("Repository URL: {}", repo_url));
            set_progress(0, 100, "Fetching release info...");

            let release = match get_latest_release(&repo_url).await {
                Ok(r) => r,
                Err(e) => {
                    log(&format!("Error: {}", e));
                    crate::debug::log(&format!("ERROR fetching release: {}", e));
                    let _ = state_tx_clone.send(AppState::Error);
                    return;
                }
            };

            let asset = match find_release_asset(&release) {
                Some(a) => a,
                None => {
                    log(&format!("Error: No {} file found in release", ASSET_EXTENSION));
                    crate::debug::log(&format!("ERROR: No {} asset found in release", ASSET_EXTENSION));
                    let _ = state_tx_clone.send(AppState::Error);
                    return;
                }
            };

            log(&format!(
                "Found release: {} ({})",
                release.tag_name, asset.name
            ));
            crate::debug::log(&format!("Release: {}", release.tag_name));
            crate::debug::log(&format!("Asset: {} ({} bytes)", asset.name, asset.size));

            // Step 2: Download
            let _ = state_tx_clone.send(AppState::Downloading);
            log("Downloading release...");
            crate::debug::log_section("Downloading Release");

            // On Linux, use cache dir (~/.cache) instead of /tmp to avoid tmpfs space issues
            #[cfg(target_os = "linux")]
            let temp_dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
            #[cfg(not(target_os = "linux"))]
            let temp_dir = std::env::temp_dir();

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
                    let _ = state_tx_clone.send(AppState::Idle);
                    return;
                }
                log(&format!("Download error: {}", e));
                let _ = state_tx_clone.send(AppState::Error);
                return;
            }

            let _ = dl_handle.await;
            log("Download complete");
            crate::debug::log("Download complete");

            // Step 3: Format drive
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
                            FormatProgress::CleaningDisk => {
                                p.message = "Cleaning disk...".to_string();
                            }
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

            if let Err(e) = format_drive_fat32(&drive.device_path, &volume_label, fmt_tx, cancel_token_clone.clone()).await {
                if e.contains("cancelled") {
                    log("Format cancelled");
                    let _ = state_tx_clone.send(AppState::Idle);
                    return;
                }
                log(&format!("Format error: {}", e));
                let _ = state_tx_clone.send(AppState::Error);
                return;
            }

            let _ = fmt_handle.await;
            log("Format complete");
            crate::debug::log("Format complete");

            // Get the destination path for extraction (platform-specific)
            crate::debug::log("Getting mount path after format...");
            let dest_path = match get_mount_path_after_format(&drive, &volume_label).await {
                Ok(path) => path,
                Err(e) => {
                    log(&format!("Error getting mount path: {}", e));
                    crate::debug::log(&format!("ERROR getting mount path: {}", e));
                    let _ = state_tx_clone.send(AppState::Error);
                    return;
                }
            };

            log(&format!("Destination: {}", dest_path.display()));
            crate::debug::log(&format!("Mount path: {:?}", dest_path));

            // Create a log file on the SD card for debugging
            let log_file_path = dest_path.join("install_log.txt");
            let write_card_log = |msg: &str| {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_file_path)
                {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let _ = writeln!(file, "[{}] {}", timestamp, msg);
                }
            };

            write_card_log("Format complete, starting extraction...");

            // Step 4: Extract to temp folder on local PC
            // On Linux, use cache dir (~/.cache) instead of /tmp to avoid tmpfs space issues
            #[cfg(target_os = "linux")]
            let extract_base_dir = dirs::cache_dir().unwrap_or_else(|| temp_dir.clone());
            #[cfg(not(target_os = "linux"))]
            let extract_base_dir = temp_dir.clone();

            let _ = state_tx_clone.send(AppState::Extracting);
            let temp_extract_dir = extract_base_dir.join(format!("{}_extract", TEMP_PREFIX));
            log("Extracting files to local temp folder...");
            crate::debug::log_section("Extracting Files");
            crate::debug::log(&format!("Temp extract dir: {:?}", temp_extract_dir));
            set_progress(0, 100, "Extracting files...");

            // Clean up any previous extraction
            let _ = std::fs::remove_dir_all(&temp_extract_dir);
            std::fs::create_dir_all(&temp_extract_dir)
                .map_err(|e| format!("Failed to create temp extract dir: {}", e))
                .unwrap();

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
                    let _ = state_tx_clone.send(AppState::Idle);
                    return;
                }
                write_card_log(&format!("Extract error: {}", e));
                log(&format!("Extract error: {}", e));
                let _ = std::fs::remove_dir_all(&temp_extract_dir);
                let _ = state_tx_clone.send(AppState::Error);
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

            write_card_log(&format!(
                "Copying files: {:?} -> {:?}",
                temp_extract_dir, dest_path
            ));

            if let Err(e) = copy_directory_with_progress(&temp_extract_dir, &dest_path, copy_tx, cancel_token_clone.clone()).await {
                if e.contains("cancelled") {
                    write_card_log("Copy cancelled");
                    log("Copy cancelled");
                    let _ = std::fs::remove_dir_all(&temp_extract_dir);
                    let _ = state_tx_clone.send(AppState::Idle);
                    return;
                }
                write_card_log(&format!("Copy error: {}", e));
                log(&format!("Copy error: {}", e));
                let _ = std::fs::remove_dir_all(&temp_extract_dir);
                let _ = state_tx_clone.send(AppState::Error);
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
            match crate::debug::copy_log_to(&dest_path) {
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
                        AppState::Extracting => "Extracting...".to_string(),
                        AppState::Copying => "Copying...".to_string(),
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
async fn get_mount_path_after_format(drive: &DriveInfo, _volume_label: &str) -> Result<PathBuf, String> {
    // On Windows, the drive letter remains the same after formatting
    // The mount_path should be set (e.g., "E:\")
    drive.mount_path.clone().ok_or_else(|| {
        format!("No mount path available for drive {}", drive.name)
    })
}

#[cfg(target_os = "macos")]
async fn get_mount_path_after_format(_drive: &DriveInfo, volume_label: &str) -> Result<PathBuf, String> {
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
async fn get_mount_path_after_format(drive: &DriveInfo, volume_label: &str) -> Result<PathBuf, String> {
    use tokio::process::Command;

    // Determine the partition path
    let partition_path = if drive.device_path.contains("mmcblk") || drive.device_path.contains("nvme") {
        format!("{}p1", drive.device_path)
    } else {
        format!("{}1", drive.device_path)
    };

    // Create a mount point
    let mount_point = PathBuf::from(format!("/tmp/{}_{}", TEMP_PREFIX, volume_label));

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
async fn get_mount_path_after_format(_drive: &DriveInfo, _volume_label: &str) -> Result<PathBuf, String> {
    Err("Mounting not supported on this platform".to_string())
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for state updates from async eject on Windows
        if let Ok(mut progress) = self.progress.lock() {
            if progress.message.starts_with("EJECT_") {
                if progress.message == "EJECT_SUCCESS" {
                    self.log("SD card safely ejected. You may now remove it.");
                    self.state = AppState::Ejected;
                } else if let Some(error_msg) = progress.message.strip_prefix("EJECT_ERROR: ") {
                    self.log(&format!("Eject warning: {}. The card should still be safe to remove.", error_msg));
                    self.state = AppState::Ejected;
                }
                progress.message.clear(); // Consume the message
            }
        }

        // Check for state updates from main installation process
        if let Ok(mut progress) = self.progress.lock() {
            if progress.message == "COMPLETE" {
                self.state = AppState::Complete;
                self.cancel_token = None;
                progress.message.clear();
            } else if progress.message == "ERROR" {
                self.state = AppState::Error;
                self.cancel_token = None;
                progress.message.clear();
            } else if progress.message == "CANCELLED" {
                self.state = AppState::Idle;
                self.cancel_token = None;
                progress.message.clear();
            } else if self.state == AppState::Idle || self.state == AppState::AwaitingConfirmation {
                // Only update state if we are not already in a process
                if progress.message.contains("Downloading") {
                    self.state = AppState::Downloading;
                } else if progress.message.contains("Formatting")
                    || progress.message.contains("format")
                    || progress.message.contains("Unmounting")
                    || progress.message.contains("Cleaning")
                    || progress.message.contains("partition")
                {
                    self.state = AppState::Formatting;
                } else if progress.message.contains("Extracting") || progress.message.contains("Extract")
                {
                    self.state = AppState::Extracting;
                } else if progress.message.contains("Copying") || progress.message.contains("Copy")
                {
                    self.state = AppState::Copying;
                }
            }
        }


        // Keep requesting repaints while busy so UI stays responsive
        let is_busy = matches!(
            self.state,
            AppState::FetchingRelease
                | AppState::Downloading
                | AppState::Formatting
                | AppState::Extracting
                | AppState::Copying
                | AppState::Ejecting
                | AppState::Cancelling
        );
        if is_busy {
            ctx.request_repaint();
        }

        // Show confirmation dialog if awaiting confirmation
        if self.state == AppState::AwaitingConfirmation {
            let window_frame = egui::Frame::window(&ctx.style())
                .fill(COLOR_BG_DARK)
                .stroke(egui::Stroke::new(1.0, COLOR_ACCENT_DIM));

            let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
            egui::Window::new(format!("Confirm {} Installation", selected_repo_name))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(window_frame)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(10.0);
                        ui.colored_label(COLOR_WARNING, "WARNING");
                        ui.add_space(10.0);

                        ui.label("This will DELETE ALL DATA on the selected drive:");
                        ui.add_space(5.0);

                        if let Some(idx) = self.selected_drive_idx {
                            if let Some(drive) = self.drives.get(idx) {
                                ui.colored_label(COLOR_ACCENT, drive.display_name());
                            }
                        }

                        ui.add_space(10.0);
                        ui.label("Are you sure you want to continue?");
                        ui.add_space(15.0);

                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.state = AppState::Idle;
                            }

                            ui.add_space(20.0);

                            if ui
                                .add(egui::Button::new(format!("Yes, Install {}", selected_repo_name)).fill(COLOR_ERROR))
                                .clicked()
                            {
                                self.start_installation(ctx.clone());
                            }
                        });

                        ui.add_space(10.0);
                    });
                });
        }

        let panel_frame = egui::Frame::central_panel(&ctx.style()).fill(COLOR_BG_DARK);

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                ui.heading(
                    egui::RichText::new(format!("{} Installer", APP_NAME)).color(COLOR_ACCENT),
                );
                ui.add_space(10.0);

                // Drive selection
                ui.horizontal(|ui| {
                    ui.label("Target Drive:");

                    let selected_text = self
                        .selected_drive_idx
                        .and_then(|idx| self.drives.get(idx))
                        .map(|d| d.display_name())
                        .unwrap_or_else(|| "No drives found".to_string());

                    egui::ComboBox::from_id_salt("drive_select")
                        .selected_text(&selected_text)
                        .show_ui(ui, |ui| {
                            for (idx, drive) in self.drives.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected_drive_idx,
                                    Some(idx),
                                    drive.display_name(),
                                );
                            }
                        });

                    if ui.button("Refresh").clicked() {
                        self.refresh_drives();
                    }
                });

                ui.add_space(10.0);

                // Repository selection
                ui.horizontal(|ui| {
                    ui.label("Release Channel:");

                    let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;

                    egui::ComboBox::from_id_salt("repo_select")
                        .selected_text(selected_repo_name)
                        .show_ui(ui, |ui| {
                            for (idx, (name, _url)) in REPO_OPTIONS.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_repo_idx, idx, *name);
                            }
                        });
                });

                ui.add_space(10.0);

                // Install button
                let is_busy = matches!(
                    self.state,
                    AppState::FetchingRelease
                        | AppState::Downloading
                        | AppState::Formatting
                        | AppState::Extracting
                        | AppState::Copying
                        | AppState::AwaitingConfirmation
                        | AppState::Ejecting
                        | AppState::Cancelling
                );

                ui.add_enabled_ui(!is_busy && self.selected_drive_idx.is_some(), |ui| {
                    let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
                    if ui.button(format!("Install {}", selected_repo_name)).clicked() {
                        self.state = AppState::AwaitingConfirmation;
                    }
                });

                ui.add_space(10.0);

                // Progress bar
                let show_progress = matches!(
                    self.state,
                    AppState::FetchingRelease
                        | AppState::Downloading
                        | AppState::Formatting
                        | AppState::Extracting
                        | AppState::Copying
                        | AppState::Cancelling
                );

                if show_progress {
                    let (current, total, message) = {
                        let p = self.progress.lock().unwrap();
                        (p.current, p.total, p.message.clone())
                    };

                    // Only FetchingRelease has indeterminate progress
                    // Downloading, Formatting, and Extracting now report percentages
                    let is_indeterminate = matches!(
                        self.state,
                        AppState::FetchingRelease
                    );

                    if is_indeterminate {
                        // Animated indeterminate progress bar
                        let time = ctx.input(|i| i.time);

                        // Allocate space for the progress bar
                        let desired_size = egui::vec2(ui.available_width(), 20.0);
                        let (rect, _response) =
                            ui.allocate_exact_size(desired_size, egui::Sense::hover());

                        if ui.is_rect_visible(rect) {
                            let painter = ui.painter();

                            // Background
                            painter.rect_filled(rect, 4.0, COLOR_BG_LIGHT);

                            // Animated highlight - moves back and forth
                            let cycle = (time * 0.8).sin() * 0.5 + 0.5; // 0.0 to 1.0
                            let bar_width = rect.width() * 0.3;
                            let bar_x = rect.left() + (rect.width() - bar_width) * cycle as f32;

                            let highlight_rect = egui::Rect::from_min_size(
                                egui::pos2(bar_x, rect.top()),
                                egui::vec2(bar_width, rect.height()),
                            );

                            painter.rect_filled(highlight_rect, 4.0, COLOR_ACCENT);
                        }
                    } else {
                        // Normal progress bar for downloading
                        let progress = if total > 0 {
                            current as f32 / total as f32
                        } else {
                            0.0
                        };

                        ui.add(
                            egui::ProgressBar::new(progress)
                                .fill(COLOR_ACCENT)
                                .show_percentage(),
                        );
                    }

                    ui.add_space(5.0);
                    ui.colored_label(COLOR_TEXT, &message);

                    // Cancel button (only show during cancellable operations)
                    let can_cancel = matches!(
                        self.state,
                        AppState::FetchingRelease
                            | AppState::Downloading
                            | AppState::Formatting
                            | AppState::Extracting
                            | AppState::Copying
                    ) && self.cancel_token.is_some();

                    if can_cancel {
                        ui.add_space(10.0);
                        if ui.button("Cancel").clicked() {
                            self.cancel_installation();
                        }
                    }

                    // Show cancelling message
                    if self.state == AppState::Cancelling {
                        ui.add_space(5.0);
                        ui.colored_label(COLOR_WARNING, "Cancelling...");
                    }
                }

                // Status
                match self.state {
                    AppState::Complete => {
                        let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
                        ui.colored_label(COLOR_SUCCESS, format!("{} installation complete!", selected_repo_name));
                        ui.add_space(5.0);
                        if ui.button("Safely Eject SD Card").clicked() {
                            if let Some(drive) = self.installed_drive.clone() {
                                #[cfg(target_os = "windows")]
                                {
                                    // WINDOWS: Run in background task
                                    self.state = AppState::Ejecting;
                                    self.log("Ejecting SD card...");
                                    
                                    let progress = self.progress.clone();
                                    let ctx_clone = ctx.clone();

                                    self.runtime.spawn(async move {
                                        let result = tokio::task::spawn_blocking(move || {
                                            eject_drive(&drive)
                                        }).await.unwrap();

                                        if let Ok(mut progress) = progress.lock() {
                                            match result {
                                                Ok(()) => progress.message = "EJECT_SUCCESS".to_string(),
                                                Err(e) => progress.message = format!("EJECT_ERROR: {}", e),
                                            }
                                        }
                                        ctx_clone.request_repaint();
                                    });
                                }

                                #[cfg(not(target_os = "windows"))]
                                {
                                    // OTHER PLATFORMS: Run synchronously
                                    match eject_drive(&drive) {
                                        Ok(()) => {
                                            self.log("SD card safely ejected. You may now remove it.");
                                            self.state = AppState::Ejected;
                                        }
                                        Err(e) => {
                                            self.log(&format!("Eject warning: {}. The card should still be safe to remove.", e));
                                            self.state = AppState::Ejected;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    AppState::Ejecting => {
                        let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
                        ui.colored_label(COLOR_SUCCESS, format!("{} installation complete!", selected_repo_name));
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(" Ejecting SD card...");
                        });
                    }
                    AppState::Ejected => {
                        ui.colored_label(COLOR_SUCCESS, "SD card ejected! You may safely remove it.");
                    }
                    AppState::Error => {
                        let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
                        ui.colored_label(COLOR_ERROR, format!("{} installation failed. See log for details.", selected_repo_name));
                    }
                    _ => {}
                }

                ui.add_space(10.0);

                // Log area
                ui.separator();
                ui.label("Log:");

                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if let Ok(logs) = self.log_messages.lock() {
                            for msg in logs.iter() {
                                ui.label(msg);
                            }
                        }
                    });
            });
    }
}