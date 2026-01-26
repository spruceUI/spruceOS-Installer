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
//      and comment out the entire checkbox block
// ============================================================================

use super::{InstallerApp, AppState};
use crate::config::REPO_OPTIONS;
use crate::eject::eject_drive;
use eframe::egui;
use egui_thematic::render_theme_panel;

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

        // Check for release fetch results
        if let Some(rx) = &mut self.release_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(release) => {
                        // Get allowed extensions from current repo option
                        let repo_option = &REPO_OPTIONS[self.selected_repo_idx];
                        let allowed_extensions = repo_option.allowed_extensions;

                        // Filter assets
                        let mut assets = Self::filter_assets(release.assets.clone(), allowed_extensions);

                        if assets.is_empty() {
                            let msg = if allowed_extensions.is_some() {
                                "No compatible files found matching the allowed extensions for this repository"
                            } else {
                                "No compatible files found in release"
                            };
                            self.log(msg);
                            self.state = AppState::Error;
                            self.release_rx = None;
                        } else {
                            // Sort assets alphabetically
                            assets.sort_by(|a, b| a.name.cmp(&b.name));

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
                                let is_raw_image = if let Some(idx) = auto_idx {
                                    let asset_name = &self.available_assets[idx].name;
                                    asset_name.ends_with(".img.gz") ||
                                    asset_name.ends_with(".img.xz") ||
                                    asset_name.ends_with(".img")
                                } else {
                                    false
                                };

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

                            self.release_rx = None;
                        }
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
                progress.message.clear();
            } else if progress.message == "ERROR" {
                self.state = AppState::Error;
                self.cancel_token = None;
                self.update_mode = false; // Reset update mode
                progress.message.clear();
            } else if progress.message == "CANCELLED" {
                self.state = AppState::Idle;
                self.cancel_token = None;
                self.update_mode = false; // Reset update mode
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
                } else if progress.message.contains("Deleting") || progress.message.contains("deletion")
                {
                    self.state = AppState::Deleting;
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
            AppState::FetchingAssets
                | AppState::FetchingRelease
                | AppState::Downloading
                | AppState::Formatting
                | AppState::Deleting
                | AppState::Extracting
                | AppState::Copying
                | AppState::Ejecting
                | AppState::Cancelling
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
                    ui.vertical_centered(|ui| {
                        match self.state {
                            AppState::SelectingAsset => {
                                ui.add_space(12.0);
                                ui.heading("Select a file to install:");
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

                                            // Try to find a display mapping for this asset
                                            let mapping = display_mappings.and_then(|mappings| {
                                                mappings.iter().find(|m| asset.name.contains(m.pattern))
                                            });

                                            ui.group(|ui| {
                                                ui.set_min_width(400.0);

                                                if let Some(mapping) = mapping {
                                                    // Show friendly display name with device list
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

                                ui.columns(2, |columns| {
                                    columns[0].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Cancel").clicked() {
                                                self.state = AppState::Idle;
                                                self.fetched_release = None;
                                                self.available_assets.clear();
                                                self.selected_asset_idx = None;
                                            }
                                        },
                                    );

                                    columns[1].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let can_continue = self.selected_asset_idx.is_some();
                                            ui.add_enabled_ui(can_continue, |ui| {
                                                if ui.button("Continue").clicked() {
                                                    // Check if selected asset is a raw image
                                                    let is_raw_image = if let Some(idx) = self.selected_asset_idx {
                                                        let asset_name = &self.available_assets[idx].name;
                                                        asset_name.ends_with(".img.gz") ||
                                                        asset_name.ends_with(".img.xz") ||
                                                        asset_name.ends_with(".img")
                                                    } else {
                                                        false
                                                    };

                                                    // If update mode and NOT a raw image, show preview; otherwise go to confirmation
                                                    if self.update_mode && !is_raw_image {
                                                        self.state = AppState::PreviewingUpdate;
                                                    } else {
                                                        // Skip preview for image mode (update mode doesn't apply to raw images)
                                                        self.state = AppState::AwaitingConfirmation;
                                                    }
                                                }
                                            });
                                        },
                                    );
                                });
                            }
                            AppState::PreviewingUpdate => {
                                ui.add_space(12.0);
                                ui.heading("Update Preview");
                                ui.add_space(12.0);

                                ui.label("The following directories will be deleted:");
                                ui.add_space(8.0);

                                // Show directories to be deleted
                                let update_dirs = REPO_OPTIONS[self.selected_repo_idx].update_directories;
                                egui::ScrollArea::vertical()
                                    .max_height(200.0)
                                    .show(ui, |ui| {
                                        for dir in update_dirs {
                                            ui.label(format!("• {}/", dir));
                                        }
                                    });

                                ui.add_space(12.0);
                                ui.colored_label(
                                    egui::Color32::from_rgb(104, 157, 106),
                                    "All other files will be preserved."
                                );
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
                                                self.update_mode = false;
                                                self.fetched_release = None;
                                                self.available_assets.clear();
                                                self.selected_asset_idx = None;
                                            }
                                        },
                                    );

                                    columns[1].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Continue").clicked() {
                                                self.state = AppState::AwaitingConfirmation;
                                            }
                                        },
                                    );
                                });
                            }
                            AppState::AwaitingConfirmation => {
                                ui.add_space(12.0);
                                ui.colored_label(ui.visuals().warn_fg_color, "WARNING");
                                ui.add_space(12.0);

                                if self.update_mode {
                                    ui.label("The selected directories will be deleted.");
                                    ui.add_space(8.0);
                                    ui.label("Continue with the update?");
                                } else {
                                    ui.label("This will DELETE ALL DATA on the selected drive:");
                                    ui.add_space(8.0);

                                    if let Some(idx) = self.selected_drive_idx {
                                        if let Some(drive) = self.drives.get(idx) {
                                            ui.label(drive.display_name());
                                        }
                                    }

                                    ui.add_space(12.0);
                                    ui.label("Are you sure you want to continue?");
                                }

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
                                                self.update_mode = false;
                                            }
                                        },
                                    );

                                    columns[1].allocate_ui_with_layout(
                                        egui::Vec2::ZERO,
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let button_text = if self.update_mode {
                                                "Yes, update"
                                            } else {
                                                "Yes, install"
                                            };
                                            if ui.button(button_text).clicked() {
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
                                let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].name;
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
                                let selected_repo_name = REPO_OPTIONS[self.selected_repo_idx].name;
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
                        AppState::FetchingAssets
                            | AppState::FetchingRelease
                            | AppState::Downloading
                            | AppState::Formatting
                            | AppState::Deleting
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

                            // Refresh button
                            ui.add_space(8.0);
                            if ui.button("🔄").on_hover_text("Refresh drive list").clicked() {
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
                            if ui.checkbox(&mut self.update_mode, "Update existing installation (skip format)").changed() {
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
                                AppState::FetchingAssets | AppState::FetchingRelease | AppState::Deleting | AppState::Idle
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
                        // Install button
                        let is_busy = matches!(
                            self.state,
                            AppState::FetchingAssets
                                | AppState::SelectingAsset
                                | AppState::PreviewingUpdate
                                | AppState::FetchingRelease
                                | AppState::Downloading
                                | AppState::Formatting
                                | AppState::Deleting
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
                                    self.fetch_and_check_assets(ctx.clone());
                                }
                            });
                        }

                        // Cancel button (only show during cancellable operations)
                        let can_cancel = matches!(
                            self.state,
                            AppState::FetchingRelease
                                | AppState::Downloading
                                | AppState::Formatting
                                | AppState::Deleting
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

                // Repository info text (shown below Install button when not busy)
                if !show_progress {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        let repo_info = REPO_OPTIONS[self.selected_repo_idx].info;
                        let text_color = egui::Color32::from_rgba_unmultiplied(251, 241, 199, 255);

                        // Split by \n and display each line
                        for line in repo_info.split('\n') {
                            ui.colored_label(text_color, line);
                        }
                    });
                }

                ui.add_space(10.0);
            });
        });
    }
}
