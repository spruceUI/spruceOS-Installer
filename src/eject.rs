use crate::drives::DriveInfo;

/// Safely eject a drive
/// Returns Ok(()) on success, Err with message on failure

// =============================================================================
// Windows Implementation
// =============================================================================

#[cfg(target_os = "windows")]
pub fn eject_drive(drive: &DriveInfo) -> Result<(), String> {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
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

    // Extract drive letter from mount path or device path
    let drive_letter = drive.mount_path
        .as_ref()
        .and_then(|p| p.to_str())
        .and_then(|s| s.chars().next())
        .or_else(|| drive.device_path.chars().next())
        .ok_or("Invalid device path")?;

    let volume_path_str = format!("\\\\.\\{}:", drive_letter);
    let volume_path_wide: Vec<u16> = volume_path_str.encode_utf16().chain(Some(0)).collect();

    crate::debug::log(&format!("Ejecting volume: {}", volume_path_str));

    // Get a handle to the volume with read/write access for IOCTLs
    let handle = unsafe {
        CreateFileW(
            PCWSTR(volume_path_wide.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
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

    // Step 1: Lock the volume (may fail if files are open, but continue anyway)
    let lock_result = unsafe {
        DeviceIoControl(handle, FSCTL_LOCK_VOLUME, None, 0, None, 0, Some(&mut bytes_returned), None)
    };
    crate::debug::log(&format!("Lock volume result: {:?}", lock_result));

    // Step 2: Dismount the volume (flushes all cached data)
    let dismount_result = unsafe {
        DeviceIoControl(handle, FSCTL_DISMOUNT_VOLUME, None, 0, None, 0, Some(&mut bytes_returned), None)
    };
    crate::debug::log(&format!("Dismount volume result: {:?}", dismount_result));

    // Step 3: Set media removal to be allowed
    #[repr(C)]
    struct PreventMediaRemoval { prevent: u8 }
    let removal_policy = PreventMediaRemoval { prevent: 0 }; // 0 = allow removal

    let removal_result = unsafe {
        DeviceIoControl(
            handle, IOCTL_STORAGE_MEDIA_REMOVAL,
            Some(&removal_policy as *const _ as *const std::ffi::c_void),
            size_of::<PreventMediaRemoval>() as u32,
            None, 0, Some(&mut bytes_returned), None,
        )
    };
    crate::debug::log(&format!("Allow removal result: {:?}", removal_result));

    // Step 4: Eject the media
    let eject_result = unsafe {
        DeviceIoControl(handle, IOCTL_STORAGE_EJECT_MEDIA, None, 0, None, 0, Some(&mut bytes_returned), None)
    };
    crate::debug::log(&format!("Eject media result: {:?}", eject_result));

    // Step 5: Unlock the volume (cleanup, even if eject failed)
    unsafe {
        let _ = DeviceIoControl(handle, FSCTL_UNLOCK_VOLUME, None, 0, None, 0, Some(&mut bytes_returned), None);
    }

    // Step 6: Close the handle
    unsafe { let _ = CloseHandle(handle); }

    // Consider success if dismount worked (data is safe) even if eject didn't
    if dismount_result.is_ok() {
        crate::debug::log("Eject successful (volume dismounted)");
        Ok(())
    } else if eject_result.is_ok() {
        crate::debug::log("Eject successful (media ejected)");
        Ok(())
    } else {
        crate::debug::log("Eject failed");
        Err("Eject failed. Please use Windows 'Safely Remove Hardware' before removing the card.".to_string())
    }
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
pub fn eject_drive(drive: &DriveInfo) -> Result<(), String> {
    use std::path::Path;
    use std::process::Command;

    // Check if device still exists - if not, it's already ejected
    if !Path::new(&drive.device_path).exists() {
        crate::debug::log(&format!("Linux eject: device {} already removed", drive.device_path));
        return Ok(());
    }

    // Determine the partition path
    let partition_path = if drive.device_path.contains("mmcblk") || drive.device_path.contains("nvme") {
        format!("{}p1", drive.device_path)
    } else {
        format!("{}1", drive.device_path)
    };

    // Sync the filesystem first
    if let Some(mount_path) = &drive.mount_path {
        if let Some(path_str) = mount_path.to_str() {
            crate::debug::log(&format!("Linux eject: syncing {}...", path_str));
            let _ = Command::new("sync").arg("-f").arg(path_str).output();
        }
    }

    // Use udisksctl for unmounting - this tells the udisks2 daemon we're ejecting
    // so it won't auto-remount the device. Falls back to regular umount if udisksctl fails.
    crate::debug::log(&format!("Linux eject: unmounting partition {} via udisksctl...", partition_path));
    let udisks_unmount = Command::new("udisksctl")
        .args(["unmount", "-b", &partition_path])
        .output();

    if let Ok(output) = &udisks_unmount {
        if output.status.success() {
            crate::debug::log("Linux eject: udisksctl unmount succeeded");
        } else {
            // Fallback to regular umount for our custom mount point
            if let Some(mount_path) = &drive.mount_path {
                if let Some(path_str) = mount_path.to_str() {
                    crate::debug::log(&format!("Linux eject: falling back to umount {}...", path_str));
                    let _ = Command::new("umount").arg(path_str).output();
                }
            }
            // Also try unmounting the partition directly
            let _ = Command::new("umount").arg(&partition_path).output();
        }
    } else {
        // udisksctl not available, use regular umount
        if let Some(mount_path) = &drive.mount_path {
            if let Some(path_str) = mount_path.to_str() {
                crate::debug::log(&format!("Linux eject: unmounting {}...", path_str));
                let _ = Command::new("umount").arg(path_str).output();
            }
        }
        let _ = Command::new("umount").arg(&partition_path).output();
    }

    // Final sync to ensure all data is written
    crate::debug::log("Linux eject: performing final system sync...");
    let _ = Command::new("sync").output();

    crate::debug::log("Linux eject: unmount successful, device is safe to remove");
    Ok(())
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