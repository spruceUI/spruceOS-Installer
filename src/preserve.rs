// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

use serde_json::Value;
use std::path::Path;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum PreserveProgress {
    Started { total_paths: usize },
    BackingUp { path: String },
    Restoring { path: String },
    Completed,
    Cancelled,
    #[allow(dead_code)]
    Error(String),
}

/// Recursively copy a file or directory from src to dst, preserving relative structure.
async fn copy_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        tokio::fs::create_dir_all(dst).await
            .map_err(|e| format!("Failed to create directory {:?}: {}", dst, e))?;

        let mut entries = tokio::fs::read_dir(src).await
            .map_err(|e| format!("Failed to read directory {:?}: {}", src, e))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| format!("Failed to read entry in {:?}: {}", src, e))? {
            let entry_src = entry.path();
            let entry_dst = dst.join(entry.file_name());
            Box::pin(copy_recursive(&entry_src, &entry_dst)).await?;
        }
    } else if src.is_file() {
        // Ensure parent directory exists
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| format!("Failed to create parent dir {:?}: {}", parent, e))?;
        }
        tokio::fs::copy(src, dst).await
            .map_err(|e| format!("Failed to copy {:?} to {:?}: {}", src, dst, e))?;
    }
    // Skip symlinks and other special files
    Ok(())
}

/// Backup preserve paths from the SD card to a local temp directory.
/// Each path in `preserve_paths` is relative to `mount_path`.
/// Files are copied to `temp_backup_dir` preserving their relative path structure.
pub async fn backup_preserve_paths(
    mount_path: &Path,
    preserve_paths: &[&str],
    temp_backup_dir: &Path,
    progress_tx: mpsc::UnboundedSender<PreserveProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(PreserveProgress::Cancelled);
        return Err("Backup cancelled".to_string());
    }

    let _ = progress_tx.send(PreserveProgress::Started {
        total_paths: preserve_paths.len(),
    });

    crate::debug::log_section("Backing Up User Data");
    crate::debug::log(&format!("Mount path: {:?}", mount_path));
    crate::debug::log(&format!("Backup dir: {:?}", temp_backup_dir));

    // Create the backup directory
    tokio::fs::create_dir_all(temp_backup_dir).await
        .map_err(|e| format!("Failed to create backup directory: {}", e))?;

    for rel_path in preserve_paths {
        if cancel_token.is_cancelled() {
            let _ = progress_tx.send(PreserveProgress::Cancelled);
            return Err("Backup cancelled".to_string());
        }

        let src = mount_path.join(rel_path);
        let dst = temp_backup_dir.join(rel_path);

        if !src.exists() {
            crate::debug::log(&format!("Path does not exist, skipping: {}", rel_path));
            continue;
        }

        let _ = progress_tx.send(PreserveProgress::BackingUp {
            path: rel_path.to_string(),
        });

        crate::debug::log(&format!("Backing up: {}", rel_path));

        copy_recursive(&src, &dst).await?;
    }

    let _ = progress_tx.send(PreserveProgress::Completed);
    crate::debug::log("User data backup complete");

    Ok(())
}

/// Restore preserved paths from the local temp backup directory back to the SD card.
/// Walks the backup directory recursively and copies all files back to `mount_path`
/// at their original relative paths, overwriting any existing files.
/// Cleans up `temp_backup_dir` when done.
pub async fn restore_preserve_paths(
    mount_path: &Path,
    temp_backup_dir: &Path,
    progress_tx: mpsc::UnboundedSender<PreserveProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(PreserveProgress::Cancelled);
        return Err("Restore cancelled".to_string());
    }

    // Count entries for progress reporting
    let paths = collect_relative_files(temp_backup_dir, temp_backup_dir).await?;

    let _ = progress_tx.send(PreserveProgress::Started {
        total_paths: paths.len(),
    });

    crate::debug::log_section("Restoring User Data");
    crate::debug::log(&format!("Backup dir: {:?}", temp_backup_dir));
    crate::debug::log(&format!("Mount path: {:?}", mount_path));
    crate::debug::log(&format!("Files to restore: {}", paths.len()));

    for rel_path in &paths {
        if cancel_token.is_cancelled() {
            let _ = progress_tx.send(PreserveProgress::Cancelled);
            return Err("Restore cancelled".to_string());
        }

        let src = temp_backup_dir.join(rel_path);
        let dst = mount_path.join(rel_path);

        let _ = progress_tx.send(PreserveProgress::Restoring {
            path: rel_path.clone(),
        });

        crate::debug::log(&format!("Restoring: {}", rel_path));

        // Ensure parent directory exists
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| format!("Failed to create parent dir {:?}: {}", parent, e))?;
        }

        tokio::fs::copy(&src, &dst).await
            .map_err(|e| format!("Failed to restore {:?}: {}", rel_path, e))?;
    }

    // Clean up backup directory
    crate::debug::log("Cleaning up backup directory...");
    let _ = tokio::fs::remove_dir_all(temp_backup_dir).await;

    let _ = progress_tx.send(PreserveProgress::Completed);
    crate::debug::log("User data restore complete");

    Ok(())
}

/// Recursively collect all file paths relative to `base` within `dir`.
async fn collect_relative_files(dir: &Path, base: &Path) -> Result<Vec<String>, String> {
    let mut results = Vec::new();

    if !dir.exists() {
        return Ok(results);
    }

    let mut entries = tokio::fs::read_dir(dir).await
        .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?;

    while let Some(entry) = entries.next_entry().await
        .map_err(|e| format!("Failed to read entry in {:?}: {}", dir, e))? {
        let path = entry.path();

        if path.is_dir() {
            let sub_results = Box::pin(collect_relative_files(&path, base)).await?;
            results.extend(sub_results);
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                results.push(rel.to_string_lossy().to_string());
            }
        }
    }

    Ok(results)
}

/// Smart merge of "selected" values from old config into new config.
/// Rust port of merge_configs.py from the on-device spruceRestore updater.
///
/// Recursively walks JSON objects. When it finds a "selected" key:
/// - If old value is null, skip
/// - If sibling "options" array exists and old value is in it, copy old -> new
/// - If no "options" key exists (free-text field), always copy old -> new
fn merge_json_selected(old: &Value, new: &mut Value) {
    let (Some(old_obj), Some(new_obj)) = (old.as_object(), new.as_object_mut()) else {
        return;
    };

    for (key, old_val) in old_obj {
        if !new_obj.contains_key(key) {
            continue;
        }

        if key == "selected" {
            if old_val.is_null() {
                continue;
            }

            // Check sibling "options" array in the new config to validate old value
            let should_copy = if let Some(options) = new_obj.get("options") {
                if let Some(options_arr) = options.as_array() {
                    options_arr.contains(old_val)
                } else {
                    false // "options" exists but isn't an array
                }
            } else {
                true // No "options" key at all — free-text field, always copy
            };

            if should_copy {
                if let Some(new_val) = new_obj.get_mut(key) {
                    *new_val = old_val.clone();
                }
            }
        } else {
            // Recurse into nested objects
            if let Some(new_val) = new_obj.get_mut(key) {
                merge_json_selected(old_val, new_val);
            }
        }
    }
}

/// Read old and new JSON config files, merge "selected" values from old into new,
/// and write the merged result back to the new config file.
async fn merge_config_file(old_path: &Path, new_path: &Path) -> Result<(), String> {
    let old_data = tokio::fs::read_to_string(old_path).await
        .map_err(|e| format!("Failed to read old config: {}", e))?;
    let new_data = tokio::fs::read_to_string(new_path).await
        .map_err(|e| format!("Failed to read new config: {}", e))?;

    let old_json: Value = serde_json::from_str(&old_data)
        .map_err(|e| format!("Failed to parse old config JSON: {}", e))?;
    let mut new_json: Value = serde_json::from_str(&new_data)
        .map_err(|e| format!("Failed to parse new config JSON: {}", e))?;

    merge_json_selected(&old_json, &mut new_json);

    let merged = serde_json::to_string_pretty(&new_json)
        .map_err(|e| format!("Failed to serialize merged config: {}", e))?;

    tokio::fs::write(new_path, merged).await
        .map_err(|e| format!("Failed to write merged config: {}", e))?;

    Ok(())
}

/// Backup dynamic config files from the SD card to the temp backup directory.
/// Mirrors the on-device spruceBackup.sh behavior:
/// - Scans Emu/*/config.json -> _dynamic_emu_configs/<name>.json
/// - Copies Saves/spruce/spruce-config.json -> _dynamic_spruce_config/spruce-config.json
/// - Scans Themes/*/config*.json -> _dynamic_theme_configs/<theme>/<filename>
pub async fn backup_dynamic_configs(
    mount_path: &Path,
    temp_backup_dir: &Path,
    progress_tx: mpsc::UnboundedSender<PreserveProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(PreserveProgress::Cancelled);
        return Err("Backup cancelled".to_string());
    }

    crate::debug::log_section("Backing Up Dynamic Configs");

    let emu_backup_dir = temp_backup_dir.join("_dynamic_emu_configs");
    let spruce_backup_dir = temp_backup_dir.join("_dynamic_spruce_config");
    let theme_backup_dir = temp_backup_dir.join("_dynamic_theme_configs");

    // 1. Backup emulator configs: Emu/*/config.json
    let emu_dir = mount_path.join("Emu");
    if emu_dir.exists() {
        let mut entries = tokio::fs::read_dir(&emu_dir).await
            .map_err(|e| format!("Failed to read Emu directory: {}", e))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| format!("Failed to read Emu entry: {}", e))? {
            if cancel_token.is_cancelled() {
                let _ = progress_tx.send(PreserveProgress::Cancelled);
                return Err("Backup cancelled".to_string());
            }

            let path = entry.path();
            if path.is_dir() {
                let config_path = path.join("config.json");
                if config_path.exists() {
                    let emu_name = entry.file_name().to_string_lossy().to_string();
                    tokio::fs::create_dir_all(&emu_backup_dir).await
                        .map_err(|e| format!("Failed to create emu backup dir: {}", e))?;

                    let dst = emu_backup_dir.join(format!("{}.json", emu_name));
                    let _ = progress_tx.send(PreserveProgress::BackingUp {
                        path: format!("Emu/{}/config.json", emu_name),
                    });
                    crate::debug::log(&format!("Backing up emu config: {}", emu_name));

                    tokio::fs::copy(&config_path, &dst).await
                        .map_err(|e| format!("Failed to backup emu config {}: {}", emu_name, e))?;
                }
            }
        }
    }

    // 2. Backup spruce config: Saves/spruce/spruce-config.json
    let spruce_config = mount_path.join("Saves").join("spruce").join("spruce-config.json");
    if spruce_config.exists() {
        tokio::fs::create_dir_all(&spruce_backup_dir).await
            .map_err(|e| format!("Failed to create spruce backup dir: {}", e))?;

        let dst = spruce_backup_dir.join("spruce-config.json");
        let _ = progress_tx.send(PreserveProgress::BackingUp {
            path: "Saves/spruce/spruce-config.json".to_string(),
        });
        crate::debug::log("Backing up spruce-config.json");

        tokio::fs::copy(&spruce_config, &dst).await
            .map_err(|e| format!("Failed to backup spruce-config.json: {}", e))?;
    }

    // 3. Backup theme configs: Themes/*/config*.json
    let themes_dir = mount_path.join("Themes");
    if themes_dir.exists() {
        let mut entries = tokio::fs::read_dir(&themes_dir).await
            .map_err(|e| format!("Failed to read Themes directory: {}", e))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| format!("Failed to read Themes entry: {}", e))? {
            if cancel_token.is_cancelled() {
                let _ = progress_tx.send(PreserveProgress::Cancelled);
                return Err("Backup cancelled".to_string());
            }

            let path = entry.path();
            if path.is_dir() {
                let theme_name = entry.file_name().to_string_lossy().to_string();
                let mut theme_entries = tokio::fs::read_dir(&path).await
                    .map_err(|e| format!("Failed to read theme dir {}: {}", theme_name, e))?;

                while let Some(theme_entry) = theme_entries.next_entry().await
                    .map_err(|e| format!("Failed to read theme entry in {}: {}", theme_name, e))? {
                    let fname = theme_entry.file_name().to_string_lossy().to_string();
                    if fname.starts_with("config") && fname.ends_with(".json") {
                        let theme_dst_dir = theme_backup_dir.join(&theme_name);
                        tokio::fs::create_dir_all(&theme_dst_dir).await
                            .map_err(|e| format!("Failed to create theme backup dir: {}", e))?;

                        let dst = theme_dst_dir.join(&fname);
                        let _ = progress_tx.send(PreserveProgress::BackingUp {
                            path: format!("Themes/{}/{}", theme_name, fname),
                        });
                        crate::debug::log(&format!("Backing up theme config: {}/{}", theme_name, fname));

                        tokio::fs::copy(theme_entry.path(), &dst).await
                            .map_err(|e| format!("Failed to backup theme config {}/{}: {}", theme_name, fname, e))?;
                    }
                }
            }
        }
    }

    crate::debug::log("Dynamic config backup complete");
    Ok(())
}

/// Restore dynamic configs with smart merging.
/// Mirrors the on-device spruceRestore.sh behavior:
/// - Emu configs: merge old "selected" values into new configs via merge_json_selected
/// - Spruce config: merge old "selected" values into new config
/// - Theme configs: plain copy (overwrite)
/// Cleans up the _dynamic_* subdirectories when done.
pub async fn restore_and_merge_configs(
    mount_path: &Path,
    temp_backup_dir: &Path,
    progress_tx: mpsc::UnboundedSender<PreserveProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(PreserveProgress::Cancelled);
        return Err("Restore cancelled".to_string());
    }

    crate::debug::log_section("Restoring Dynamic Configs (Smart Merge)");

    let emu_backup_dir = temp_backup_dir.join("_dynamic_emu_configs");
    let spruce_backup_dir = temp_backup_dir.join("_dynamic_spruce_config");
    let theme_backup_dir = temp_backup_dir.join("_dynamic_theme_configs");

    // 1. Merge emulator configs
    if emu_backup_dir.exists() {
        let mut entries = tokio::fs::read_dir(&emu_backup_dir).await
            .map_err(|e| format!("Failed to read emu backup dir: {}", e))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| format!("Failed to read emu backup entry: {}", e))? {
            if cancel_token.is_cancelled() {
                let _ = progress_tx.send(PreserveProgress::Cancelled);
                return Err("Restore cancelled".to_string());
            }

            let fname = entry.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".json") {
                continue;
            }

            let emu_name = fname.strip_suffix(".json").unwrap_or(&fname);
            let new_config_path = mount_path.join("Emu").join(emu_name).join("config.json");

            if !new_config_path.exists() {
                crate::debug::log(&format!("Emu {} no longer exists in new install, skipping merge", emu_name));
                continue;
            }

            let _ = progress_tx.send(PreserveProgress::Restoring {
                path: format!("Emu/{}/config.json (merge)", emu_name),
            });

            match merge_config_file(&entry.path(), &new_config_path).await {
                Ok(()) => {
                    crate::debug::log(&format!("Merged emu config: {}", emu_name));
                }
                Err(e) => {
                    crate::debug::log(&format!("WARNING: Failed to merge emu config {}: {}", emu_name, e));
                }
            }
        }
        let _ = tokio::fs::remove_dir_all(&emu_backup_dir).await;
    }

    // 2. Merge spruce config
    let old_spruce_config = spruce_backup_dir.join("spruce-config.json");
    if old_spruce_config.exists() {
        let new_spruce_config = mount_path.join("Saves").join("spruce").join("spruce-config.json");
        if new_spruce_config.exists() {
            let _ = progress_tx.send(PreserveProgress::Restoring {
                path: "Saves/spruce/spruce-config.json (merge)".to_string(),
            });

            match merge_config_file(&old_spruce_config, &new_spruce_config).await {
                Ok(()) => {
                    crate::debug::log("Merged spruce-config.json");
                }
                Err(e) => {
                    crate::debug::log(&format!("WARNING: Failed to merge spruce-config.json: {}", e));
                }
            }
        }
        let _ = tokio::fs::remove_dir_all(&spruce_backup_dir).await;
    }

    // 3. Copy theme configs (plain overwrite, no merge — matches on-device behavior)
    if theme_backup_dir.exists() {
        let mut themes = tokio::fs::read_dir(&theme_backup_dir).await
            .map_err(|e| format!("Failed to read theme backup dir: {}", e))?;

        while let Some(theme_entry) = themes.next_entry().await
            .map_err(|e| format!("Failed to read theme backup entry: {}", e))? {
            if cancel_token.is_cancelled() {
                let _ = progress_tx.send(PreserveProgress::Cancelled);
                return Err("Restore cancelled".to_string());
            }

            let theme_name = theme_entry.file_name().to_string_lossy().to_string();
            let theme_dest = mount_path.join("Themes").join(&theme_name);

            if !theme_dest.exists() {
                crate::debug::log(&format!("Theme {} no longer exists in new install, skipping", theme_name));
                continue;
            }

            if let Ok(mut configs) = tokio::fs::read_dir(theme_entry.path()).await {
                while let Ok(Some(config_entry)) = configs.next_entry().await {
                    let config_fname = config_entry.file_name().to_string_lossy().to_string();
                    let _ = progress_tx.send(PreserveProgress::Restoring {
                        path: format!("Themes/{}/{}", theme_name, config_fname),
                    });
                    crate::debug::log(&format!("Restoring theme config: {}/{}", theme_name, config_fname));

                    let dst = theme_dest.join(&config_fname);
                    if let Err(e) = tokio::fs::copy(config_entry.path(), &dst).await {
                        crate::debug::log(&format!("WARNING: Failed to restore theme config {}/{}: {}", theme_name, config_fname, e));
                    }
                }
            }
        }
        let _ = tokio::fs::remove_dir_all(&theme_backup_dir).await;
    }

    crate::debug::log("Dynamic config restore complete");
    Ok(())
}
