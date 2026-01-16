use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone)]
pub enum FormatProgress {
    Started,
    CleaningDisk,
    CreatingPartition,
    Formatting,
    Completed,
    Error(String),
}

/// Format a drive to FAT32 with MBR partition table
/// Works for drives of any size (bypasses Windows 32GB FAT32 limit)
#[cfg(windows)]
pub async fn format_drive_fat32(
    drive_letter: char,
    volume_label: &str,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::{IOCTL_STORAGE_GET_DEVICE_NUMBER, IOCTL_DISK_GET_LENGTH_INFO};

    let _ = progress_tx.send(FormatProgress::Started);

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
        return Err(format!("Failed to get disk number for drive {}: {:?}", drive_letter, result));
    }

    let disk_number = device_number.device_number;
    drop(file);

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
        get_drive_size(drive_letter).unwrap_or(32u64 * 1024 * 1024 * 1024)
    };

    drop(disk_file);

    let _ = progress_tx.send(FormatProgress::CleaningDisk);

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

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Diskpart failed: {}", e))?;

    // Check for errors
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("DiskPart has encountered an error")
        || stdout.contains("Virtual Disk Service error")
        || stdout.contains("Access is denied")
    {
        return Err(format!("Diskpart error:\n{}", stdout));
    }

    // Wait a moment for diskpart to finish
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let _ = progress_tx.send(FormatProgress::Formatting);

    // Use our custom FAT32 formatter with disk number (writes to PhysicalDrive directly)
    crate::fat32::format_fat32_large(disk_number, volume_label, disk_size, progress_tx.clone()).await?;

    // Wait for Windows to recognize the new filesystem
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

    Ok(())
}

#[cfg(windows)]
fn get_drive_size(drive_letter: char) -> Result<u64, String> {
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

// Stub implementation for non-Windows platforms
#[cfg(not(windows))]
pub async fn format_drive_fat32(
    _drive_letter: char,
    volume_label: &str,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
) -> Result<(), String> {
    let _ = progress_tx.send(FormatProgress::Started);
    let _ = progress_tx.send(FormatProgress::CleaningDisk);
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let _ = progress_tx.send(FormatProgress::CreatingPartition);
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let _ = progress_tx.send(FormatProgress::Formatting);
    crate::fat32::format_fat32_large(0, volume_label, 64 * 1024 * 1024 * 1024, progress_tx.clone()).await?;

    eprintln!("Warning: Format simulation - not running on Windows.");
    Ok(())
}
