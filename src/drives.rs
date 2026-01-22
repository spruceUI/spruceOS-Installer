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

    crate::debug::log_section("Windows Drive Detection");

    let mut drives = Vec::new();
    let drive_bits = unsafe { GetLogicalDrives() };
    crate::debug::log(&format!("Drive bitmask: 0x{:08X}", drive_bits));

    for i in 0..26u8 {
        if (drive_bits >> i) & 1 == 1 {
            let letter = (b'A' + i) as char;
            let root_path: Vec<u16> = format!("{}:\\", letter)
                .encode_utf16()
                .chain(Some(0))
                .collect();

            let drive_type = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_path.as_ptr())) };

            crate::debug::log(&format!("Drive {}: type={}", letter, drive_type));

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

                crate::debug::log(&format!(
                    "  ACCEPTED: {}: label='{}' size={} bytes",
                    letter, label, total_bytes
                ));

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

    crate::debug::log(&format!("Total removable drives found: {}", drives.len()));
    drives
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    crate::debug::log_section("Linux Drive Detection");

    let mut drives = Vec::new();

    // Read block devices from /sys/block/
    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        crate::debug::log("Failed to read /sys/block");
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

        crate::debug::log(&format!("Checking device: {}", name));

        // Check if it's removable
        let removable_path = format!("/sys/block/{}/removable", name);
        let is_removable = std::fs::read_to_string(&removable_path)
            .map(|s| s.trim() == "1")
            .unwrap_or(false);

        crate::debug::log(&format!("  is_removable: {}", is_removable));

        if !is_removable {
            crate::debug::log("  SKIPPED: not removable");
            continue;
        }

        // Get size (in 512-byte sectors)
        let size_path = format!("/sys/block/{}/size", name);
        let size_bytes = std::fs::read_to_string(&size_path)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|sectors| sectors * 512)
            .unwrap_or(0);

        crate::debug::log(&format!("  size_bytes: {}", size_bytes));

        // Skip if size is 0 (no media inserted)
        if size_bytes == 0 {
            crate::debug::log("  SKIPPED: size is 0");
            continue;
        }

        let device_path = format!("/dev/{}", name);

        // Try to find mount point and label
        let (mount_path, label) = find_linux_mount_info(&device_path, &name);

        crate::debug::log(&format!("  label: '{}'", label));
        crate::debug::log(&format!("  mount_path: {:?}", mount_path));
        crate::debug::log("  ACCEPTED");

        drives.push(DriveInfo {
            name: name.clone(),
            device_path,
            mount_path,
            label,
            size_bytes,
        });
    }

    crate::debug::log(&format!("Total removable drives found: {}", drives.len()));
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
fn get_macos_disk_info(disk_id: &str) -> Option<DriveInfo> {
    use std::process::Command;

    let output = Command::new("diskutil")
        .args(["info", disk_id])
        .output()
        .ok()?;

    if !output.status.success() {
        crate::debug::log(&format!("{} - diskutil info failed", disk_id));
        return None;
    }

    let info = String::from_utf8_lossy(&output.stdout);

    let mut size_bytes: u64 = 0;
    let mut label = String::new();
    let mut mount_point: Option<PathBuf> = None;
    let mut is_removable = false;
    let mut is_ejectable = false;
    let mut is_internal = false;
    let mut protocol = String::new();
    let mut media_type = String::new();
    let mut device_location = String::new();

    for line in info.lines() {
        let line = line.trim();

        if line.starts_with("Disk Size:") || line.starts_with("Total Size:") {
            if let Some(start) = line.find('(') {
                if let Some(end) = line.find(" Bytes") {
                    if let Ok(bytes) = line[start + 1..end].trim().replace(",", "").parse::<u64>() {
                        size_bytes = bytes;
                    }
                }
            }
        } else if line.starts_with("Volume Name:") {
            label = line.replace("Volume Name:", "").trim().to_string();
            if label == "Not applicable (no file system)" {
                label = String::new();
            }
        } else if line.starts_with("Device / Media Name:") {
            // Fallback label if Volume Name is empty
            if label.is_empty() {
                label = line.replace("Device / Media Name:", "").trim().to_string();
            }
        } else if line.starts_with("Mount Point:") || line.starts_with("Mounted:") {
            let mp = line.replace("Mount Point:", "").replace("Mounted:", "").trim().to_string();
            if !mp.is_empty() && mp != "Not applicable (no file system)" {
                mount_point = Some(PathBuf::from(mp));
            }
        } else if line.starts_with("Removable Media:") {
            // Parse "Removable Media:     Removable" or "Removable Media:     Yes"
            let value = line.replace("Removable Media:", "").trim().to_lowercase();
            is_removable = value.contains("removable") || value == "yes";
        } else if line.starts_with("Ejectable:") {
            let value = line.replace("Ejectable:", "").trim().to_lowercase();
            is_ejectable = value == "yes";
        } else if line.starts_with("Protocol:") {
            protocol = line.replace("Protocol:", "").trim().to_string();
        } else if line.starts_with("Device Location:") {
            device_location = line.replace("Device Location:", "").trim().to_string();
            is_internal = device_location.to_lowercase().contains("internal");
        } else if line.starts_with("Media Type:") {
            media_type = line.replace("Media Type:", "").trim().to_string();
        }
    }

    // Enhanced detection
    let proto_lower = protocol.to_lowercase();
    let loc_lower = device_location.to_lowercase();

    // Skip disk images (DMGs)
    if proto_lower.contains("disk image") {
        crate::debug::log("  REJECTED: disk image");
        return None;
    }

    // Check various removable indicators
    let is_sd_card = proto_lower.contains("secure digital") || proto_lower.contains("sd");
    let is_usb = proto_lower.contains("usb") || loc_lower.contains("usb");
    let is_external = loc_lower.contains("external");
    
    // SD cards and USB are always removable
    if is_sd_card || is_usb {
        is_removable = true;
    }

    // Debug output
    crate::debug::log(&format!("  label: '{}'", label));
    crate::debug::log(&format!("  size_bytes: {}", size_bytes));
    crate::debug::log(&format!("  protocol: '{}'", protocol));
    crate::debug::log(&format!("  media_type: '{}'", media_type));
    crate::debug::log(&format!("  device_location: '{}'", device_location));
    crate::debug::log(&format!("  is_removable: {}", is_removable));
    crate::debug::log(&format!("  is_ejectable: {}", is_ejectable));
    crate::debug::log(&format!("  is_internal: {}", is_internal));
    crate::debug::log(&format!("  is_sd_card: {}", is_sd_card));
    crate::debug::log(&format!("  is_usb: {}", is_usb));
    crate::debug::log(&format!("  is_external: {}", is_external));

    // A disk is usable if it's removable, ejectable, SD card, USB, or external
    let is_usable = is_removable || is_ejectable || is_sd_card || is_usb || is_external;

    crate::debug::log(&format!("  is_usable: {}", is_usable));

    // Skip synthesized disks
    if loc_lower.contains("synthesized") {
        crate::debug::log("  REJECTED: synthesized disk");
        return None;
    }

    // Accept if usable and has size
    if is_usable && size_bytes > 0 {
        crate::debug::log("  ACCEPTED");
        Some(DriveInfo {
            name: disk_id.to_string(),
            device_path: format!("/dev/{}", disk_id),
            mount_path: mount_point,
            label: if label.is_empty() { disk_id.to_string() } else { label },
            size_bytes,
        })
    } else {
        crate::debug::log(&format!("  REJECTED: not usable (usable={}, size={})", is_usable, size_bytes));
        None
    }
}

// ===========================================================================
// macOS get_removable_drives
// ===========================================================================

#[cfg(target_os = "macos")]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    use std::process::Command;
    use std::collections::HashSet;

    crate::debug::log_section("macOS Drive Detection");

    let mut drives = Vec::new();
    let mut disk_ids: HashSet<String> = HashSet::new();

    if let Ok(output) = Command::new("diskutil")
        .args(["list", "-plist"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_diskutil_plist(&stdout, &mut disk_ids);
            crate::debug::log(&format!("Found disk IDs: {:?}", disk_ids));
        } else {
            crate::debug::log("diskutil list -plist failed");
        }
    } else {
        crate::debug::log("Failed to run diskutil");
    }

    for disk_id in &disk_ids {
        crate::debug::log(&format!("\nChecking disk: {}", disk_id));
        if let Some(drive_info) = get_macos_disk_info(disk_id) {
            crate::debug::log(&format!(">>> ACCEPTED: {} - {} ({} bytes)",
                drive_info.name, drive_info.label, drive_info.size_bytes));
            drives.push(drive_info);
        }
    }

    crate::debug::log(&format!("\nTotal removable drives found: {}", drives.len()));
    drives
}

#[cfg(target_os = "macos")]
fn parse_diskutil_plist(stdout: &str, disk_ids: &mut std::collections::HashSet<String>) {
    let mut in_whole_disks = false;
    
    for line in stdout.lines() {
        let line = line.trim();
        
        if line.contains("<key>WholeDisks</key>") {
            in_whole_disks = true;
            continue;
        }
        
        if in_whole_disks && (line.starts_with("<key>") || line == "</array>") {
            if line.starts_with("<key>") {
                in_whole_disks = false;
            }
        }
        
        if in_whole_disks && line.starts_with("<string>disk") {
            if let Some(start) = line.find("disk") {
                if let Some(end) = line.find("</string>") {
                    let disk_id = &line[start..end];
                    disk_ids.insert(disk_id.to_string());
                }
            }
        }
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
