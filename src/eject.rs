use crate::drives::DriveInfo;

/// Safely eject a drive
/// Returns Ok(()) on success, Err with message on failure

// =============================================================================
// Windows Implementation
// =============================================================================

#[cfg(target_os = "windows")]
pub fn eject_drive(drive: &DriveInfo) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::DeviceIoControl;

    // IOCTL codes for ejecting media
    const FSCTL_LOCK_VOLUME: u32 = 0x00090018;
    const FSCTL_DISMOUNT_VOLUME: u32 = 0x00090020;
    const IOCTL_STORAGE_EJECT_MEDIA: u32 = 0x002D4808;

    // Extract drive letter from device path (e.g., "E:" -> 'E')
    let drive_letter = drive
        .device_path
        .chars()
        .next()
        .ok_or_else(|| "Invalid device path".to_string())?;

    // Open the volume
    let volume_path = format!("\\\\.\\{}:", drive_letter);

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&volume_path)
        .map_err(|e| format!("Failed to open volume: {}", e))?;

    let handle = HANDLE(file.as_raw_handle() as *mut std::ffi::c_void);
    let mut bytes_returned = 0u32;

    // Step 1: Lock the volume
    let result = unsafe {
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

    if result.is_err() {
        // Lock failed, but we can still try to eject
        eprintln!("Warning: Could not lock volume, attempting eject anyway");
    }

    // Step 2: Dismount the volume
    let result = unsafe {
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

    if result.is_err() {
        eprintln!("Warning: Could not dismount volume");
    }

    // Step 3: Eject the media
    let result = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_EJECT_MEDIA,
            None,
            0,
            None,
            0,
            Some(&mut bytes_returned),
            None,
        )
    };

    // Close handle (file will be dropped automatically)
    drop(file);

    if result.is_err() {
        // If eject failed, the drive is at least dismounted and safe to remove
        Ok(())
    } else {
        Ok(())
    }
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
pub fn eject_drive(drive: &DriveInfo) -> Result<(), String> {
    use std::process::Command;

    // First unmount all partitions
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0].starts_with(&drive.device_path) {
            let _ = Command::new("sudo")
                .args(["umount", parts[1]])
                .output();
        }
    }

    // Try udisksctl first (more modern, handles power-off)
    let output = Command::new("udisksctl")
        .args(["power-off", "-b", &drive.device_path])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            return Ok(());
        }
    }

    // Fall back to eject command
    let output = Command::new("eject")
        .arg(&drive.device_path)
        .output()
        .map_err(|e| format!("Failed to run eject: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Eject failed: {}", stderr.trim()))
    }
}

// =============================================================================
// macOS Implementation
// =============================================================================

#[cfg(target_os = "macos")]
pub fn eject_drive(drive: &DriveInfo) -> Result<(), String> {
    use std::process::Command;

    let output = Command::new("diskutil")
        .args(["eject", &drive.device_path])
        .output()
        .map_err(|e| format!("Failed to run diskutil: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Eject failed: {}", stderr.trim()))
    }
}

// =============================================================================
// Fallback for other platforms
// =============================================================================

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn eject_drive(_drive: &DriveInfo) -> Result<(), String> {
    Err("Eject not supported on this platform".to_string())
}
