// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

use crate::config::{setup_theme, DEFAULT_REPO_INDEX};
use crate::drives::{get_removable_drives, DriveInfo};
use crate::github::{Release, Asset};
use egui_thematic::ThemeEditorState;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Idle,
    FetchingAssets,
    SelectingAsset,
    PreviewingUpdate,
    AwaitingConfirmation,
    FetchingRelease,
    Downloading,
    Formatting,
    Deleting,
    Extracting,
    Copying,
    Burning,
    Complete,
    Ejecting,
    Ejected,
    Cancelling,
    Error,
}

#[derive(Debug, Clone)]
pub struct ProgressInfo {
    pub current: u64,
    pub total: u64,
    pub message: String,
}

pub struct InstallerApp {
    // Runtime for async operations
    pub(super) runtime: Runtime,

    // UI State
    pub(super) drives: Vec<DriveInfo>,
    pub(super) selected_drive_idx: Option<usize>,
    pub(super) selected_repo_idx: usize,
    // HIDE UPDATE MODE: To completely remove, delete this field and all references
    // (Easier approach: just hide the checkbox in ui.rs - this field stays but is unused)
    pub(super) update_mode: bool,

    // Progress tracking
    pub(super) state: AppState,
    pub(super) progress: Arc<Mutex<ProgressInfo>>,
    pub(super) log_messages: Arc<Mutex<Vec<String>>>,

    // Drive that was installed to (for eject)
    pub(super) installed_drive: Option<DriveInfo>,

    // Cancellation token for aborting installation
    pub(super) cancel_token: Option<CancellationToken>,

    // Channel for background drive updates
    pub(super) drive_rx: mpsc::UnboundedReceiver<Vec<DriveInfo>>,
    pub(super) drive_poll_tx: mpsc::UnboundedSender<bool>,
    pub(super) manual_refresh_tx: mpsc::UnboundedSender<()>,

    // Asset selection
    pub(super) fetched_release: Option<Release>,
    pub(super) available_assets: Vec<Asset>,
    pub(super) selected_asset_idx: Option<usize>,
    pub(super) release_rx: Option<mpsc::UnboundedReceiver<Result<Release, String>>>,

    // Manifest support for external asset hosting
    pub(super) manifest_rx: Option<mpsc::UnboundedReceiver<Option<crate::manifest::Manifest>>>,
    pub(super) pending_release: Option<(Release, Option<&'static [&'static str]>)>,

    // Theme editor
    pub(super) theme_state: ThemeEditorState,
    pub(super) show_theme_editor: bool,
    pub(super) show_log: bool,
    pub(super) last_system_dark_mode: bool,
}

/// Get available disk space for a given path (in bytes)
pub fn get_available_disk_space(path: &std::path::Path) -> u64 {
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
        let (manual_refresh_tx, mut manual_refresh_rx) = mpsc::unbounded_channel::<()>();
        let ctx_clone = cc.egui_ctx.clone();

        runtime.spawn(async move {
            let mut enabled = true;
            let mut next_poll = tokio::time::Instant::now();

            loop {
                // Check for enable/disable messages
                while let Ok(new_state) = poll_rx.try_recv() {
                    enabled = new_state;
                }

                // Check for manual refresh requests
                let manual_refresh = manual_refresh_rx.try_recv().is_ok();

                // Poll if enabled and (time to poll OR manual refresh requested)
                if enabled && (tokio::time::Instant::now() >= next_poll || manual_refresh) {
                    let drives = tokio::task::spawn_blocking(get_removable_drives).await.unwrap_or_default();
                    if tx.send(drives).is_err() {
                        break;
                    }
                    ctx_clone.request_repaint();

                    // Schedule next auto-poll in 3 minutes
                    next_poll = tokio::time::Instant::now() + tokio::time::Duration::from_secs(180);
                }

                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });

        let is_dark = cc.egui_ctx.style().visuals.dark_mode;

        // Initial app creation to use helper method
        let mut app = Self {
            runtime,
            drives: Vec::new(),
            selected_drive_idx: None,
            selected_repo_idx: DEFAULT_REPO_INDEX,
            // HIDE UPDATE MODE: Remove this if you delete the update_mode field above
            update_mode: false,
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
            manual_refresh_tx: manual_refresh_tx,
            fetched_release: None,
            available_assets: Vec::new(),
            selected_asset_idx: None,
            release_rx: None,
            manifest_rx: None,
            pending_release: None,
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
}
