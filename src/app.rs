use crate::drives::{get_removable_drives, DriveInfo};
use crate::extract::{extract_7z_with_progress, ExtractProgress};
use crate::format::{format_drive_fat32, FormatProgress};
use crate::github::{download_asset, find_7z_asset, get_latest_release, DownloadProgress, Release};
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

// ============================================================================
// CONFIGURATION - Change this to point to your GitHub repository
// ============================================================================
const DEFAULT_GITHUB_REPO: &str = "spruceUI/spruceOS";
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum AppState {
    Idle,
    FetchingRelease,
    Downloading,
    Formatting,
    Extracting,
    Complete,
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
    github_repo: String,
    release_info: Option<Release>,

    // Progress tracking
    state: AppState,
    progress: Arc<Mutex<ProgressInfo>>,
    log_messages: Arc<Mutex<Vec<String>>>,

    // Temp file for downloads
    temp_download_path: Option<PathBuf>,
}

impl InstallerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        let mut app = Self {
            runtime,
            drives: Vec::new(),
            selected_drive_idx: None,
            github_repo: String::from(DEFAULT_GITHUB_REPO),
            release_info: None,
            state: AppState::Idle,
            progress: Arc::new(Mutex::new(ProgressInfo {
                current: 0,
                total: 100,
                message: String::new(),
            })),
            log_messages: Arc::new(Mutex::new(Vec::new())),
            temp_download_path: None,
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

    fn set_progress(&self, current: u64, total: u64, message: &str) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.current = current;
            progress.total = total;
            progress.message = message.to_string();
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

        self.state = AppState::FetchingRelease;
        self.log(&format!("Starting installation to drive {}:", drive.letter));

        let repo_url = self.github_repo.clone();
        let progress = self.progress.clone();
        let log_messages = self.log_messages.clone();
        let ctx_clone = ctx.clone();

        // Channel for state updates
        let (state_tx, mut state_rx) = mpsc::unbounded_channel::<AppState>();

        // Clone values for the async block
        let state_tx_clone = state_tx.clone();

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
            set_progress(0, 100, "Fetching release info...");

            let release = match get_latest_release(&repo_url).await {
                Ok(r) => r,
                Err(e) => {
                    log(&format!("Error: {}", e));
                    let _ = state_tx_clone.send(AppState::Error);
                    return;
                }
            };

            let asset = match find_7z_asset(&release) {
                Some(a) => a,
                None => {
                    log("Error: No .7z file found in release");
                    let _ = state_tx_clone.send(AppState::Error);
                    return;
                }
            };

            log(&format!(
                "Found release: {} ({})",
                release.tag_name, asset.name
            ));

            // Step 2: Download
            let _ = state_tx_clone.send(AppState::Downloading);
            log("Downloading release...");

            let temp_dir = std::env::temp_dir();
            let download_path = temp_dir.join(&asset.name);

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
                        DownloadProgress::Error(e) => {
                            if let Ok(mut p) = progress_clone.lock() {
                                p.message = format!("Download error: {}", e);
                            }
                        }
                    }
                    ctx_dl.request_repaint();
                }
            });

            if let Err(e) = download_asset(&asset_clone, &download_path_clone, dl_tx).await {
                log(&format!("Download error: {}", e));
                let _ = state_tx_clone.send(AppState::Error);
                return;
            }

            let _ = dl_handle.await;
            log("Download complete");

            // Step 3: Format drive
            let _ = state_tx_clone.send(AppState::Formatting);
            log(&format!("Formatting drive {}:...", drive.letter));
            set_progress(0, 100, "Formatting drive...");

            let (fmt_tx, mut fmt_rx) = mpsc::unbounded_channel::<FormatProgress>();
            let progress_fmt = progress.clone();
            let ctx_fmt = ctx_clone.clone();

            // Spawn format progress handler
            let fmt_handle = tokio::spawn(async move {
                while let Some(prog) = fmt_rx.recv().await {
                    let msg = match prog {
                        FormatProgress::Started => "Starting format...",
                        FormatProgress::CleaningDisk => "Cleaning disk...",
                        FormatProgress::CreatingPartition => "Creating partition...",
                        FormatProgress::Formatting => "Formatting to FAT32...",
                        FormatProgress::Completed => "Format complete",
                        FormatProgress::Error(ref e) => {
                            if let Ok(mut p) = progress_fmt.lock() {
                                p.message = format!("Format error: {}", e);
                            }
                            continue;
                        }
                    };
                    if let Ok(mut p) = progress_fmt.lock() {
                        p.message = msg.to_string();
                    }
                    ctx_fmt.request_repaint();
                }
            });

            if let Err(e) = format_drive_fat32(drive.letter, "SPRUCEOS", fmt_tx).await {
                log(&format!("Format error: {}", e));
                let _ = state_tx_clone.send(AppState::Error);
                return;
            }

            let _ = fmt_handle.await;
            log("Format complete");

            // Step 4: Extract
            let _ = state_tx_clone.send(AppState::Extracting);
            log("Extracting files to SD card...");
            set_progress(0, 100, "Extracting files...");

            let dest_path = PathBuf::from(format!("{}:\\", drive.letter));

            let (ext_tx, mut ext_rx) = mpsc::unbounded_channel::<ExtractProgress>();
            let progress_ext = progress.clone();
            let ctx_ext = ctx_clone.clone();

            // Spawn extract progress handler
            let ext_handle = tokio::spawn(async move {
                while let Some(prog) = ext_rx.recv().await {
                    match prog {
                        ExtractProgress::Started => {
                            if let Ok(mut p) = progress_ext.lock() {
                                p.message = "Starting extraction...".to_string();
                            }
                        }
                        ExtractProgress::Extracting => {
                            if let Ok(mut p) = progress_ext.lock() {
                                p.message = "Extracting files...".to_string();
                            }
                        }
                        ExtractProgress::Completed => {
                            if let Ok(mut p) = progress_ext.lock() {
                                p.message = "Extraction complete".to_string();
                            }
                        }
                        ExtractProgress::Error(e) => {
                            if let Ok(mut p) = progress_ext.lock() {
                                p.message = format!("Extract error: {}", e);
                            }
                        }
                    }
                    ctx_ext.request_repaint();
                }
            });

            if let Err(e) = extract_7z_with_progress(&download_path, &dest_path, ext_tx).await {
                log(&format!("Extract error: {}", e));
                let _ = state_tx_clone.send(AppState::Error);
                return;
            }

            let _ = ext_handle.await;
            log("Extraction complete");

            // Cleanup temp file
            let _ = tokio::fs::remove_file(&download_path).await;

            log("Installation complete! You can now safely eject the SD card.");
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
                        AppState::Complete => "COMPLETE".to_string(),
                        AppState::Error => "ERROR".to_string(),
                        _ => p.message.clone(),
                    };
                }
                ctx_state.request_repaint();
            }
        });
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for state updates from progress message
        if let Ok(progress) = self.progress.lock() {
            if progress.message == "COMPLETE" {
                self.state = AppState::Complete;
            } else if progress.message == "ERROR" {
                self.state = AppState::Error;
            } else if progress.message.contains("Downloading") {
                self.state = AppState::Downloading;
            } else if progress.message.contains("Formatting") || progress.message.contains("format") {
                self.state = AppState::Formatting;
            } else if progress.message.contains("Extracting") || progress.message.contains("Extract") {
                self.state = AppState::Extracting;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SpruceOS Installer");
            ui.add_space(10.0);

            // GitHub repo input
            ui.horizontal(|ui| {
                ui.label("GitHub Repository:");
                ui.text_edit_singleline(&mut self.github_repo);
            });
            ui.add_space(5.0);

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

            // Warning
            if self.selected_drive_idx.is_some() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Warning: This will erase ALL data on the selected drive!",
                );
            }

            ui.add_space(10.0);

            // Install button
            let is_busy = matches!(
                self.state,
                AppState::FetchingRelease
                    | AppState::Downloading
                    | AppState::Formatting
                    | AppState::Extracting
            );

            ui.add_enabled_ui(!is_busy && self.selected_drive_idx.is_some(), |ui| {
                if ui.button("Install SpruceOS").clicked() {
                    self.start_installation(ctx.clone());
                }
            });

            ui.add_space(10.0);

            // Progress bar
            if is_busy {
                let (current, total, message) = {
                    let p = self.progress.lock().unwrap();
                    (p.current, p.total, p.message.clone())
                };

                let progress = if total > 0 {
                    current as f32 / total as f32
                } else {
                    0.0
                };

                ui.add(egui::ProgressBar::new(progress).show_percentage());
                ui.label(&message);
            }

            // Status
            match self.state {
                AppState::Complete => {
                    ui.colored_label(egui::Color32::GREEN, "Installation complete!");
                }
                AppState::Error => {
                    ui.colored_label(egui::Color32::RED, "Installation failed. See log for details.");
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
