// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

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
    /// Bus the device is attached by (e.g., "USB", "Secure Digital"); empty if unknown
    pub bus: String,
    /// Device or media name the hardware reports (e.g., "SD Card Reader"); empty if unknown
    pub media_name: String,
}

/// Drives at or above this size are flagged in the drive menu: anything this
/// large is almost certainly a hard drive or SSD, not an SD card.
const CARD_SIZE_LIMIT_BYTES: u64 = 1_100_000_000_000;

impl DriveInfo {
    /// "name - label - size (bus; media name)", e.g.
    /// "disk4 - SPRUCE - 128 GB (USB; SD Card Reader)". Empty parts are left
    /// out, so every platform shares the format with whatever it can report.
    pub fn display_name(&self) -> String {
        let mut text = self.name.clone();
        if !self.label.is_empty() {
            text.push_str(&format!(" - {}", self.label));
        }
        text.push_str(&format!(" - {}", format_size_si(self.size_bytes)));

        let hardware: Vec<&str> = [self.bus.as_str(), self.media_name.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        if !hardware.is_empty() {
            text.push_str(&format!(" ({})", hardware.join("; ")));
        }
        text
    }

    pub fn is_card_sized(&self) -> bool {
        self.size_bytes < CARD_SIZE_LIMIT_BYTES
    }
}

/// The drive to preselect: the first card-sized one. None when every drive is
/// large, so erasing one takes a deliberate choice.
pub fn default_drive_index(drives: &[DriveInfo]) -> Option<usize> {
    drives.iter().position(DriveInfo::is_card_sized)
}

/// Formats a byte count in SI units (1 GB = 10^9 bytes), the way card and
/// drive packaging is labeled. One decimal below 10, whole numbers above.
fn format_size_si(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    // Step up while the rounded value would print as 1000 or more
    while value >= 999.5 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }

    let number = if value < 10.0 {
        format!("{:.1}", value).trim_end_matches(".0").to_string()
    } else {
        format!("{:.0}", value)
    };
    format!("{} {}", number, UNITS[unit])
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
    use windows::Win32::System::Ioctl::{IOCTL_STORAGE_GET_DEVICE_NUMBER};
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::Foundation::{GENERIC_READ, CloseHandle};

    const DRIVE_REMOVABLE: u32 = 2;

    #[repr(C)]
    struct STORAGE_DEVICE_NUMBER {
        device_type: u32,
        device_number: u32,
        partition_number: u32,
    }

    crate::debug::log_section("Windows Drive Detection");

    let mut drives = Vec::new();
    let drive_bits = unsafe { GetLogicalDrives() };
    crate::debug::log(&format!("Drive bitmask: 0x{:08X}", drive_bits));

    // Helper function to get physical drive number from logical drive letter
    let get_physical_drive_number = |letter: char| -> Option<u32> {
        let volume_path: Vec<u16> = format!("\\\\.\\{}:", letter)
            .encode_utf16()
            .chain(Some(0))
            .collect();

        unsafe {
            let handle = CreateFileW(
                windows::core::PCWSTR(volume_path.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            );

            if handle.is_err() {
                crate::debug::log(&format!("  Failed to open volume {}:", letter));
                return None;
            }

            let handle = handle.unwrap();
            let mut device_number = STORAGE_DEVICE_NUMBER {
                device_type: 0,
                device_number: 0,
                partition_number: 0,
            };
            let mut bytes_returned: u32 = 0;

            let result = DeviceIoControl(
                handle,
                IOCTL_STORAGE_GET_DEVICE_NUMBER,
                None,
                0,
                Some(&mut device_number as *mut _ as *mut _),
                std::mem::size_of::<STORAGE_DEVICE_NUMBER>() as u32,
                Some(&mut bytes_returned),
                None,
            );

            let _ = CloseHandle(handle);

            if result.is_ok() {
                crate::debug::log(&format!("  Physical drive number: {}", device_number.device_number));
                Some(device_number.device_number)
            } else {
                crate::debug::log("  Failed to get device number");
                None
            }
        }
    };

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

                // Get physical drive number for burning
                let physical_drive_num = get_physical_drive_number(letter);
                let device_path = if let Some(num) = physical_drive_num {
                    format!("\\\\.\\PhysicalDrive{}", num)
                } else {
                    // Fallback to logical drive if we can't get physical drive number
                    crate::debug::log(&format!("  WARNING: Could not determine physical drive number for {}:", letter));
                    format!("{}:", letter)
                };

                crate::debug::log(&format!(
                    "  ACCEPTED: {}: label='{}' size={} bytes device_path='{}'",
                    letter, label, total_bytes, device_path
                ));

                drives.push(DriveInfo {
                    name: format!("{}:", letter),
                    device_path,
                    mount_path: Some(PathBuf::from(format!("{}:\\", letter))),
                    label,
                    size_bytes: total_bytes,
                    bus: String::new(),
                    media_name: String::new(),
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
    use std::collections::HashSet;

    crate::debug::log_section("Linux Drive Detection");

    let mut drives = Vec::new();

    // Read block devices from /sys/block/
    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        crate::debug::log("Failed to read /sys/block");
        return drives;
    };

    // First pass: collect all device names to identify internal eMMC
    // Internal eMMC has boot partitions (mmcblkNboot0, mmcblkNboot1)
    let all_devices: Vec<String> = std::fs::read_dir("/sys/block")
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    // Find mmcblk devices that have boot partitions (these are internal eMMC)
    let internal_emmc: HashSet<String> = all_devices
        .iter()
        .filter(|name| name.contains("boot"))
        .filter_map(|name| {
            // Extract base device: mmcblk1boot0 -> mmcblk1
            if let Some(pos) = name.find("boot") {
                Some(name[..pos].to_string())
            } else {
                None
            }
        })
        .collect();

    crate::debug::log(&format!("Internal eMMC devices (have boot partitions): {:?}", internal_emmc));

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

        // Skip boot partitions
        if name.contains("boot") {
            continue;
        }

        crate::debug::log(&format!("Checking device: {}", name));

        // Check if it's an mmcblk device
        let is_mmcblk = name.starts_with("mmcblk");

        // Skip internal eMMC (mmcblk devices that have boot partitions)
        if is_mmcblk && internal_emmc.contains(&name) {
            crate::debug::log("  SKIPPED: internal eMMC (has boot partitions)");
            continue;
        }

        // Check if it's removable
        let removable_path = format!("/sys/block/{}/removable", name);
        let is_removable = std::fs::read_to_string(&removable_path)
            .map(|s| s.trim() == "1")
            .unwrap_or(false);

        // mmcblk devices without boot partitions are SD cards (even if removable=0)
        let is_sd_card = is_mmcblk && !internal_emmc.contains(&name);

        crate::debug::log(&format!("  is_removable: {}", is_removable));
        crate::debug::log(&format!("  is_sd_card: {}", is_sd_card));

        if !is_removable && !is_sd_card {
            crate::debug::log("  SKIPPED: not removable and not SD card");
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
            crate::debug::log("  SKIPPED: size is 0 (no media inserted)");
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
            bus: String::new(),
            media_name: String::new(),
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

/// The subset of `diskutil list -plist` this module reads.
#[cfg(target_os = "macos")]
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct DiskutilList {
    all_disks_and_partitions: Vec<ListedDisk>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct ListedDisk {
    device_identifier: String,
    /// Set only when the whole disk holds one filesystem, with no partitions
    volume_name: String,
    partitions: Vec<ListedPartition>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct ListedPartition {
    device_identifier: String,
    volume_name: String,
}

#[cfg(target_os = "macos")]
impl ListedDisk {
    /// The volume name of the lowest-numbered partition that has one, or the
    /// disk's own name when it is formatted whole. "EFI" is skipped: every GPT
    /// disk has that system partition, so it names nothing. Empty if nothing
    /// else is named.
    fn label(&self) -> String {
        let names_the_disk = |name: &str| !name.is_empty() && name != "EFI";
        // "disk8s3" is partition 3; anything unparseable sorts last
        let number = |p: &ListedPartition| {
            p.device_identifier
                .rsplit_once('s')
                .and_then(|(_, n)| n.parse::<u32>().ok())
                .unwrap_or(u32::MAX)
        };
        self.partitions
            .iter()
            .filter(|p| names_the_disk(&p.volume_name))
            .min_by_key(|p| number(p))
            .map(|p| p.volume_name.as_str())
            .or(Some(self.volume_name.as_str()).filter(|name| names_the_disk(name)))
            .unwrap_or_default()
            .to_string()
    }
}

/// The subset of `diskutil info -plist <disk>` this module reads. String keys
/// are empty, not absent, when they do not apply (e.g., `MountPoint` on a
/// whole disk), but every key defaults so an older macOS cannot fail the parse.
#[cfg(target_os = "macos")]
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct DiskutilInfo {
    total_size: u64,
    media_name: String,
    media_type: String,
    mount_point: String,
    bus_protocol: String,
    removable_media: bool,
    ejectable: bool,
    /// Absent reads as external, so an unexpected plist lists a disk rather
    /// than hiding the card the user is trying to flash.
    internal: bool,
}

/// Runs `diskutil` with `args` and decodes its plist output.
#[cfg(target_os = "macos")]
fn run_diskutil<T: serde::de::DeserializeOwned>(args: &[&str]) -> Option<T> {
    use std::process::Command;

    let output = match Command::new("diskutil").args(args).output() {
        Ok(output) => output,
        Err(e) => {
            crate::debug::log(&format!("Failed to run diskutil {:?}: {}", args, e));
            return None;
        }
    };
    if !output.status.success() {
        crate::debug::log(&format!("diskutil {:?} failed", args));
        return None;
    }

    plist::from_bytes(&output.stdout)
        .inspect_err(|e| crate::debug::log(&format!("diskutil {:?} gave an unreadable plist: {}", args, e)))
        .ok()
}

#[cfg(target_os = "macos")]
fn get_macos_disk_info(disk: &ListedDisk) -> Option<DriveInfo> {
    let disk_id = disk.device_identifier.as_str();
    let info: DiskutilInfo = run_diskutil(&["info", "-plist", disk_id])?;
    macos_drive_from_info(disk_id, disk.label(), info)
}

#[cfg(target_os = "macos")]
fn macos_drive_from_info(disk_id: &str, label: String, info: DiskutilInfo) -> Option<DriveInfo> {
    let proto_lower = info.bus_protocol.to_lowercase();

    // Skip disk images (DMGs)
    if proto_lower.contains("disk image") {
        crate::debug::log("  REJECTED: disk image");
        return None;
    }

    // Check various removable indicators
    let is_sd_card = proto_lower.contains("secure digital") || proto_lower.contains("sd");
    let is_usb = proto_lower.contains("usb");
    let is_external = !info.internal;

    // SD cards and USB are always removable
    let is_removable = info.removable_media || is_sd_card || is_usb;

    // Debug output
    crate::debug::log(&format!("  label: '{}'", label));
    crate::debug::log(&format!("  media_name: '{}'", info.media_name));
    crate::debug::log(&format!("  size_bytes: {}", info.total_size));
    crate::debug::log(&format!("  protocol: '{}'", info.bus_protocol));
    crate::debug::log(&format!("  media_type: '{}'", info.media_type));
    crate::debug::log(&format!("  is_removable: {}", is_removable));
    crate::debug::log(&format!("  is_ejectable: {}", info.ejectable));
    crate::debug::log(&format!("  internal: {}", info.internal));
    crate::debug::log(&format!("  is_sd_card: {}", is_sd_card));
    crate::debug::log(&format!("  is_usb: {}", is_usb));

    // A disk is usable if it's removable, ejectable, SD card, USB, or external
    let is_usable = is_removable || info.ejectable || is_sd_card || is_usb || is_external;

    crate::debug::log(&format!("  is_usable: {}", is_usable));

    // Accept if usable and has size
    if is_usable && info.total_size > 0 {
        crate::debug::log("  ACCEPTED");
        Some(DriveInfo {
            name: disk_id.to_string(),
            device_path: format!("/dev/{}", disk_id),
            mount_path: Some(info.mount_point)
                .filter(|mp| !mp.is_empty())
                .map(PathBuf::from),
            label,
            size_bytes: info.total_size,
            bus: info.bus_protocol,
            media_name: info.media_name,
        })
    } else {
        crate::debug::log(&format!("  REJECTED: not usable (usable={}, size={})", is_usable, info.total_size));
        None
    }
}

// ===========================================================================
// macOS get_removable_drives
// ===========================================================================

#[cfg(target_os = "macos")]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    crate::debug::log_section("macOS Drive Detection");

    let mut drives = Vec::new();

    // "physical" leaves out disk images and the synthesized disks APFS
    // containers appear as, which share their store's bus and location.
    let disks = run_diskutil::<DiskutilList>(&["list", "-plist", "physical"])
        .map(|list| list.all_disks_and_partitions)
        .unwrap_or_default();
    crate::debug::log(&format!(
        "Found disk IDs: {:?}",
        disks.iter().map(|d| d.device_identifier.as_str()).collect::<Vec<_>>()
    ));

    for disk in &disks {
        crate::debug::log(&format!("\nChecking disk: {}", disk.device_identifier));
        if let Some(drive_info) = get_macos_disk_info(disk) {
            crate::debug::log(&format!(">>> ACCEPTED: {} - {} ({} bytes)",
                drive_info.name, drive_info.label, drive_info.size_bytes));
            drives.push(drive_info);
        }
    }

    crate::debug::log(&format!("\nTotal removable drives found: {}", drives.len()));
    drives
}

// =============================================================================
// Fallback for other platforms
// =============================================================================

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    eprintln!("Warning: Drive detection not implemented for this platform");
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(size_bytes: u64, label: &str, bus: &str, media_name: &str) -> DriveInfo {
        DriveInfo {
            name: "disk4".to_string(),
            device_path: "/dev/disk4".to_string(),
            mount_path: None,
            label: label.to_string(),
            size_bytes,
            bus: bus.to_string(),
            media_name: media_name.to_string(),
        }
    }

    #[test]
    fn sizes_read_like_the_packaging() {
        assert_eq!(format_size_si(31_900_000_000), "32 GB");
        assert_eq!(format_size_si(128_000_000_000), "128 GB");
        assert_eq!(format_size_si(2_000_000_000_000), "2 TB");
        assert_eq!(format_size_si(7_940_000_000), "7.9 GB");
        assert_eq!(format_size_si(1_100_000_000_000), "1.1 TB");
        assert_eq!(format_size_si(512), "512 B");
    }

    #[test]
    fn sizes_that_round_to_a_thousand_move_up_a_unit() {
        assert_eq!(format_size_si(999_600_000_000), "1 TB");
    }

    #[test]
    fn display_name_shows_label_size_bus_and_media_name() {
        let card = drive(128_000_000_000, "SPRUCE", "USB", "SD Card Reader");
        assert_eq!(card.display_name(), "disk4 - SPRUCE - 128 GB (USB; SD Card Reader)");
    }

    #[test]
    fn display_name_leaves_out_what_the_platform_cannot_report() {
        assert_eq!(drive(31_900_000_000, "SPRUCE", "", "").display_name(), "disk4 - SPRUCE - 32 GB");
        assert_eq!(drive(31_900_000_000, "", "USB", "").display_name(), "disk4 - 32 GB (USB)");
        assert_eq!(drive(31_900_000_000, "", "", "").display_name(), "disk4 - 32 GB");
    }

    #[test]
    fn default_is_the_first_card_sized_drive() {
        let large = drive(2_000_000_000_000, "", "", "");
        let card = drive(128_000_000_000, "", "", "");
        assert_eq!(default_drive_index(&[large.clone(), card.clone(), card]), Some(1));
        assert_eq!(default_drive_index(&[large.clone(), large]), None);
        assert_eq!(default_drive_index(&[]), None);
    }

    #[test]
    fn card_sized_stops_at_1_1_tb() {
        assert!(drive(1_099_999_999_999, "", "", "").is_card_sized());
        assert!(!drive(1_100_000_000_000, "", "", "").is_card_sized());
    }

    /// Decodes `diskutil ... -plist` style output from its `<dict>` entries.
    #[cfg(target_os = "macos")]
    fn decode<T: serde::de::DeserializeOwned>(entries: &str) -> T {
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>{}</dict></plist>"#,
            entries
        );
        plist::from_bytes(plist.as_bytes()).unwrap()
    }

    /// An internal, fixed SSD: no key in it makes the disk usable.
    #[cfg(target_os = "macos")]
    const INTERNAL_SSD: &str = "
        <key>BusProtocol</key><string>Apple Fabric</string>
        <key>Internal</key><true/>
        <key>TotalSize</key><integer>1000000000000</integer>";

    #[cfg(target_os = "macos")]
    #[test]
    fn diskutil_info_plist_decodes_into_a_drive() {
        // Pins the keys that reach the drive; the filter's keys are pinned below
        let info: DiskutilInfo = decode(
            "<key>BusProtocol</key><string>USB</string>
             <key>Ejectable</key><true/>
             <key>Internal</key><false/>
             <key>MediaName</key><string>SD Card Reader</string>
             <key>MediaType</key><string>Generic</string>
             <key>MountPoint</key><string>/Volumes/SPRUCE</string>
             <key>RemovableMedia</key><true/>
             <key>SMARTStatus</key><string>Not Supported</string>
             <key>TotalSize</key><integer>128000000000</integer>",
        );
        let drive = macos_drive_from_info("disk4", "SPRUCE".to_string(), info).unwrap();

        assert_eq!(drive.display_name(), "disk4 - SPRUCE - 128 GB (USB; SD Card Reader)");
        assert_eq!(drive.device_path, "/dev/disk4");
        assert_eq!(drive.mount_path, Some(PathBuf::from("/Volumes/SPRUCE")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn internal_fixed_disk_is_rejected() {
        let info = decode(INTERNAL_SSD);
        assert!(macos_drive_from_info("disk0", String::new(), info).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn removable_or_ejectable_alone_makes_an_internal_disk_usable() {
        for key in ["RemovableMedia", "Ejectable"] {
            let info = decode(&format!("{}<key>{}</key><true/>", INTERNAL_SSD, key));
            assert!(macos_drive_from_info("disk0", String::new(), info).is_some(), "{} was not read", key);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn missing_internal_key_reads_as_external() {
        let info = decode(&INTERNAL_SSD.replace("<key>Internal</key><true/>", ""));
        assert!(macos_drive_from_info("disk0", String::new(), info).is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn label_is_the_lowest_numbered_named_partition() {
        // Listed out of order, with an unnamed first partition, the way a
        // card holding a boot volume, a Linux root, and a games volume reads
        let list: DiskutilList = decode(
            "<key>AllDisksAndPartitions</key><array>
                <dict>
                    <key>DeviceIdentifier</key><string>disk4</string>
                    <key>Partitions</key><array>
                        <dict><key>DeviceIdentifier</key><string>disk4s3</string>
                              <key>VolumeName</key><string>ROMS</string></dict>
                        <dict><key>DeviceIdentifier</key><string>disk4s1</string></dict>
                        <dict><key>DeviceIdentifier</key><string>disk4s2</string>
                              <key>VolumeName</key><string>BOOT</string></dict>
                    </array>
                </dict>
                <dict><key>DeviceIdentifier</key><string>disk5</string></dict>
                <dict><key>DeviceIdentifier</key><string>disk6</string>
                      <key>VolumeName</key><string>STICK</string></dict>
            </array>",
        );
        let [card, blank, whole] = list.all_disks_and_partitions.as_slice() else {
            panic!("expected three disks");
        };

        assert_eq!(card.device_identifier, "disk4");
        assert_eq!(card.label(), "BOOT");
        assert_eq!(blank.device_identifier, "disk5");
        assert_eq!(blank.label(), "");
        // Formatted whole, with no partition table, the name is the disk's own
        assert_eq!(whole.label(), "STICK");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn efi_partition_never_names_the_disk() {
        let partition = |id: &str, name: &str| ListedPartition {
            device_identifier: id.to_string(),
            volume_name: name.to_string(),
        };
        let gpt_data = ListedDisk {
            device_identifier: "disk6".to_string(),
            partitions: vec![partition("disk6s1", "EFI"), partition("disk6s2", "DATA")],
            ..Default::default()
        };
        let gpt_only_efi = ListedDisk {
            device_identifier: "disk7".to_string(),
            partitions: vec![partition("disk7s1", "EFI"), partition("disk7s2", "")],
            ..Default::default()
        };

        assert_eq!(gpt_data.label(), "DATA");
        assert_eq!(gpt_only_efi.label(), "");
    }
}
