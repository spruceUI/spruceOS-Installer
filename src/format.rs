use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
use std::process::Stdio;
#[cfg(target_os = "windows")]
use tokio::io::AsyncWriteExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone)]
pub enum FormatProgress {
    Started,
    Unmounting,
    CleaningDisk,
    CreatingPartition,
    Formatting,
    Progress { percent: u8 },
    Completed,
    Cancelled,
    Error(String),
}

// =============================================================================
// Windows Implementation
// =============================================================================

/// Format a drive to FAT32 with MBR partition table (Windows)
/// Works for drives of any size (bypasses Windows 32GB FAT32 limit)
#[cfg(target_os = "windows")]
pub async fn format_drive_fat32(
    device_path: &str,
    volume_label: &str,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::{IOCTL_DISK_GET_LENGTH_INFO, IOCTL_STORAGE_GET_DEVICE_NUMBER};

    crate::debug::log_section("Windows Format Operation");
    crate::debug::log(&format!("Device path: {}", device_path));
    crate::debug::log(&format!("Volume label: {}", volume_label));

    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    let _ = progress_tx.send(FormatProgress::Started);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 0 });

    // Extract drive letter from device path (e.g., "E:" -> 'E')
    let drive_letter = device_path
        .chars()
        .next()
        .ok_or_else(|| "Invalid device path".to_string())?;

    // Get disk number using Windows API directly
    let volume_path = format!("\\\\.\\{}:", drive_letter);

    let file = OpenOptions::new()
        .read(true)
        .open(&volume_path)
        .map_err(|e| format!("Failed to open volume {}: {}", drive_letter, e))?;

    let handle = HANDLE(file.as_raw_handle() as *mut std::ffi::c_void);

    #[repr(C)]
    #[derive(Default)]
    struct StorageDeviceNumber {
        device_type: u32,
        device_number: u32,
        partition_number: u32,
    }

    let mut device_number = StorageDeviceNumber::default();
    let mut bytes_returned = 0u32;

    let result = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            None,
            0,
            Some(&mut device_number as *mut _ as *mut std::ffi::c_void),
            std::mem::size_of::<StorageDeviceNumber>() as u32,
            Some(&mut bytes_returned),
            None,
        )
    };

    if result.is_err() {
        return Err(format!(
            "Failed to get disk number for drive {}: {:?}",
            drive_letter, result
        ));
    }

    let disk_number = device_number.device_number;
    crate::debug::log(&format!("Disk number: {}", disk_number));
    drop(file);

    let _ = progress_tx.send(FormatProgress::Progress { percent: 10 });

    // Get the disk size from the physical disk
    let disk_path = format!("\\\\.\\PhysicalDrive{}", disk_number);
    let disk_file = OpenOptions::new()
        .read(true)
        .open(&disk_path)
        .map_err(|e| format!("Failed to open physical disk {}: {}", disk_number, e))?;

    let disk_handle = HANDLE(disk_file.as_raw_handle() as *mut std::ffi::c_void);

    #[repr(C)]
    #[derive(Default)]
    struct GetLengthInfo {
        length: i64,
    }

    let mut length_info = GetLengthInfo::default();

    let result = unsafe {
        DeviceIoControl(
            disk_handle,
            IOCTL_DISK_GET_LENGTH_INFO,
            None,
            0,
            Some(&mut length_info as *mut _ as *mut std::ffi::c_void),
            std::mem::size_of::<GetLengthInfo>() as u32,
            Some(&mut bytes_returned),
            None,
        )
    };

    let disk_size = if result.is_ok() && length_info.length > 0 {
        length_info.length as u64
    } else {
        // Fallback: try GetDiskFreeSpaceExW
        get_drive_size_windows(drive_letter).unwrap_or(32u64 * 1024 * 1024 * 1024)
    };

    crate::debug::log(&format!("Disk size: {} bytes ({:.2} GB)", disk_size, disk_size as f64 / 1_073_741_824.0));
    drop(disk_file);

    // Check for cancellation before destructive operation
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    let _ = progress_tx.send(FormatProgress::CleaningDisk);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 20 });
    crate::debug::log("Running diskpart to clean and partition disk...");

    // Create diskpart script for partitioning only (no format)
    let script = create_partition_script(disk_number);

    // Run diskpart with the script
    let mut child = Command::new("diskpart")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to start diskpart: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to diskpart: {}", e))?;
    }

    let _ = progress_tx.send(FormatProgress::CreatingPartition);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 40 });

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Diskpart failed: {}", e))?;

    // Check for errors
    let stdout = String::from_utf8_lossy(&output.stdout);
    crate::debug::log(&format!("Diskpart output:\n{}", stdout));

    if stdout.contains("DiskPart has encountered an error")
        || stdout.contains("Virtual Disk Service error")
        || stdout.contains("Access is denied")
    {
        crate::debug::log(&format!("Diskpart error detected"));
        return Err(format!("Diskpart error:\n{}", stdout));
    }

    crate::debug::log("Diskpart completed successfully");
    let _ = progress_tx.send(FormatProgress::Progress { percent: 50 });

    // Check for cancellation before format
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    // Wait for diskpart to finish and Windows to settle
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    let _ = progress_tx.send(FormatProgress::Formatting);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 60 });
    crate::debug::log("Locking and dismounting volume before FAT32 format...");

    // Lock and dismount the volume to prevent Windows from interfering
    // The new partition will likely be mounted on the same drive letter
    lock_and_dismount_volume(drive_letter).await;
    crate::debug::log("Volume locked/dismounted");

    let _ = progress_tx.send(FormatProgress::Progress { percent: 70 });

    // Use our custom FAT32 formatter with disk number (writes to PhysicalDrive directly)
    crate::debug::log("Starting custom FAT32 format...");
    crate::fat32::format_fat32_large(disk_number, volume_label, disk_size, progress_tx.clone())
        .await?;

    let _ = progress_tx.send(FormatProgress::Progress { percent: 95 });
    crate::debug::log("FAT32 format completed, waiting for Windows to recognize filesystem...");
    // Wait for Windows to recognize the new filesystem
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

    let _ = progress_tx.send(FormatProgress::Progress { percent: 100 });
    let _ = progress_tx.send(FormatProgress::Completed);
    crate::debug::log("Windows format operation completed successfully");
    Ok(())
}

#[cfg(target_os = "windows")]
fn get_drive_size_windows(drive_letter: char) -> Result<u64, String> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let root_path: Vec<u16> = format!("{}:\\", drive_letter)
        .encode_utf16()
        .chain(Some(0))
        .collect();

    let mut total_bytes = 0u64;

    unsafe {
        let _ = GetDiskFreeSpaceExW(
            windows::core::PCWSTR(root_path.as_ptr()),
            None,
            Some(&mut total_bytes),
            None,
        );
    }

    if total_bytes == 0 {
        total_bytes = 32u64 * 1024 * 1024 * 1024;
    }

    Ok(total_bytes)
}

#[cfg(target_os = "windows")]
fn create_partition_script(disk_number: u32) -> String {
    // Only partition, don't format - we'll use our custom formatter
    format!(
        r#"select disk {}
clean
create partition primary
select partition 1
active
exit
"#,
        disk_number
    )
}

#[cfg(target_os = "windows")]
async fn lock_and_dismount_volume(drive_letter: char) {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::DeviceIoControl;

    const FSCTL_LOCK_VOLUME: u32 = 0x00090018;
    const FSCTL_DISMOUNT_VOLUME: u32 = 0x00090020;

    let volume_path = format!("\\\\.\\{}:", drive_letter);

    // Try to open and lock the volume
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(&volume_path)
    {
        Ok(f) => f,
        Err(_) => return, // Volume might not exist yet, that's okay
    };

    let handle = HANDLE(file.as_raw_handle() as *mut std::ffi::c_void);
    let mut bytes_returned = 0u32;

    // Try to lock the volume
    let _ = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_LOCK_VOLUME,
            None,
            0,
            None,
            0,
            Some(&mut bytes_returned),
            None,
        )
    };

    // Dismount the volume
    let _ = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_DISMOUNT_VOLUME,
            None,
            0,
            None,
            0,
            Some(&mut bytes_returned),
            None,
        )
    };

    // Keep the handle open briefly to maintain the lock
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    drop(file);
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
pub async fn format_drive_fat32(
    device_path: &str,
    volume_label: &str,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    crate::debug::log_section("Linux Format Operation");
    crate::debug::log(&format!("Device path: {}", device_path));
    crate::debug::log(&format!("Volume label: {}", volume_label));

    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    let _ = progress_tx.send(FormatProgress::Started);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 0 });

    // Unmount any mounted partitions on this device
    let _ = progress_tx.send(FormatProgress::Unmounting);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 10 });
    crate::debug::log("Unmounting device partitions...");
    unmount_linux_device(device_path).await?;
    crate::debug::log("Unmount complete");

    // Check for cancellation before destructive operation
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    let _ = progress_tx.send(FormatProgress::CleaningDisk);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 20 });
    crate::debug::log("Creating msdos partition table with parted...");

    // Create a new partition table and partition using parted
    // First, create a new msdos partition table
    let output = Command::new("parted")
        .args(["-s", device_path, "mklabel", "msdos"])
        .output()
        .await
        .map_err(|e| format!("Failed to run parted: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        crate::debug::log(&format!("Parted mklabel failed: {}", stderr));
        return Err(format!("Failed to create partition table: {}", stderr));
    }
    crate::debug::log("Partition table created");

    let _ = progress_tx.send(FormatProgress::CreatingPartition);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 40 });
    crate::debug::log("Creating primary partition...");

    // Create a primary partition spanning the entire disk
    let output = Command::new("parted")
        .args([
            "-s", device_path, "mkpart", "primary", "fat32", "1MiB", "100%",
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to create partition: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        crate::debug::log(&format!("Parted mkpart failed: {}", stderr));
        return Err(format!("Failed to create partition: {}", stderr));
    }
    crate::debug::log("Primary partition created");

    let _ = progress_tx.send(FormatProgress::Progress { percent: 50 });

    // Set the partition as bootable
    crate::debug::log("Setting boot flag...");
    let _ = Command::new("parted")
        .args(["-s", device_path, "set", "1", "boot", "on"])
        .output()
        .await;

    // Wait for the kernel to recognize the new partition
    crate::debug::log("Running partprobe...");
    let _ = Command::new("partprobe")
        .args([device_path])
        .output()
        .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // Check for cancellation before format
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    let _ = progress_tx.send(FormatProgress::Formatting);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 60 });

    // Determine the partition path (e.g., /dev/sdb1 or /dev/mmcblk0p1)
    let partition_path = if device_path.contains("mmcblk") || device_path.contains("nvme") {
        format!("{}p1", device_path)
    } else {
        format!("{}1", device_path)
    };
    crate::debug::log(&format!("Partition path: {}", partition_path));

    // Format the partition as FAT32
    crate::debug::log("Running mkfs.vfat...");
    let output = Command::new("mkfs.vfat")
        .args(["-F", "32", "-n", volume_label, &partition_path])
        .output()
        .await
        .map_err(|e| format!("Failed to run mkfs.vfat: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        crate::debug::log(&format!("mkfs.vfat failed: {}", stderr));
        return Err(format!("Failed to format partition: {}", stderr));
    }

    let _ = progress_tx.send(FormatProgress::Progress { percent: 100 });
    crate::debug::log("Linux format operation completed successfully");
    let _ = progress_tx.send(FormatProgress::Completed);
    Ok(())
}

#[cfg(target_os = "linux")]
async fn unmount_linux_device(device_path: &str) -> Result<(), String> {
    // Read /proc/mounts to find all mount points for this device
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0].starts_with(device_path) {
            let mount_point = parts[1];
            let _ = Command::new("umount")
                .args([mount_point])
                .output()
                .await;
        }
    }

    // Give the system time to complete unmounting
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    Ok(())
}

// =============================================================================
// macOS Implementation
// =============================================================================

#[cfg(target_os = "macos")]
pub async fn format_drive_fat32(
    device_path: &str,
    volume_label: &str,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    crate::debug::log_section("macOS Format Operation");
    crate::debug::log(&format!("Device path: {}", device_path));
    crate::debug::log(&format!("Volume label: {}", volume_label));

    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    let _ = progress_tx.send(FormatProgress::Started);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 0 });

    // Extract disk identifier from device path (e.g., "/dev/disk2" -> "disk2")
    let disk_id = device_path
        .strip_prefix("/dev/")
        .unwrap_or(device_path);
    crate::debug::log(&format!("Disk ID: {}", disk_id));

    let _ = progress_tx.send(FormatProgress::Unmounting);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 10 });
    crate::debug::log("Unmounting disk...");

    // Unmount the disk first
    let output = Command::new("diskutil")
        .args(["unmountDisk", device_path])
        .output()
        .await
        .map_err(|e| format!("Failed to unmount disk: {}", e))?;

    if !output.status.success() {
        // It's okay if unmount fails (might not be mounted)
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("was already unmounted") && !stderr.contains("not mounted") {
            // Log but continue
            crate::debug::log(&format!("Unmount warning: {}", stderr));
        }
    } else {
        crate::debug::log("Disk unmounted successfully");
    }

    // Check for cancellation before destructive format
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(FormatProgress::Cancelled);
        return Err("Format cancelled".to_string());
    }

    let _ = progress_tx.send(FormatProgress::Formatting);
    let _ = progress_tx.send(FormatProgress::Progress { percent: 30 });
    crate::debug::log("Running diskutil eraseDisk...");

    // Use diskutil to erase and format the disk as FAT32 with MBR
    // diskutil eraseDisk FAT32 LABEL MBRFormat /dev/diskN
    let output = Command::new("diskutil")
        .args([
            "eraseDisk",
            "FAT32",
            volume_label,
            "MBRFormat",
            device_path,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to format disk: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    crate::debug::log(&format!("diskutil stdout: {}", stdout));
    if !stderr.is_empty() {
        crate::debug::log(&format!("diskutil stderr: {}", stderr));
    }

    if !output.status.success() {
        crate::debug::log("diskutil eraseDisk failed");
        return Err(format!(
            "Failed to format disk: {}\n{}",
            stderr.trim(),
            stdout.trim()
        ));
    }

    let _ = progress_tx.send(FormatProgress::Progress { percent: 100 });
    crate::debug::log("macOS format operation completed successfully");
    let _ = progress_tx.send(FormatProgress::Completed);
    Ok(())
}

// =============================================================================
// Fallback for other platforms
// =============================================================================

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub async fn format_drive_fat32(
    _device_path: &str,
    _volume_label: &str,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
    _cancel_token: CancellationToken,
) -> Result<(), String> {
    let _ = progress_tx.send(FormatProgress::Error(
        "Formatting not supported on this platform".to_string(),
    ));
    Err("Formatting not supported on this platform".to_string())
}
