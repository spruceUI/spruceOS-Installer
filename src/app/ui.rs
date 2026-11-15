// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

// ============================================================================
// UPDATE MODE: UI references to update_mode
// ============================================================================
// This file contains UI logic that references update_mode:
// - Asset selection: Show preview modal if update_mode is enabled
// - State resets: Reset update_mode flag on completion/cancel/error
// - Window titles: "Confirm Update" vs "Confirm Installation"
// - Confirmation text: Different messages for update vs fresh install
// - THE CHECKBOX: Allows users to toggle update mode (search for "Update existing installation")
//
// CONTROLLING UPDATE MODE:
//   1. Per-repository: Set supports_update_mode field in config.rs REPO_OPTIONS
//      - true for archives (.7z, .zip) that support updates
//      - false for raw images (.img.gz) that always do full burns
//   2. Hide completely: Search for "Update existing installation (skip format)"
//      (the comment beside the main.update_mode checkbox) and comment out the
//      entire checkbox block
// ============================================================================

use super::{InstallerApp, AppState};
use crate::config::REPO_OPTIONS;
use crate::eject::eject_drive;
use eframe::egui;
use egui_thematic::render_theme_panel;
use rust_i18n::t;
use tokio::sync::mpsc;

/// Widest the drive picker may grow; a longer selection is cut off with an
/// ellipsis so the refresh button beside it stays in the window. The open
/// menu still shows full names.
const DRIVE_PICKER_MAX_WIDTH: f32 = 260.0;

/// Narrowest the install dialogs get. An auto-sized egui window otherwise
/// wraps text at its default width (about 320 points), which splits sentences
/// that used to fit on one line, more so in longer languages.
const MODAL_MIN_WIDTH: f32 = 440.0;

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Show modal dialogs for confirmation or status
        let show_modal = matches!(
            self.state,
            AppState::SelectingAsset
                | AppState::PreviewingUpdate
                | AppState::AwaitingConfirmation
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

        // Startup update check result, if it ever arrives
        if let Some(rx) = &mut self.update_rx {
            if let Ok(tag) = rx.try_recv() {
                self.newer_version = Some(tag);
                self.update_rx = None;
            }
        }

        // Check for release fetch results
        if let Some(rx) = &mut self.release_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(release) => {
                        // Check for manifest.json in release
                        let ctx_clone = ctx.clone();
                        let release_clone = release.clone();

                        // Spawn async task to check for manifest
                        let (manifest_tx, manifest_rx) = mpsc::unbounded_channel();
                        self.runtime.spawn(async move {
                            let manifest_result = crate::github::get_manifest_from_release(&release_clone).await;
                            let _ = manifest_tx.send(manifest_result);
                            ctx_clone.request_repaint();
                        });

                        // Store the release and set up for manifest check
                        self.manifest_rx = Some(manifest_rx);
                        self.pending_release = Some((release, self.selected_repo_idx));
                        self.state = AppState::FetchingAssets; // Keep in fetching state
                        self.release_rx = None;
                    }
                    Err(e) => {
                        self.log(&format!("Error fetching release: {}", e));
                        self.state = AppState::Error;
                        self.release_rx = None;
                    }
                }
                ctx.request_repaint();
            }
        }

        // Check for manifest fetch results
        if let Some(rx) = &mut self.manifest_rx {
            if let Ok(manifest_opt) = rx.try_recv() {
                // Get the pending release
                if let Some((release, repo_idx)) = self.pending_release.take() {
                    // Read the filters from the repo that was fetched, not whichever
                    // tab is selected now - the user can switch tabs mid-fetch.
                    let allowed_extensions = REPO_OPTIONS[repo_idx].allowed_extensions;
                    let excluded_patterns = REPO_OPTIONS[repo_idx].excluded_patterns;

                    // Determine which assets to use
                    let mut assets = if let Some(manifest) = manifest_opt {
                        self.log("Using external assets from manifest.json");
                        crate::debug::log("Converting manifest assets to Asset structs");

                        // Store manifest display name if provided
                        if let Some(ref display_name) = manifest.display_name {
                            self.manifest_display_name = Some(display_name.clone());
                            crate::debug::log(&format!("Using manifest display name: {}", display_name));
                        }

                        // Convert manifest assets to Asset structs
                        let manifest_assets: Vec<crate::github::Asset> = manifest.assets
                            .into_iter()
                            .map(|ma| ma.into())
                            .collect();

                        // Still apply extension filtering to manifest assets
                        let filtered = Self::filter_assets(manifest_assets, allowed_extensions, excluded_patterns);
                        Self::group_multipart_assets(filtered)
                    } else {
                        // No manifest found, use GitHub assets
                        crate::debug::log("No manifest found, using GitHub release assets");
                        self.manifest_display_name = None;  // Clear any previous manifest display name
                        let filtered = Self::filter_assets(release.assets.clone(), allowed_extensions, excluded_patterns);
                        Self::group_multipart_assets(filtered)
                    };

                    // Continue with existing asset processing logic
                    if assets.is_empty() {
                        let msg = if allowed_extensions.is_some() {
                            "No compatible files found matching the allowed extensions for this repository"
                        } else {
                            "No compatible files found in release"
                        };
                        self.log(msg);
                        self.state = AppState::Error;
                        self.manifest_rx = None;
                    } else {
                        // Device first when the manifest names one, then filename.
                        assets.sort_by(|a, b| {
                            let ka = a.display_name.as_deref().unwrap_or(&a.name);
                            let kb = b.display_name.as_deref().unwrap_or(&b.name);
                            ka.cmp(kb).then_with(|| a.name.cmp(&b.name))
                        });

                        // Check if we should auto-select
                        let (should_auto, auto_idx) = Self::should_auto_select(&assets);

                        self.fetched_release = Some(release);
                        self.available_assets = assets;

                        if should_auto {
                            // Auto-select and proceed
                            self.selected_asset_idx = auto_idx;
                            if let Some(idx) = auto_idx {
                                self.log(&format!("Auto-selected: {}", self.available_assets[idx].name));
                            }

                            // Check if selected asset is a raw image
                            let is_raw_image = auto_idx.is_some_and(|idx| {
                                Self::is_raw_image_asset(&self.available_assets[idx].name)
                            });

                            // If update mode and NOT a raw image, show preview modal; otherwise go to confirmation
                            if self.update_mode && !is_raw_image {
                                self.state = AppState::PreviewingUpdate;
                            } else {
                                // Skip preview for image mode (update mode doesn't apply to raw images)
                                self.state = AppState::AwaitingConfirmation;
                            }
                        } else {
                            // Show asset selection UI
                            self.selected_asset_idx = Some(0); // Pre-select first item
                            self.state = AppState::SelectingAsset;
                        }

                        self.manifest_rx = None;
                    }
                }
                ctx.request_repaint();
            }
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
                self.update_mode = false; // Reset update mode
                self.preserve_data = true; // Reset preserve data to default
                self.was_just_paused = false; // Clear pause flag on completion
                progress.message.clear();
            } else if progress.message == "ERROR" {
                self.state = AppState::Error;
                self.cancel_token = None;
                self.update_mode = false; // Reset update mode
                self.preserve_data = true; // Reset preserve data to default
                self.was_just_paused = false; // Clear pause flag on error
                progress.message.clear();
            } else if progress.message == "CANCELLED" || progress.message.contains("Download paused at") {
                self.state = AppState::Idle;
                self.cancel_token = None;
                self.update_mode = false; // Reset update mode
                self.preserve_data = true; // Reset preserve data to default
                self.manifest_display_name = None;
                progress.message.clear();
            } else if progress.message.starts_with("SCRAPE_COMPLETE") {
                // Parse stats from message
                let stats = if let Some(stats_str) = progress.message.strip_prefix("SCRAPE_COMPLETE:") {
                    let parts: Vec<&str> = stats_str.split(':').collect();
                    if parts.len() == 4 {
                        crate::boxart_scraper::ScrapeStats {
                            total: parts[0].parse().unwrap_or(0),
                            succeeded: parts[1].parse().unwrap_or(0),
                            skipped: parts[2].parse().unwrap_or(0),
                            failed: parts[3].parse().unwrap_or(0),
                        }
                    } else {
                        crate::boxart_scraper::ScrapeStats::default()
                    }
                } else {
                    crate::boxart_scraper::ScrapeStats::default()
                };
                self.scraper_stats = Some(stats);
                self.state = AppState::Idle;
                self.cancel_token = None;
                progress.message.clear();
            } else if progress.message == "SCRAPE_ERROR" {
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
                } else if progress.message.contains("Backing up") || progress.message.contains("Backup")
                {
                    self.state = AppState::BackingUp;
                } else if progress.message.contains("Deleting") || progress.message.contains("deletion")
                {
                    self.state = AppState::Deleting;
                } else if progress.message.contains("Extracting") || progress.message.contains("Extract")
                {
                    self.state = AppState::Extracting;
                } else if progress.message.contains("Copying") || progress.message.contains("Copy")
                {
                    self.state = AppState::Copying;
                } else if progress.message.contains("Restoring") || progress.message.contains("Restore")
                {
                    self.state = AppState::Restoring;
                }
            }
        }


        // Keep requesting repaints while busy so UI stays responsive
        let is_busy = matches!(
            self.state,
            AppState::FetchingAssets
                | AppState::FetchingRelease
                | AppState::Downloading
                | AppState::Formatting
                | AppState::BackingUp
                | AppState::Deleting
                | AppState::Extracting
                | AppState::Copying
                | AppState::Restoring
                | AppState::Ejecting
                | AppState::Cancelling
                | AppState::ScrapingBoxart
        );
        if is_busy {
            ctx.request_repaint();
        }

        if show_modal {
            // Background Dimmer - paint at Background layer, below everything
            let screen_rect = ctx.viewport_rect();
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::from("modal_dimmer"),
            ))
            .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(140));

            let window_frame = egui::Frame::window(&ctx.style())
                .fill(ctx.style().visuals.window_fill)
                .stroke(ctx.style().visuals.window_stroke);

            let window_title = match self.state {
                AppState::SelectingAsset => "Select Download".to_string(),
                AppState::PreviewingUpdate => "Update Confirmation".to_string(),
                AppState::AwaitingConfirmation => {
                    let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].name;
                    if self.update_mode {
                        format!("Confirm {} Update", selected_repo_name)
                    } else {
                        format!("Confirm {} Installation", selected_repo_name)
                    }
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
                    ui.set_min_width(MODAL_MIN_WIDTH);
                    ui.vertical_centered(|ui| {
                        match self.state {
                            AppState::SelectingAsset => {
                                ui.add_space(12.0);
                                ui.heading(t!("select_asset.heading"));
                                ui.add_space(12.0);

                                // Get display mappings from current repo
                                let repo_option = &REPO_OPTIONS[self.selected_repo_idx];
                                let display_mappings = repo_option.asset_display_mappings;

                                // Scrollable list of assets
                                egui::ScrollArea::vertical()
                                    .max_height(300.0)
                                    .show(ui, |ui| {
                                        for (idx, asset) in self.available_assets.iter().enumerate() {
                                            let is_selected = self.selected_asset_idx == Some(idx);

                                            // Check for manifest display info first, then fall back to config mappings
                                            let has_manifest_info = asset.display_name.is_some() || asset.devices.is_some();

                                            // Try to find a display mapping for this asset (only if no manifest info)
                                            let mapping = if !has_manifest_info {
                                                display_mappings.and_then(|mappings| {
                                                    mappings.iter().find(|m| asset.name.contains(m.pattern))
                                                })
                                            } else {
                                                None
                                            };

                                            ui.group(|ui| {
                                                ui.set_min_width(400.0);

                                                if has_manifest_info {
                                                    // Use manifest display info
                                                    let label_response = ui.vertical(|ui| {
                                                        let display_text = asset.display_name.as_ref()
                                                            .map(|s| s.as_str())
                                                            .unwrap_or(&asset.name);
                                                        let response = ui.selectable_label(is_selected, display_text);

                                                        // Show devices if available
                                                        if let Some(devices) = &asset.devices {
                                                            ui.add_space(2.0);
                                                            ui.label(
                                                                egui::RichText::new(devices)
                                                                    .small()
                                                                    .color(ui.style().visuals.weak_text_color())
                                                            );
                                                        }

                                                        // Only show filename if we have a display name
                                                        if asset.display_name.is_some() {
                                                            ui.add_space(2.0);
                                                            ui.label(
                                                                egui::RichText::new(&asset.name)
                                                                    .size(9.0)
                                                                    .color(ui.style().visuals.weak_text_color().gamma_multiply(0.7))
                                                            );
                                                        }

                                                        response
                                                    }).inner;

                                                    if label_response.clicked() {
                                                        self.selected_asset_idx = Some(idx);
                                                    }
                                                } else if let Some(mapping) = mapping {
                                                    // Show friendly display name with device list from config
                                                    let label_response = ui.vertical(|ui| {
                                                        let response = ui.selectable_label(is_selected, mapping.display_name);

                                                        ui.add_space(2.0);

                                                        // Device list in smaller, dimmer text
                                                        ui.label(
                                                            egui::RichText::new(mapping.devices)
                                                                .small()
                                                                .color(ui.style().visuals.weak_text_color())
                                                        );

                                                        ui.add_space(2.0);

                                                        // Filename in tiny, very dim text
                                                        ui.label(
                                                            egui::RichText::new(&asset.name)
                                                                .size(9.0)
                                                                .color(ui.style().visuals.weak_text_color().gamma_multiply(0.7))
                                                        );

                                                        response
                                                    }).inner;

                                                    if label_response.clicked() {
                                                        self.selected_asset_idx = Some(idx);
                                                    }
                                                } else {
                                                    // Fallback: show asset name only
                                                    if ui.selectable_label(is_selected, &asset.name).clicked() {
                                                        self.selected_asset_idx = Some(idx);
                                                    }
                                                }
                                            });
                                        }
                                    });

                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(8.0);

                                let can_continue = self.selected_asset_idx.is_some();
                                match centered_buttons(ui, &[(&t!("common.cancel"), true), (&t!("common.continue"), can_continue)]) {
                                    Some(0) => {
                                        self.state = AppState::Idle;
                                        self.fetched_release = None;
                                        self.available_assets.clear();
                                        self.selected_asset_idx = None;
                                        self.manifest_display_name = None;
                                    }
                                    Some(_) => {
                                        // Check if selected asset is a raw image
                                        let is_raw_image = self.selected_asset_idx.is_some_and(|idx| {
                                            Self::is_raw_image_asset(&self.available_assets[idx].name)
                                        });

                                        // If update mode and NOT a raw image, show preview; otherwise go to confirmation
                                        if self.update_mode && !is_raw_image {
                                            self.state = AppState::PreviewingUpdate;
                                        } else {
                                            // Skip preview for image mode (update mode doesn't apply to raw images)
                                            self.state = AppState::AwaitingConfirmation;
                                        }
                                    }
                                    None => {}
                                }
                            }
                            AppState::PreviewingUpdate => {
                                ui.add_space(12.0);
                                ui.heading(t!("update_preview.heading"));
                                ui.add_space(12.0);

                                ui.label(t!("update_preview.deleted_dirs"));
                                ui.add_space(8.0);

                                // Show directories to be deleted
                                let update_dirs = REPO_OPTIONS[self.selected_repo_idx].update_directories;
                                egui::ScrollArea::vertical()
                                    .max_height(150.0)
                                    .show(ui, |ui| {
                                        for dir in update_dirs {
                                            ui.label(format!("  \u{2022} {}/", dir));
                                        }
                                    });

                                ui.add_space(12.0);

                                if self.preserve_data {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(104, 157, 106),
                                        t!("update_preview.preserved")
                                    );
                                    ui.add_space(4.0);
                                    for item in [
                                        "update_preview.preserved_retroarch",
                                        "update_preview.preserved_emulators",
                                        "update_preview.preserved_syncthing",
                                        "update_preview.preserved_spruce",
                                    ] {
                                        ui.label(format!("  \u{2022} {}", t!(item)));
                                    }
                                } else {
                                    ui.colored_label(
                                        ui.visuals().warn_fg_color,
                                        format!("\u{26A0} {}", t!("update_preview.preserve_disabled"))
                                    );
                                    ui.add_space(4.0);
                                    // One string with its own line break, so a translation can
                                    // reorder the sentence and choose where it wraps
                                    ui.label(t!("update_preview.customizations_lost"));
                                    ui.colored_label(
                                        egui::Color32::from_rgb(104, 157, 106),
                                        t!("update_preview.still_kept")
                                    );
                                }
                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(8.0);

                                match centered_buttons(ui, &[(&t!("common.cancel"), true), (&t!("common.continue"), true)]) {
                                    Some(0) => {
                                        self.state = AppState::Idle;
                                        self.update_mode = false;
                                        self.fetched_release = None;
                                        self.available_assets.clear();
                                        self.selected_asset_idx = None;
                                        self.manifest_display_name = None;
                                    }
                                    Some(_) => self.state = AppState::AwaitingConfirmation,
                                    None => {}
                                }
                            }
                            AppState::AwaitingConfirmation => {
                                ui.add_space(12.0);
                                ui.colored_label(ui.visuals().warn_fg_color, t!("confirm.warning"));
                                ui.add_space(12.0);

                                if self.update_mode {
                                    ui.label(t!("confirm.update_deletes_dirs"));
                                    ui.add_space(8.0);
                                    ui.label(t!("confirm.update_question"));
                                } else {
                                    ui.label(t!("confirm.erase_warning"));
                                    ui.add_space(8.0);

                                    if let Some(idx) = self.selected_drive_idx {
                                        if let Some(drive) = self.drives.get(idx) {
                                            ui.label(drive.display_name());
                                        }
                                    }

                                    ui.add_space(12.0);
                                    ui.label(t!("confirm.erase_question"));
                                }

                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(8.0);

                                let confirm_text = if self.update_mode {
                                    t!("confirm.yes_update")
                                } else {
                                    t!("confirm.yes_install")
                                };
                                match centered_buttons(ui, &[(&t!("common.cancel"), true), (&confirm_text, true)]) {
                                    Some(0) => {
                                        self.state = AppState::Idle;
                                        self.update_mode = false;
                                    }
                                    Some(_) => self.start_installation(ctx.clone()),
                                    None => {}
                                }
                            }
                            AppState::Complete => {
                                ui.add_space(12.0);
                                ui.colored_label(egui::Color32::from_rgb(104, 157, 106), t!("complete.title"));
                                ui.add_space(12.0);
                                let repo = &REPO_OPTIONS[self.selected_repo_idx];
                                // Fallback chain: manifest display_name → config.rs display_name → repo name
                                let display_name = self.manifest_display_name.as_deref()
                                    .or(repo.display_name)
                                    .unwrap_or(repo.name);
                                ui.label(t!("complete.message", name = display_name));
                                ui.add_space(15.0);
                                ui.separator();
                                ui.add_space(8.0);

                                match centered_buttons(ui, &[(&t!("common.close"), true), (&t!("complete.eject"), true)]) {
                                    Some(0) => {
                                        self.state = AppState::Idle;
                                        self.manifest_display_name = None;
                                    }
                                    Some(_) => {
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
                                    None => {}
                                }
                            }
                            AppState::Ejecting => {
                                ui.add_space(12.0);
                                ui.add(egui::Spinner::new().color(ui.visuals().selection.bg_fill));
                                ui.add_space(8.0);
                                ui.label(t!("eject.ejecting"));
                                ui.add_space(12.0);
                            }
                            AppState::Ejected => {
                                ui.add_space(12.0);
                                ui.label(t!("eject.ejected"));
                                ui.add_space(8.0);
                                ui.colored_label(egui::Color32::from_rgb(104, 157, 106), t!("eject.safe_to_remove"));
                                ui.add_space(15.0);
                                if ui.button(t!("common.ok")).clicked() {
                                    self.state = AppState::Idle;
                                    self.manifest_display_name = None;
                                }
                            }
                            AppState::Error => {
                                ui.add_space(12.0);
                                ui.colored_label(ui.visuals().error_fg_color, t!("error.title"));
                                ui.add_space(12.0);
                                let repo = &REPO_OPTIONS[self.selected_repo_idx];
                                // Fallback chain: manifest display_name → config.rs display_name → repo name
                                let display_name = self.manifest_display_name.as_deref()
                                    .or(repo.display_name)
                                    .unwrap_or(repo.name);
                                ui.label(t!("error.message", name = display_name));
                                ui.add_space(8.0);
                                ui.label(t!("error.check_log"));
                                ui.add_space(15.0);
                                if ui.button(t!("common.ok")).clicked() {
                                    self.state = AppState::Idle;
                                    self.manifest_display_name = None;
                                }
                            }
                            _ => {}
                        }
                        ui.add_space(8.0);
                    });
                });
        }

        // Scraper Window
        if self.show_scraper_window {
            let window_frame = egui::Frame::window(&ctx.style())
                .fill(ctx.style().visuals.window_fill)
                .stroke(ctx.style().visuals.window_stroke);

            egui::Window::new("Scrape Boxart")
                .order(egui::Order::Foreground)
                .collapsible(false)
                .resizable(false)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(window_frame)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(12.0);
                        ui.heading(t!("scraper.title"));
                        ui.add_space(12.0);

                        // Show selected drive
                        if let Some(idx) = self.selected_drive_idx {
                            if let Some(drive) = self.drives.get(idx) {
                                ui.label(t!("scraper.selected_drive", drive = drive.display_name()));
                                ui.add_space(8.0);
                            }
                        }

                        // Roms folder label
                        ui.label(t!("scraper.roms_path", path = self.scraper_roms_path));
                        ui.add_space(8.0);

                        // Select All / Deselect All buttons
                        if !self.scraper_folders.is_empty() && !matches!(self.state, AppState::ScrapingBoxart) && self.scraper_stats.is_none() {
                            let select_all = t!("scraper.select_all");
                            let deselect_all = t!("scraper.deselect_all");
                            if let Some(clicked) = centered_buttons(ui, &[(&select_all, true), (&deselect_all, true)]) {
                                let selected = clicked == 0;
                                for folder in &mut self.scraper_folders {
                                    folder.1 = selected;
                                }
                            }
                            ui.add_space(4.0);

                            // Scrollable checkbox list
                            egui::ScrollArea::vertical()
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    for folder in &mut self.scraper_folders {
                                        ui.checkbox(&mut folder.1, &folder.0);
                                    }
                                });
                        } else if self.scraper_folders.is_empty() && !matches!(self.state, AppState::ScrapingBoxart) && self.scraper_stats.is_none() {
                            ui.label(t!("scraper.no_folders"));
                        }
                        ui.add_space(12.0);

                        // Show progress section during scraping
                        if matches!(self.state, AppState::ScrapingBoxart) {
                            ui.separator();
                            ui.add_space(8.0);

                            let (current, total, message) = {
                                let p = self.progress.lock().unwrap();
                                (p.current, p.total, p.message.clone())
                            };

                            ui.label(&message);
                            ui.add_space(8.0);

                            let progress = if total > 0 {
                                current as f32 / total as f32
                            } else {
                                0.0
                            };

                            ui.add(
                                egui::ProgressBar::new(progress)
                                    .fill(ui.visuals().selection.bg_fill)
                                    .desired_height(16.0)
                                    .desired_width(300.0)
                            );
                            ui.add_space(12.0);
                        }

                        // Show results section when complete
                        if let Some(ref stats) = self.scraper_stats {
                            ui.separator();
                            ui.add_space(8.0);
                            ui.colored_label(egui::Color32::from_rgb(104, 157, 106), t!("scraper.complete"));
                            ui.add_space(8.0);
                            ui.label(t!("scraper.total", count = stats.total));
                            ui.label(t!("scraper.downloaded", count = stats.succeeded));
                            ui.label(t!("scraper.skipped", count = stats.skipped));
                            ui.label(t!("scraper.failed", count = stats.failed));
                            ui.add_space(12.0);
                        }

                        ui.separator();
                        ui.add_space(8.0);

                        // Buttons
                        let is_scraping = matches!(self.state, AppState::ScrapingBoxart);
                        let has_selection = self.scraper_folders.iter().any(|(_, selected)| *selected);
                        let path_valid = !self.scraper_roms_path.is_empty() &&
                                        std::path::Path::new(&self.scraper_roms_path).exists() &&
                                        has_selection;

                        let cancel = t!("common.cancel");
                        if is_scraping {
                            // Cancel during scraping
                            if centered_buttons(ui, &[(&cancel, true)]).is_some() {
                                self.cancel_installation();
                            }
                        } else if self.scraper_stats.is_some() {
                            // Close when complete
                            if centered_buttons(ui, &[(&t!("common.close"), true)]).is_some() {
                                self.show_scraper_window = false;
                                self.scraper_stats = None;
                                self.state = AppState::Idle;
                            }
                        } else {
                            // Cancel or start before scraping
                            match centered_buttons(ui, &[(&cancel, true), (&t!("scraper.start"), path_valid)]) {
                                Some(0) => self.show_scraper_window = false,
                                Some(_) => self.start_boxart_scraping(ctx.clone()),
                                None => {}
                            }
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
                                ui.heading(t!("log.title"));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("X").on_hover_text(t!("log.close")).clicked() {
                                        self.show_log = false;
                                        let current_size = ui.ctx().content_rect().size();
                                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(current_size.x - 320.0, current_size.y)));
                                    }
                                });
                            });

                            // Copy to Clipboard button
                            ui.horizontal(|ui| {
                                if ui.button(format!("📋 {}", t!("log.copy"))).clicked() {
                                    let log_path = crate::debug::get_log_path();
                                    match std::fs::read_to_string(&log_path) {
                                        Ok(contents) => {
                                            match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(contents)) {
                                                Ok(_) => {
                                                    self.log("Log copied to clipboard");
                                                },
                                                Err(e) => {
                                                    self.log(&format!("Failed to copy to clipboard: {}", e));
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            self.log(&format!("Failed to read log file: {}", e));
                                        }
                                    }
                                }
                                ui.label(t!("log.file", file = format!("{:?}", crate::debug::get_log_path().file_name().unwrap_or_default())));
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
                                            ui.label(t!("log.read_failed", error = e));
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
                        AppState::FetchingAssets
                            | AppState::FetchingRelease
                            | AppState::Downloading
                            | AppState::Formatting
                            | AppState::BackingUp
                            | AppState::Deleting
                            | AppState::Extracting
                            | AppState::Copying
                            | AppState::Restoring
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
                                egui::include_image!("../../assets/Icons/icon_dark.png")
                            } else {
                                egui::include_image!("../../assets/Icons/icon.png")
                            };
                            ui.add(egui::Image::new(image).fit_to_exact_size(egui::vec2(60.0, 60.0)));
                        },
                    );

                    columns[2].allocate_ui_with_layout(
                        egui::Vec2::ZERO,
                        egui::Layout::right_to_left(egui::Align::TOP),
                        |ui| {
                            // Theme editor button (hidden - use Ctrl+T shortcut instead)
                            // if ui.button("🎨").on_hover_text("Toggle Theme Editor (Ctrl+T)").clicked() {
                            //     self.show_theme_editor = !self.show_theme_editor;
                            // }
                            if ui.button(egui::RichText::new("📜").size(24.0)).on_hover_text(t!("main.toggle_log")).clicked() {
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

                            // Scrape Boxart button - only enabled when drive is selected and state is Idle or Complete
                            let can_scrape = self.selected_drive_idx.is_some() &&
                                           matches!(self.state, AppState::Idle | AppState::Complete);
                            ui.add_enabled_ui(can_scrape, |ui| {
                                if ui.button(egui::RichText::new("🖼").size(24.0)).on_hover_text(t!("scraper.title")).clicked() {
                                    // Set default Roms path based on selected drive
                                    if let Some(idx) = self.selected_drive_idx {
                                        if let Some(drive) = self.drives.get(idx) {
                                            if let Some(mount_path) = &drive.mount_path {
                                                #[cfg(target_os = "windows")]
                                                let roms_path = format!("{}Roms", mount_path.display());
                                                #[cfg(not(target_os = "windows"))]
                                                let roms_path = mount_path.join("Roms").display().to_string();
                                                self.scraper_roms_path = roms_path.clone();

                                                // Scan for valid subfolders
                                                let mut folders = Vec::new();
                                                if let Ok(entries) = std::fs::read_dir(&roms_path) {
                                                    for entry in entries.flatten() {
                                                        let path = entry.path();
                                                        if path.is_dir() {
                                                            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                                                if crate::boxart_scraper::BoxArtScraper::get_ra_alias(name).is_some() {
                                                                    folders.push((name.to_string(), true));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                folders.sort_by(|a, b| a.0.cmp(&b.0));
                                                self.scraper_folders = folders;
                                            }
                                        }
                                    }
                                    self.show_scraper_window = true;
                                    self.scraper_stats = None;
                                }
                            });
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
                                (t!("main.no_sd_card").into_owned(), false)
                            } else {
                                (
                                    self.selected_drive_idx
                                        .and_then(|idx| self.drives.get(idx))
                                        .map(|d| d.display_name())
                                        .unwrap_or_else(|| t!("main.select_drive").into_owned()),
                                    !show_progress
                                )
                            };

                            ui.add_enabled_ui(enabled, |ui| {
                                ui.set_max_width(DRIVE_PICKER_MAX_WIDTH);
                                egui::ComboBox::from_id_salt("drive_select")
                                    .selected_text(&selected_text)
                                    .truncate()
                                    .show_ui(ui, |ui| {
                                        for (idx, drive) in self.drives.iter().enumerate() {
                                            let text = if drive.is_card_sized() {
                                                drive.display_name()
                                            } else {
                                                format!("\u{26A0} {}", drive.display_name())
                                            };
                                            ui.selectable_value(&mut self.selected_drive_idx, Some(idx), text);
                                        }
                                    });
                            });

                            // Refresh button
                            ui.add_space(8.0);
                            if ui.button("🔄").on_hover_text(t!("main.refresh_drives")).clicked() {
                                let _ = self.manual_refresh_tx.send(());
                            }
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

                                for (idx, repo) in REPO_OPTIONS.iter().enumerate() {
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
                                            repo.name,
                                        ).frame_when_inactive(true)).clicked() {
                                            self.selected_repo_idx = idx;
                                        }
                                    });
                                }
                            });
                        },
                    );
                });

                ui.add_space(8.0);

                // ========================================================================
                // HIDE UPDATE MODE: Comment out this entire block to disable the feature
                // ========================================================================
                // This checkbox allows users to enable update mode (preserve ROMs/saves).
                // The checkbox only appears when the selected repository supports update mode
                // (configured via the supports_update_mode field in REPO_OPTIONS).
                // To completely hide this feature, comment out this entire if statement
                // block through the matching closing brace below.
                // ========================================================================
                // Update mode checkbox (only show when not in progress AND repo supports it)
                let current_repo_supports_update = REPO_OPTIONS[self.selected_repo_idx].supports_update_mode;
                if !show_progress && current_repo_supports_update {
                    ui.horizontal(|ui| {
                        ui.vertical_centered(|ui| {
                            // "Update existing installation (skip format)"
                            if ui.checkbox(&mut self.update_mode, t!("main.update_mode")).changed() {
                                // Reset state when toggling update mode
                                if !self.update_mode {
                                    self.fetched_release = None;
                                    self.available_assets.clear();
                                    self.selected_asset_idx = None;
                                }
                            }
                        });
                    });
                } else if !current_repo_supports_update && self.update_mode {
                    // If we switch to a repo that doesn't support update mode, disable it
                    self.update_mode = false;
                    self.fetched_release = None;
                    self.available_assets.clear();
                    self.selected_asset_idx = None;
                }
                // ========================================================================
                // END HIDE UPDATE MODE - Comment through here to disable the checkbox
                // ========================================================================

                // Preserve user data checkbox (only show when update mode is ON and repo supports it)
                let current_repo_supports_preserve = REPO_OPTIONS[self.selected_repo_idx].supports_preserve_mode;
                if !show_progress && self.update_mode && current_repo_supports_preserve {
                    ui.horizontal(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.checkbox(&mut self.preserve_data, t!("main.preserve_data"));
                        });
                    });
                } else if !current_repo_supports_preserve || !self.update_mode {
                    // Reset preserve_data to default when conditions aren't met
                    self.preserve_data = true;
                }

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

                            // FetchingAssets, FetchingRelease, and Deleting have indeterminate progress (spinner)
                            // Downloading, Formatting, Extracting, and Copying report percentages
                            let is_indeterminate = matches!(
                                self.state,
                                AppState::FetchingAssets | AppState::FetchingRelease | AppState::BackingUp | AppState::Deleting | AppState::Restoring | AppState::Idle
                            );

                            if is_indeterminate {
                                // Animated indeterminate progress bar
                                let time = ctx.input(|i| i.time);

                                // Allocate space for the progress bar - match normal progress bar dimensions
                                let desired_size = egui::vec2(ui.available_width() / 2.0, 16.0);
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
                        // Install/Resume button
                        let is_busy = matches!(
                            self.state,
                            AppState::FetchingAssets
                                | AppState::SelectingAsset
                                | AppState::PreviewingUpdate
                                | AppState::FetchingRelease
                                | AppState::Downloading
                                | AppState::Formatting
                                | AppState::BackingUp
                                | AppState::Deleting
                                | AppState::Extracting
                                | AppState::Copying
                                | AppState::Restoring
                                | AppState::AwaitingConfirmation
                                | AppState::Ejecting
                                | AppState::Cancelling
                        );

                        if !is_busy {
                            ui.add_enabled_ui(!is_busy && self.selected_drive_idx.is_some() && !self.drives.is_empty(), |ui| {
                                // Use in-memory flag to show Resume (works for both Windows and Linux)
                                let button_text = if self.was_just_paused {
                                    t!("main.resume")
                                } else {
                                    t!("main.install")
                                };

                                let button = egui::Button::new(button_text)
                                    .min_size(egui::vec2(96.0, 48.0))
                                    .fill(egui::Color32::from_rgb(104, 157, 106)); // Green
                                if ui.add(button).clicked() {
                                    self.fetch_and_check_assets(ctx.clone());
                                }

                                // Show info if partial download exists
                                if self.was_just_paused {
                                    ui.label(
                                        egui::RichText::new(t!("main.partial_download"))
                                            .small()
                                            .color(egui::Color32::from_rgb(214, 93, 14))
                                    );
                                }
                            });
                        }
                    });
                });

                // Pause/Cancel buttons (only show during cancellable operations)
                // These are outside the horizontal layout so they center properly
                let can_cancel = matches!(
                    self.state,
                    AppState::FetchingRelease
                        | AppState::Downloading
                        | AppState::Formatting
                        | AppState::BackingUp
                        | AppState::Deleting
                        | AppState::Extracting
                        | AppState::Copying
                        | AppState::Restoring
                ) && self.cancel_token.is_some();

                if can_cancel {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        // Show Pause button only during downloads
                        if matches!(self.state, AppState::Downloading) && self.pause_token.is_some() {
                            let pause_button = egui::Button::new(t!("main.pause"))
                                .min_size(egui::vec2(96.0, 48.0))
                                .fill(egui::Color32::from_rgb(214, 93, 14)); // Orange
                            if ui.add(pause_button).clicked() {
                                self.pause_download();
                            }
                            ui.add_space(8.0);
                        }

                        // Cancel button
                        let cancel_button = egui::Button::new(t!("common.cancel"))
                            .min_size(egui::vec2(96.0, 48.0))
                            .fill(egui::Color32::from_rgb(251, 73, 52)); // Red
                        if ui.add(cancel_button).clicked() {
                            self.cancel_installation();
                        }
                    });
                }

                // Repository info text (shown below Install button when not busy)
                if !show_progress {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        let repo_info = REPO_OPTIONS[self.selected_repo_idx].info;
                        let text_color = egui::Color32::from_rgba_unmultiplied(251, 241, 199, 255);

                        // Split by \n and display each line
                        for line in repo_info.split('\n') {
                            // Parse markdown-style links: [text](url)
                            if line.contains('[') && line.contains("](") {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_justify(true), |ui| {
                                    let mut remaining = line;
                                    while let Some(start) = remaining.find('[') {
                                        // Render text before the link
                                        if start > 0 {
                                            ui.colored_label(text_color, &remaining[..start]);
                                        }

                                        remaining = &remaining[start + 1..];

                                        if let Some(end) = remaining.find("](") {
                                            let link_text = &remaining[..end];
                                            remaining = &remaining[end + 2..];

                                            if let Some(url_end) = remaining.find(')') {
                                                let url = &remaining[..url_end];
                                                ui.hyperlink_to(link_text, url);
                                                remaining = &remaining[url_end + 1..];
                                            } else {
                                                // Malformed link, render as-is
                                                ui.colored_label(text_color, &format!("[{}", link_text));
                                                break;
                                            }
                                        } else {
                                            // No matching ](, render as-is
                                            ui.colored_label(text_color, &format!("[{}", remaining));
                                            break;
                                        }
                                    }
                                    // Render any remaining text
                                    if !remaining.is_empty() {
                                        ui.colored_label(text_color, remaining);
                                    }
                                });
                            } else {
                                // No links in this line
                                ui.colored_label(text_color, line);
                            }
                        }
                    });
                }

                ui.add_space(10.0);

                // Version, and a quiet nudge when a newer one exists. No
                // auto-update: the binaries are unsigned, so a self-replacing
                // download would trip Gatekeeper and SmartScreen.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        if let Some(tag) = self.newer_version.clone() {
                            ui.hyperlink_to(
                                t!("main.newer_version", version = tag),
                                crate::config::UPDATE_DOWNLOAD_URL,
                            );
                            ui.label("-");
                        }
                        ui.weak(format!("v{}", crate::config::APP_VERSION));
                    });
                });
            });
        });

        self.show_update_notice(ctx);
    }
}

impl InstallerApp {
    /// One-off notice on startup when a newer installer exists. Dismissed for
    /// the rest of the run; the footer link stays either way.
    fn show_update_notice(&mut self, ctx: &egui::Context) {
        let Some(tag) = self.newer_version.clone() else {
            return;
        };
        if self.update_notice_dismissed {
            return;
        }

        egui::Window::new(t!("update_notice.title"))
            .id(egui::Id::new("update_notice"))
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Cap the width from inside: in an auto-sizing window the
                // available space is the whole parent, so anything that fills
                // its width stretches the window instead of fitting content.
                ui.set_max_width(320.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(t!(
                        "update_notice.message",
                        latest = tag,
                        current = crate::config::APP_VERSION
                    ));
                    ui.add_space(6.0);
                    ui.label(t!("update_notice.get_latest"));
                    // The URL is spelled out, not hidden behind link text:
                    // opening a browser needs xdg-open wired up, which plenty
                    // of Linux setups do not have, and then a bare "click
                    // here" leaves the user with nothing to copy.
                    ui.hyperlink(crate::config::UPDATE_DOWNLOAD_URL);
                    ui.add_space(8.0);
                    let copy_label = format!("📋 {}", t!("update_notice.copy_link"));
                    match centered_buttons(ui, &[(&copy_label, true), (&t!("common.continue"), true)]) {
                        Some(0) => {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(crate::config::UPDATE_DOWNLOAD_URL);
                            }
                        }
                        Some(_) => self.update_notice_dismissed = true,
                        None => {}
                    }
                    ui.add_space(4.0);
                });
            });
    }
}

/// A row of buttons centred in the available width, as `(label, enabled)`.
/// Returns the index of the button clicked this frame.
///
/// egui lays a row out from the cursor, and main_align only aligns content
/// *inside* a widget (text in a button), so a row of widgets is centred by
/// measuring it and padding. A Button is its text galley plus button_padding
/// each side. Measuring the actual labels keeps the row centred whatever their
/// length, which a translation changes; two columns that each hug the middle
/// only centre the pair when both labels happen to be the same width.
fn centered_buttons(ui: &mut egui::Ui, buttons: &[(&str, bool)]) -> Option<usize> {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let padding = ui.spacing().button_padding.x * 2.0;
    let gaps = buttons.len().saturating_sub(1) as f32 * ui.spacing().item_spacing.x;
    let row_width = gaps
        + buttons
            .iter()
            .map(|(label, _)| {
                ui.painter()
                    .layout_no_wrap(label.to_string(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
                    + padding
            })
            .sum::<f32>();

    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - row_width) / 2.0).max(0.0));
        for (idx, (label, enabled)) in buttons.iter().enumerate() {
            if ui.add_enabled(*enabled, egui::Button::new(*label)).clicked() {
                clicked = Some(idx);
            }
        }
    });
    clicked
}
