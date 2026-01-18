use crate::drives::DriveInfo;

/// Safely eject a drive
/// Returns Ok(()) on success, Err with message on failure

// =============================================================================
// Windows Implementation
// =============================================================================

#[cfg(target_os = "windows")]
pub fn eject_drive(drive: &DriveInfo) -> Result<(), String> {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::core::PCWSTR;

    // IOCTL codes
    const FSCTL_LOCK_VOLUME: u32 = 0x00090018;
    const FSCTL_DISMOUNT_VOLUME: u32 = 0x00090020;
    const IOCTL_STORAGE_EJECT_MEDIA: u32 = 0x002D4808;
    const IOCTL_STORAGE_MEDIA_REMOVAL: u32 = 0x002D4804;
    const FSCTL_UNLOCK_VOLUME: u32 = 0x0009001C;

    // Extract drive letter
    let drive_letter = drive.device_path.chars().next().ok_or("Invalid device path")?;
    let volume_path_str = format!("\\\\.\\{}:", drive_letter);
    let volume_path_wide: Vec<u16> = volume_path_str.encode_utf16().chain(Some(0)).collect();

    // Get a handle to the volume.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(volume_path_wide.as_ptr()),
            0, // No specific access needed for these IOCTLs
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map_err(|e| format!("Failed to get volume handle: {}", e))?;

    if handle.is_invalid() {
        return Err("Invalid volume handle".to_string());
    }

    let mut bytes_returned = 0u32;

    // Step 1: Lock the volume to force dismount
    unsafe {
        let _ = DeviceIoControl(handle, FSCTL_LOCK_VOLUME, None, 0, None, 0, Some(&mut bytes_returned), None);
    }

    // Step 2: Dismount the volume
    unsafe {
        let _ = DeviceIoControl(handle, FSCTL_DISMOUNT_VOLUME, None, 0, None, 0, Some(&mut bytes_returned), None);
    }

    // Step 3: Set media removal to be allowed
    #[repr(C)]
    struct PreventMediaRemoval { PreventMediaRemoval: bool, }
    let removal_policy = PreventMediaRemoval { PreventMediaRemoval: false };

    unsafe {
        let _ = DeviceIoControl(
            handle, IOCTL_STORAGE_MEDIA_REMOVAL,
            Some(&removal_policy as *const _ as *const std::ffi::c_void),
            size_of::<PreventMediaRemoval>() as u32,
            None, 0, Some(&mut bytes_returned), None,
        );
    }

    // Step 4: Eject the media
    let eject_result = unsafe {
        DeviceIoControl(handle, IOCTL_STORAGE_EJECT_MEDIA, None, 0, None, 0, Some(&mut bytes_returned), None)
    };

    // Step 5: Unlock the volume (cleanup)
    unsafe {
        let _ = DeviceIoControl(handle, FSCTL_UNLOCK_VOLUME, None, 0, None, 0, Some(&mut bytes_returned), None);
    }

    // Step 6: Close the handle
    unsafe { CloseHandle(handle); }

    if eject_result.is_ok() {
        Ok(())
    } else {
        Err("Eject command failed. The drive may be dismounted and safe to remove.".to_string())
    }
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
pub fn eject_drive(drive: &DriveInfo) -> Result<(), String> {
    use std::process::Command;

    // udisksctl (called below) handles unmounting partitions before powering off the device.
    // The explicit `sudo umount` loop is removed as the application now runs with elevated privileges,
    // and udisksctl is the preferred method for ejecting.

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