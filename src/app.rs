use crate::config::{
    setup_theme, ASSET_EXTENSION, REPO_OPTIONS, TEMP_PREFIX, VOLUME_LABEL, DEFAULT_REPO_INDEX,
};
use crate::copy::{copy_directory_with_progress, CopyProgress};
use crate::drives::{get_removable_drives, DriveInfo};
use crate::eject::eject_drive;
use crate::extract::{extract_7z_with_progress, ExtractProgress};
use crate::format::{format_drive_fat32, FormatProgress};
use crate::github::{download_asset, find_release_asset, get_latest_release, DownloadProgress};
use eframe::egui;
use egui_thematic::{ThemeConfig, ThemeEditorState, render_theme_panel};
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

    // Progress tracking
    state: AppState,
    progress: Arc<Mutex<ProgressInfo>>,
    log_messages: Arc<Mutex<Vec<String>>>,

    // Drive that was installed to (for eject)
    installed_drive: Option<DriveInfo>,

    // Cancellation token for aborting installation
    cancel_token: Option<CancellationToken>,

    // Channel for background drive updates
    drive_rx: mpsc::UnboundedReceiver<Vec<DriveInfo>>,
    drive_poll_tx: mpsc::UnboundedSender<bool>,

    // Theme editor
    theme_state: ThemeEditorState,
    show_theme_editor: bool,
    show_log: bool,
    last_system_dark_mode: bool,
}

/// Get available disk space for a given path (in bytes)
fn get_available_disk_space(path: &std::path::Path) -> u64 {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

        let path_wide: Vec<u16> = path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();

        let mut free_bytes = 0u64;
        unsafe {
            if GetDiskFreeSpaceExW(
                windows::core::PCWSTR(path_wide.as_ptr()),
                None,
                None,
                Some(&mut free_bytes),
            ).is_ok() {
                return free_bytes;
            }
        }
        crate::debug::log("WARNING: Failed to get disk space on Windows, assuming sufficient space");
        u64::MAX // Assume sufficient space if we can't check
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let path_cstr = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap_or_default();
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };

        unsafe {
            if libc::statvfs(path_cstr.as_ptr(), &mut stat) == 0 {
                // Available space = block size * available blocks
                // Cast both to u64 to handle platforms where they're u32 (macOS, ARM32)
                return (stat.f_bavail as u64) * (stat.f_bsize as u64);
            }
        }
        crate::debug::log("WARNING: Failed to get disk space on Unix, assuming sufficient space");
        u64::MAX // Assume sufficient space if we can't check
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        crate::debug::log("WARNING: Disk space check not supported on this platform");
        u64::MAX // Assume sufficient space on unsupported platforms
    }
}

impl InstallerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Apply theme from config
        setup_theme(&cc.egui_ctx);

        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        // Start background drive polling
        let (tx, rx) = mpsc::unbounded_channel();
        let (poll_tx, mut poll_rx) = mpsc::unbounded_channel::<bool>();
        let ctx_clone = cc.egui_ctx.clone();
        
        runtime.spawn(async move {
            let mut enabled = true;
            loop {
                while let Ok(new_state) = poll_rx.try_recv() {
                    enabled = new_state;
                }

                if enabled {
                    let drives = tokio::task::spawn_blocking(get_removable_drives).await.unwrap_or_default();
                    if tx.send(drives).is_err() {
                        break;
                    }
                    ctx_clone.request_repaint();
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });

        let is_dark = cc.egui_ctx.style().visuals.dark_mode;
        
        // Initial app creation to use helper method
        let mut app = Self {
            runtime,
            drives: Vec::new(),
            selected_drive_idx: None,
            selected_repo_idx: DEFAULT_REPO_INDEX,
            state: AppState::Idle,
            progress: Arc::new(Mutex::new(ProgressInfo {
                current: 0,
                total: 100,
                message: String::new(),
            })),
            log_messages: Arc::new(Mutex::new(Vec::new())),
            installed_drive: None,
            cancel_token: None,
            drive_rx: rx,
            drive_poll_tx: poll_tx,
            theme_state: ThemeEditorState::default(),
            show_theme_editor: false,
            show_log: false,
            last_system_dark_mode: is_dark,
        };

        app.theme_state.current_config = app.get_theme_config();

        // Initial sync load
        app.drives = get_removable_drives();
        app.ensure_selection_valid();
        
        app
    }

    fn get_theme_config(&self) -> ThemeConfig {
        ThemeConfig {
            name: "SpruceOS".to_string(),
            dark_mode: true,
            override_text_color: Some([251, 241, 199, 255]),
            override_weak_text_color: Some([124, 111, 100, 255]),
            override_hyperlink_color: Some([131, 165, 152, 255]),
            override_faint_bg_color: Some([48, 48, 48, 255]),
            override_extreme_bg_color: Some([29, 32, 33, 255]),
            override_code_bg_color: Some([60, 56, 54, 255]),
            override_warn_fg_color: Some([214, 93, 14, 255]),
            override_error_fg_color: Some([204, 36, 29, 255]),
            override_window_fill: Some([40, 40, 40, 255]),
            override_window_stroke_color: None,
            override_window_stroke_width: None,
            override_window_corner_radius: None,
            override_window_shadow_size: None,
            override_panel_fill: Some([40, 40, 40, 255]),
            override_popup_shadow_size: None,
            override_selection_bg: Some([215, 180, 95, 255]),
            override_selection_stroke_color: None,
            override_selection_stroke_width: None,
            override_widget_noninteractive_bg_fill: None,
            override_widget_noninteractive_weak_bg_fill: None,
            override_widget_noninteractive_bg_stroke_color: None,
            override_widget_noninteractive_bg_stroke_width: None,
            override_widget_noninteractive_corner_radius: None,
            override_widget_noninteractive_fg_stroke_color: None,
            override_widget_noninteractive_fg_stroke_width: None,
            override_widget_noninteractive_expansion: None,
            override_widget_inactive_bg_fill: Some([215, 180, 95, 255]),
            override_widget_inactive_weak_bg_fill: None,
            override_widget_inactive_bg_stroke_color: Some([124, 111, 100, 100]),
            override_widget_inactive_bg_stroke_width: None,
            override_widget_inactive_corner_radius: None,
            override_widget_inactive_fg_stroke_color: Some([104, 157, 106, 255]),
            override_widget_inactive_fg_stroke_width: None,
            override_widget_inactive_expansion: None,
            override_widget_hovered_bg_fill: Some([215, 180, 95, 60]),
            override_widget_hovered_weak_bg_fill: None,
            override_widget_hovered_bg_stroke_color: Some([215, 180, 95, 255]),
            override_widget_hovered_bg_stroke_width: None,
            override_widget_hovered_corner_radius: None,
            override_widget_hovered_fg_stroke_color: None,
            override_widget_hovered_fg_stroke_width: None,
            override_widget_hovered_expansion: None,
            override_widget_active_bg_fill: Some([215, 180, 95, 100]),
            override_widget_active_weak_bg_fill: None,
            override_widget_active_bg_stroke_color: Some([215, 180, 95, 255]),
            override_widget_active_bg_stroke_width: None,
            override_widget_active_corner_radius: None,
            override_widget_active_fg_stroke_color: None,
            override_widget_active_fg_stroke_width: None,
            override_widget_active_expansion: None,
            override_widget_open_bg_fill: None,
            override_widget_open_weak_bg_fill: None,
            override_widget_open_bg_stroke_color: None,
            override_widget_open_bg_stroke_width: None,
            override_widget_open_corner_radius: None,
            override_widget_open_fg_stroke_color: None,
            override_widget_open_fg_stroke_width: None,
            override_widget_open_expansion: None,
            override_resize_corner_size: None,
            override_text_cursor_width: None,
            override_clip_rect_margin: None,
            override_button_frame: None,
            override_collapsing_header_frame: None,
            override_indent_has_left_vline: None,
            override_striped: None,
            override_slider_trailing_fill: None,
        }
    }

    fn ensure_selection_valid(&mut self) {
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
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }
            };

            let asset = match find_release_asset(&release) {
                Some(a) => a,
                None => {
                    log(&format!("Error: No {} file found in release", ASSET_EXTENSION));
                    crate::debug::log(&format!("ERROR: No {} asset found in release", ASSET_EXTENSION));
                    let _ = state_tx_clone.send(AppState::Error);
                    let _ = drive_poll_tx_clone.send(true);
                    return;
                }
            };

            log(&format!(
                "Found release: {} ({})",
                release.tag_name, asset.name
            ));
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

            // Step 2: Format drive (do this first so we fail fast if the card has issues)
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

            if let Err(e) = format_drive_fat32(&drive.device_path, &volume_label, fmt_tx, cancel_token_clone.clone()).await {
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

            // Get the destination path for extraction (platform-specific)
            crate::debug::log("Getting mount path after format...");
            let dest_path = match get_mount_path_after_format(&drive, &volume_label).await {
                Ok(path) => path,
                Err(e) => {
                    log(&format!("Error getting mount path: {}", e));
                    crate::debug::log(&format!("ERROR getting mount path: {}", e));
                    let _ = state_tx_clone.send(AppState::Error);
                    let _ = drive_poll_tx_clone.send(true);
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

            write_card_log("Format complete, starting download...");

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

            write_card_log(&format!(
                "Copying files: {:?} -> {:?}",
                temp_extract_dir, dest_path
            ));

            if let Err(e) = copy_directory_with_progress(&temp_extract_dir, &dest_path, copy_tx, cancel_token_clone.clone()).await {
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
            let _ = drive_poll_tx_clone.send(true);
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

    // Fallback: use raw mount if udisksctl fails (e.g., no udisks2 daemon)
    crate::debug::log("udisksctl mount failed, falling back to raw mount...");
    let stderr = String::from_utf8_lossy(&output.stderr);
    crate::debug::log(&format!("udisksctl error: {}", stderr.trim()));

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
async fn get_mount_path_after_format(_drive: &DriveInfo, _volume_label: &str) -> Result<PathBuf, String> {
    Err("Mounting not supported on this platform".to_string())
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Show modal dialogs for confirmation or status
        let show_modal = matches!(
            self.state,
            AppState::AwaitingConfirmation
                | AppState::Complete
                | AppState::Ejecting
                | AppState::Ejected
                | AppState::Error
        );

        // Sync with system theme if it changes
        let is_dark = ctx.style().visuals.dark_mode;
        if is_dark != self.last_system_dark_mode {
            self.last_system_dark_mode = is_dark;
            self.theme_state.current_config = self.get_theme_config();
        }

        // Theme editor panel
        if !show_modal {
            render_theme_panel(ctx, &mut self.theme_state, &mut self.show_theme_editor);

            // Keyboard shortcut to toggle theme editor (Ctrl+T)
            if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::T))) {
                self.show_theme_editor = !self.show_theme_editor;
            }
        }

        // Poll for drive updates
        while let Ok(drives) = self.drive_rx.try_recv() {
            self.drives = drives;
            self.ensure_selection_valid();
        }

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
            } else {
                // Update state based on progress message
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

        if show_modal {
            // Background Dimmer
            egui::Area::new(egui::Id::from("modal_dimmer"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    let screen_rect = ui.ctx().content_rect();
                    ui.allocate_rect(screen_rect, egui::Sense::click()); // Block clicks
                    ui.painter()
                        .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(140));
                });

            let window_frame = egui::Frame::window(&ctx.style())
                .fill(ctx.style().visuals.window_fill)
                .stroke(ctx.style().visuals.window_stroke);

            let window_title = match self.state {
                AppState::AwaitingConfirmation => {
                    let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
                    format!("Confirm {} Installation", selected_repo_name)
                }
                AppState::Complete => "Installation Complete".to_string(),
                AppState::Ejecting => "Ejecting...".to_string(),
                AppState::Ejected => "Safe to Remove".to_string(),
                AppState::Error => "Installation Error".to_string(),
                _ => String::new(),
            };

            egui::Window::new(window_title)
                .order(egui::Order::Foreground)
                .collapsible(false)
                .resizable(false)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(window_frame)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        match self.state {
                            AppState::AwaitingConfirmation => {
                                ui.add_space(12.0);
                                ui.colored_label(ui.visuals().warn_fg_color, "WARNING");
                                ui.add_space(12.0);

                                ui.label("This will DELETE ALL DATA on the selected drive:");
                                ui.add_space(8.0);

                                if let Some(idx) = self.selected_drive_idx {
                                    if let Some(drive) = self.drives.get(idx) {
                                        ui.label(drive.display_name());
                                    }
                                }

                                ui.add_space(12.0);
                                ui.label("Are you sure you want to continue?");
                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.columns(2, |columns| {
                                    columns[0].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Cancel").clicked() {
                                                self.state = AppState::Idle;
                                            }
                                        },
                                    );

                                    columns[1].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Yes, install").clicked() {
                                                self.start_installation(ctx.clone());
                                            }
                                        },
                                    );
                                });
                            }
                            AppState::Complete => {
                                ui.add_space(12.0);
                                ui.colored_label(egui::Color32::from_rgb(104, 157, 106), "SUCCESS");
                                ui.add_space(12.0);
                                let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
                                ui.label(format!("{} has been successfully installed.", selected_repo_name));
                                ui.add_space(15.0);
                                ui.separator();
                                ui.add_space(8.0);
                                
                                ui.columns(2, |columns| {
                                    columns[0].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Close").clicked() {
                                                self.state = AppState::Idle;
                                            }
                                        },
                                    );

                                    columns[1].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Safely Eject").clicked() {
                                                if let Some(drive) = self.installed_drive.clone() {
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
                                            }
                                        },
                                    );
                                });
                            }
                            AppState::Ejecting => {
                                ui.add_space(12.0);
                                ui.add(egui::Spinner::new().color(ui.visuals().selection.bg_fill));
                                ui.add_space(8.0);
                                ui.label("Ejecting SD card...");
                                ui.add_space(12.0);
                            }
                            AppState::Ejected => {
                                ui.add_space(12.0);
                                ui.label("SD card ejected!");
                                ui.add_space(8.0);
                                ui.colored_label(egui::Color32::from_rgb(104, 157, 106), "You may now safely remove it.");
                                ui.add_space(15.0);
                                if ui.button("OK").clicked() {
                                    self.state = AppState::Idle;
                                }
                            }
                            AppState::Error => {
                                ui.add_space(12.0);
                                ui.colored_label(ui.visuals().error_fg_color, "FAILED");
                                ui.add_space(12.0);
                                let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].0;
                                ui.label(format!("{} installation failed.", selected_repo_name));
                                ui.add_space(8.0);
                                ui.label("Check the log for details.");
                                ui.add_space(15.0);
                                if ui.button("OK").clicked() {
                                    self.state = AppState::Idle;
                                }
                            }
                            _ => {}
                        }
                        ui.add_space(8.0);
                    });
                });
        }

        if self.show_log {
            egui::SidePanel::right("log_panel")
                .resizable(true)
                .default_width(320.0)
                .min_width(200.0)
                .show(ctx, |ui| {
                    ui.add_enabled_ui(!show_modal, |ui| {
                        ui.vertical(|ui| {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.heading("Debug Log");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("X").on_hover_text("Close Log").clicked() {
                                        self.show_log = false;
                                        let current_size = ui.ctx().content_rect().size();
                                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(current_size.x - 320.0, current_size.y)));
                                    }
                                });
                            });

                            // Copy to Clipboard button
                            ui.horizontal(|ui| {
                                if ui.button("📋 Copy to Clipboard").clicked() {
                                    let log_path = crate::debug::get_log_path();
                                    if let Ok(contents) = std::fs::read_to_string(&log_path) {
                                        ui.ctx().copy_text(contents);
                                    }
                                }
                                ui.label(format!("Log: {:?}", crate::debug::get_log_path().file_name().unwrap_or_default()));
                            });

                            ui.separator();

                            egui::ScrollArea::vertical()
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());

                                    // Read and display debug log file
                                    let log_path = crate::debug::get_log_path();
                                    match std::fs::read_to_string(&log_path) {
                                        Ok(contents) => {
                                            ui.add(
                                                egui::TextEdit::multiline(&mut contents.as_str())
                                                    .font(egui::TextStyle::Monospace)
                                                    .desired_width(f32::INFINITY)
                                                    .interactive(false)
                                            );
                                        }
                                        Err(e) => {
                                            ui.label(format!("Failed to read log file: {}", e));
                                        }
                                    }
                                });
                        });
                    });
                });
        }

        let panel_frame = egui::Frame::central_panel(&ctx.style()).fill(ctx.style().visuals.panel_fill);

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!show_modal, |ui| {
                    let show_progress = matches!(
                        self.state,
                        AppState::FetchingRelease
                            | AppState::Downloading
                            | AppState::Formatting
                            | AppState::Extracting
                            | AppState::Copying
                            | AppState::Cancelling
                    );

                    //ctx.set_zoom_factor(1.15);
                    //ui.style_mut().spacing.item_spacing = egui::vec2(16.0, 16.0);

                    ui.columns(3, |columns| {
                    columns[0].allocate_ui_with_layout(
                        egui::Vec2::ZERO,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |_ui| {

                        }
                    );

                    columns[1].allocate_ui_with_layout(
                        egui::Vec2::ZERO,
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space(12.0);
                            let is_dark = ctx.style().visuals.dark_mode;
                            let image = if is_dark {
                                egui::include_image!("../assets/Icons/icon_dark.png")
                            } else {
                                egui::include_image!("../assets/Icons/icon.png")
                            };
                            ui.add(egui::Image::new(image).fit_to_exact_size(egui::vec2(60.0, 60.0)));
                        },
                    );

                    columns[2].allocate_ui_with_layout(
                        egui::Vec2::ZERO,
                        egui::Layout::right_to_left(egui::Align::TOP),
                        |ui| {
                            if ui.button("🎨").on_hover_text("Toggle Theme Editor (Ctrl+T)").clicked() {
                                self.show_theme_editor = !self.show_theme_editor;
                            }
                            if ui.button("📜").on_hover_text("Toggle Log Area").clicked() {
                                self.show_log = !self.show_log;
                                
                                // Adjust window size when toggling log
                                let current_size = ctx.content_rect().size();
                                let new_width = if self.show_log {
                                    current_size.x + 320.0
                                } else {
                                    current_size.x - 320.0
                                };
                                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(new_width, current_size.y)));
                            }
                            },
                    );
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(16.0);
                ui.columns(2, |columns| {
                    columns[0].allocate_ui_with_layout(
                        egui::Vec2::ZERO,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            // Drive selection
                            let (selected_text, enabled) = if self.drives.is_empty() {
                                ("No SD card".to_string(), false)
                            } else {
                                (
                                    self.selected_drive_idx
                                        .and_then(|idx| self.drives.get(idx))
                                        .map(|d| d.display_name())
                                        .unwrap_or_else(|| "Select Drive".to_string()),
                                    !show_progress
                                )
                            };

                            ui.add_enabled_ui(enabled, |ui| {
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
                            });
                        }
                    );

                    columns[1].allocate_ui_with_layout(
                        egui::Vec2::ZERO,
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add_enabled_ui(!show_progress, |ui| {
                                // Repository selection
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let count = REPO_OPTIONS.len();

                                for (idx, (name, _url)) in REPO_OPTIONS.iter().enumerate() {
                                    let corner_radius = if count == 1 {
                                        egui::CornerRadius::same(4)
                                    } else if idx == 0 {
                                        egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }
                                    } else if idx == count - 1 {
                                        egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 }
                                    } else {
                                        egui::CornerRadius::ZERO
                                    };

                                    ui.scope(|ui| {
                                        ui.visuals_mut().widgets.inactive.corner_radius = corner_radius;
                                        ui.visuals_mut().widgets.hovered.corner_radius = corner_radius;
                                        ui.visuals_mut().widgets.active.corner_radius = corner_radius;

                                        if ui.add(egui::Button::selectable(
                                            self.selected_repo_idx == idx,
                                            *name,
                                        )).clicked() {
                                            self.selected_repo_idx = idx;
                                        }
                                    });
                                }
                            });
                        },
                    );
                });

                ui.add_space(12.0);

                // Progress bar
                if show_progress {
                    
                    let (current, total, message) = {
                        let p = self.progress.lock().unwrap();
                        (p.current, p.total, p.message.clone())
                    };

                    ui.horizontal(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(&message);
                        });
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.vertical_centered(|ui| {
                            
                            // Only FetchingRelease has indeterminate progress
                            // Downloading, Formatting, and Extracting now report percentages
                            let is_indeterminate = matches!(
                                self.state,
                                AppState::FetchingRelease | AppState::Idle
                            );

                            if is_indeterminate {
                                // Animated indeterminate progress bar
                                let time = ctx.input(|i| i.time);

                                // Allocate space for the progress bar
                                let desired_size = egui::vec2(ui.available_width() / 2.0, 6.0);
                                let (outer_rect, _response) =
                                    ui.allocate_exact_size(desired_size, egui::Sense::hover());

                                if ui.is_rect_visible(outer_rect) {
                                    let visuals = ui.style().visuals.clone();
                                    let half_height = outer_rect.height() / 2.0;
                                    let corner_radius = half_height;
                                    ui.painter()
                                        .rect_filled(outer_rect, corner_radius, visuals.extreme_bg_color);

                                    // Animated highlight - moves back and forth
                                    let cycle = (time * 0.8).sin() * 0.5 + 0.5; // 0.0 to 1.0
                                    let bar_width = outer_rect.width() * 0.3;
                                    let bar_x = outer_rect.left() + (outer_rect.width() - bar_width) * cycle as f32;

                                    let highlight_rect = egui::Rect::from_min_size(
                                        egui::pos2(bar_x, outer_rect.top()),
                                        egui::vec2(bar_width, outer_rect.height()),
                                    );

                                    ui.painter().rect_filled(
                                        highlight_rect,
                                        corner_radius,
                                        visuals.selection.bg_fill);
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
                                        .fill(ui.visuals().selection.bg_fill).desired_height(16.0).desired_width(ui.available_width() / 2.0)
                                );
                            }
                        });
                    });
                    ui.add_space(12.0);
                }


                ui.horizontal(|ui| {
                    ui.vertical_centered(|ui| {
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
                        
                        if !is_busy {
                            ui.add_enabled_ui(!is_busy && self.selected_drive_idx.is_some() && !self.drives.is_empty(), |ui| {
                                let button = egui::Button::new("Install")
                                    .min_size(egui::vec2(96.0, 48.0))
                                    .fill(egui::Color32::from_rgb(104, 157, 106)); // Green
                                if ui.add(button).clicked() {
                                    self.state = AppState::AwaitingConfirmation;
                                }
                            });
                        }

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
                            let button = egui::Button::new("Cancel")
                                .min_size(egui::vec2(96.0, 48.0))
                                .fill(egui::Color32::from_rgb(251, 73, 52)); // Red
                            if ui.add(button).clicked() {
                                self.cancel_installation();
                            }
                        }
                    });
                });

                ui.add_space(10.0);
            });
        });
    }
}