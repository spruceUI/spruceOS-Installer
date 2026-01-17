use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DriveInfo {
    /// Display name (e.g., "E:" on Windows, "sdb" on Linux, "disk2" on macOS)
    pub name: String,
    /// Full device path (e.g., "E:" on Windows, "/dev/sdb" on Linux, "/dev/disk2" on macOS)
    pub device_path: String,
    /// Mount point path (e.g., "E:\\" on Windows, "/media/user/DRIVE" on Linux, "/Volumes/DRIVE" on macOS)
    pub mount_path: Option<PathBuf>,
    /// Volume label
    pub label: String,
    /// Total size in bytes
    pub size_bytes: u64,
}

impl DriveInfo {
    pub fn display_name(&self) -> String {
        let size_gb = self.size_bytes as f64 / 1_073_741_824.0;
        if self.label.is_empty() {
            format!("{} ({:.1} GB)", self.name, size_gb)
        } else {
            format!("{} - {} ({:.1} GB)", self.name, self.label, size_gb)
        }
    }
}

// =============================================================================
// Windows Implementation
// =============================================================================

#[cfg(target_os = "windows")]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    const DRIVE_REMOVABLE: u32 = 2;

    let mut drives = Vec::new();
    let drive_bits = unsafe { GetLogicalDrives() };

    for i in 0..26u8 {
        if (drive_bits >> i) & 1 == 1 {
            let letter = (b'A' + i) as char;
            let root_path: Vec<u16> = format!("{}:\\", letter)
                .encode_utf16()
                .chain(Some(0))
                .collect();

            let drive_type = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_path.as_ptr())) };

            if drive_type == DRIVE_REMOVABLE {
                let mut label_buf = [0u16; 261];
                let mut serial_number: u32 = 0;
                let mut max_component_len: u32 = 0;
                let mut fs_flags: u32 = 0;
                let mut fs_buf = [0u16; 261];

                let label = unsafe {
                    if GetVolumeInformationW(
                        windows::core::PCWSTR(root_path.as_ptr()),
                        Some(&mut label_buf),
                        Some(&mut serial_number),
                        Some(&mut max_component_len),
                        Some(&mut fs_flags),
                        Some(&mut fs_buf),
                    )
                    .is_ok()
                    {
                        let len = label_buf.iter().position(|&c| c == 0).unwrap_or(label_buf.len());
                        OsString::from_wide(&label_buf[..len])
                            .to_string_lossy()
                            .to_string()
                    } else {
                        String::new()
                    }
                };

                let mut total_bytes = 0u64;

                unsafe {
                    let _ = GetDiskFreeSpaceExW(
                        windows::core::PCWSTR(root_path.as_ptr()),
                        None,
                        Some(&mut total_bytes),
                        None,
                    );
                }

                drives.push(DriveInfo {
                    name: format!("{}:", letter),
                    device_path: format!("{}:", letter),
                    mount_path: Some(PathBuf::from(format!("{}:\\", letter))),
                    label,
                    size_bytes: total_bytes,
                });
            }
        }
    }

    drives
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    let mut drives = Vec::new();

    // Read block devices from /sys/block/
    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        return drives;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip non-disk devices (loop, ram, etc.)
        if name.starts_with("loop")
            || name.starts_with("ram")
            || name.starts_with("zram")
            || name.starts_with("dm-")
        {
            continue;
        }

        // Check if it's removable
        let removable_path = format!("/sys/block/{}/removable", name);
        let is_removable = std::fs::read_to_string(&removable_path)
            .map(|s| s.trim() == "1")
            .unwrap_or(false);

        if !is_removable {
            continue;
        }

        // Get size (in 512-byte sectors)
        let size_path = format!("/sys/block/{}/size", name);
        let size_bytes = std::fs::read_to_string(&size_path)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|sectors| sectors * 512)
            .unwrap_or(0);

        // Skip if size is 0 (no media inserted)
        if size_bytes == 0 {
            continue;
        }

        let device_path = format!("/dev/{}", name);

        // Try to find mount point and label
        let (mount_path, label) = find_linux_mount_info(&device_path, &name);

        drives.push(DriveInfo {
            name: name.clone(),
            device_path,
            mount_path,
            label,
            size_bytes,
        });
    }

    drives
}

#[cfg(target_os = "linux")]
fn find_linux_mount_info(device_path: &str, device_name: &str) -> (Option<PathBuf>, String) {
    let mut mount_path = None;
    let mut label = String::new();

    // Check /proc/mounts for mount point
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                // Check if this mount matches our device or a partition on it
                if parts[0].starts_with(device_path) || parts[0].starts_with(&format!("/dev/{}p", device_name)) || parts[0].starts_with(&format!("/dev/{}1", device_name)) {
                    mount_path = Some(PathBuf::from(parts[1]));
                    break;
                }
            }
        }
    }

    // Try to get label from /dev/disk/by-label/
    if let Ok(entries) = std::fs::read_dir("/dev/disk/by-label") {
        for entry in entries.flatten() {
            if let Ok(target) = std::fs::read_link(entry.path()) {
                let target_str = target.to_string_lossy();
                if target_str.contains(device_name) {
                    label = entry.file_name().to_string_lossy().to_string();
                    // URL-decode the label (spaces are encoded as \x20)
                    label = label.replace("\\x20", " ");
                    break;
                }
            }
        }
    }

    (mount_path, label)
}

// =============================================================================
// macOS Implementation
// =============================================================================

#[cfg(target_os = "macos")]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    use std::process::Command;
    use std::collections::HashSet;
    use std::io::Write;

    let log_path = std::env::temp_dir().join("spruce_installer_debug.txt");

    // Helper to append to log file
    fn write_log(path: &std::path::Path, msg: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{}", msg);
        }
    }

    // Clear and start fresh
    let _ = std::fs::write(&log_path, "");

    write_log(&log_path, "=== SpruceOS Installer Debug Log ===");
    write_log(&log_path, &format!("Log file: {:?}", log_path));
    write_log(&log_path, &format!("Timestamp: {:?}", std::time::SystemTime::now()));
    write_log(&log_path, "");
    write_log(&log_path, "macOS get_removable_drives starting...");

    let mut drives = Vec::new();
    let mut disk_ids: HashSet<String> = HashSet::new();

    // List all disks and let get_macos_disk_info filter appropriately
    if let Ok(output) = Command::new("diskutil")
        .args(["list", "-plist"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_diskutil_plist(&stdout, &mut disk_ids);
            write_log(&log_path, &format!("Found disk IDs: {:?}", disk_ids));
        } else {
            write_log(&log_path, "diskutil list -plist failed");
        }
    } else {
        write_log(&log_path, "Failed to run diskutil");
    }

    // Get info for each disk
    for disk_id in &disk_ids {
        write_log(&log_path, &format!("\nChecking disk: {}", disk_id));
        if let Some(drive_info) = get_macos_disk_info(disk_id, &log_path) {
            write_log(&log_path, &format!(">>> ACCEPTED: {} - {} ({} bytes)",
                drive_info.name, drive_info.label, drive_info.size_bytes));
            drives.push(drive_info);
        }
    }

    write_log(&log_path, &format!("\nTotal drives found: {}", drives.len()));
    write_log(&log_path, "\n=== End Debug Log ===");

    // Print location for user
    eprintln!("Debug log written to: {:?}", log_path);

    drives
}

#[cfg(target_os = "macos")]
fn parse_diskutil_plist(stdout: &str, disk_ids: &mut std::collections::HashSet<String>) {
    // Parse plist to find whole disk identifiers like "disk2", "disk3"
    // Avoid partitions like "disk2s1"
    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("<string>disk") {
            // Extract the disk ID between <string> and </string>
            if let Some(start) = line.find("disk") {
                if let Some(end) = line.find("</string>") {
                    let disk_id = &line[start..end];
                    // Only include whole disks (no 's' followed by a number = partition)
                    if !disk_id.contains('s') || !disk_id.chars().last().unwrap_or('x').is_ascii_digit() {
                        // Double check it's a valid disk ID pattern (disk followed by number)
                        let after_disk = &disk_id[4..];
                        if after_disk.chars().all(|c| c.is_ascii_digit()) && !after_disk.is_empty() {
                            disk_ids.insert(disk_id.to_string());
                        }
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn get_macos_disk_info(disk_id: &str, log_path: &std::path::Path) -> Option<DriveInfo> {
    use std::process::Command;
    use std::io::Write;

    let log = |msg: &str| {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) {
            let _ = writeln!(f, "{}", msg);
        }
    };

    let output = Command::new("diskutil")
        .args(["info", disk_id])
        .output()
        .ok()?;

    if !output.status.success() {
        log(&format!("{} - diskutil info failed", disk_id));
        return None;
    }

    let info = String::from_utf8_lossy(&output.stdout);

    let mut size_bytes: u64 = 0;
    let mut label = String::new();
    let mut mount_point: Option<PathBuf> = None;
    let mut is_removable = false;
    let mut is_ejectable = false;
    let mut is_internal = false;
    let mut is_physical = false;
    let mut protocol = String::new();
    let mut media_type = String::new();

    for line in info.lines() {
        let line = line.trim();

        if line.starts_with("Disk Size:") {
            // Parse size like "Disk Size:                 31.9 GB (31914983424 Bytes)..."
            if let Some(start) = line.find('(') {
                if let Some(end) = line.find(" Bytes") {
                    if let Ok(bytes) = line[start + 1..end].trim().parse::<u64>() {
                        size_bytes = bytes;
                    }
                }
            }
        } else if line.starts_with("Volume Name:") {
            label = line.replace("Volume Name:", "").trim().to_string();
            if label == "Not applicable (no file system)" {
                label = String::new();
            }
        } else if line.starts_with("Mount Point:") {
            let mp = line.replace("Mount Point:", "").trim().to_string();
            if !mp.is_empty() && mp != "Not applicable (no file system)" {
                mount_point = Some(PathBuf::from(mp));
            }
        } else if line.starts_with("Removable Media:") {
            // Can be "Removable", "Yes", or "Fixed"/"No"
            let value = line.replace("Removable Media:", "").trim().to_lowercase();
            is_removable = value.contains("removable") || value.contains("yes");
        } else if line.starts_with("Ejectable:") {
            // Ejectable media (like SD cards in built-in readers)
            let value = line.replace("Ejectable:", "").trim().to_lowercase();
            is_ejectable = value.contains("yes");
        } else if line.starts_with("Protocol:") {
            protocol = line.replace("Protocol:", "").trim().to_string();
            // USB and SD card protocols indicate removable media
            let proto_lower = protocol.to_lowercase();
            if proto_lower.contains("usb") || proto_lower.contains("secure digital") || proto_lower.contains("sd") {
                is_removable = true;
            }
        } else if line.starts_with("Device Location:") {
            // Check if it's internal
            if line.to_lowercase().contains("internal") {
                is_internal = true;
            }
        } else if line.starts_with("Virtual:") {
            // Physical (non-virtual) disks
            let value = line.replace("Virtual:", "").trim().to_lowercase();
            if value.contains("no") {
                is_physical = true;
            }
        } else if line.starts_with("Media Type:") {
            media_type = line.replace("Media Type:", "").trim().to_string();
            // SD cards often show as "SD Card" or similar
            let mt_lower = media_type.to_lowercase();
            if mt_lower.contains("sd") || mt_lower.contains("card") {
                is_removable = true;
            }
        }
    }

    // Debug output
    log(&format!("  label: '{}'", label));
    log(&format!("  size_bytes: {}", size_bytes));
    log(&format!("  protocol: '{}'", protocol));
    log(&format!("  media_type: '{}'", media_type));
    log(&format!("  is_removable: {}", is_removable));
    log(&format!("  is_ejectable: {}", is_ejectable));
    log(&format!("  is_internal: {}", is_internal));
    log(&format!("  is_physical: {}", is_physical));

    // A disk is considered usable if:
    // 1. It's explicitly removable, OR
    // 2. It's ejectable (like SD cards in built-in readers)
    let is_usable = is_removable || is_ejectable;

    log(&format!("  is_usable: {}", is_usable));

    // Skip if it's internal, not ejectable, and not removable (system drives)
    if is_internal && !is_ejectable && !is_removable {
        log("  REJECTED: internal and not ejectable/removable");
        return None;
    }

    // Only return if it's usable and has a size
    if is_usable && size_bytes > 0 {
        log("  ACCEPTED");
        Some(DriveInfo {
            name: disk_id.to_string(),
            device_path: format!("/dev/{}", disk_id),
            mount_path: mount_point,
            label,
            size_bytes,
        })
    } else {
        log(&format!("  REJECTED: not usable (usable={}, size={})", is_usable, size_bytes));
        None
    }
}

// =============================================================================
// Fallback for other platforms
// =============================================================================

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    eprintln!("Warning: Drive detection not implemented for this platform");
    Vec::new()
}
