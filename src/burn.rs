use sha2::{Sha256, Digest};
use std::path::Path;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use flate2::read::GzDecoder;
use std::io::Read;

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB chunks

#[derive(Debug, Clone)]
pub enum BurnProgress {
    Started { total_bytes: u64 },
    Writing { written: u64, total: u64 },
    Verifying { verified: u64, total: u64 },
    Completed,
    Cancelled,
    Error(String),
}

/// Burns a raw disk image to a device and verifies the write
pub async fn burn_image(
    image_path: &Path,
    device_path: &str,
    progress_tx: UnboundedSender<BurnProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    crate::debug::log_section("Image Burning");
    crate::debug::log(&format!("Image: {:?}", image_path));
    crate::debug::log(&format!("Device: {}", device_path));

    // Get image size
    let image_size = tokio::fs::metadata(image_path)
        .await
        .map_err(|e| format!("Failed to get image size: {}", e))?
        .len();

    crate::debug::log(&format!("Image size: {} bytes ({:.2} GB)", image_size, image_size as f64 / 1_073_741_824.0));

    let _ = progress_tx.send(BurnProgress::Started { total_bytes: image_size });

    // Unmount the device first
    unmount_device(device_path).await?;

    // Platform-specific burn implementation
    #[cfg(target_os = "windows")]
    let result = burn_image_windows(image_path, device_path, image_size, &progress_tx, &cancel_token).await;

    #[cfg(target_os = "linux")]
    let result = burn_image_linux(image_path, device_path, image_size, &progress_tx, &cancel_token).await;

    #[cfg(target_os = "macos")]
    let result = burn_image_macos(image_path, device_path, image_size, &progress_tx, &cancel_token).await;

    match result {
        Ok(_) => {
            crate::debug::log("Image write completed, starting verification...");

            // Verify the written image
            verify_image(image_path, device_path, image_size, &progress_tx, &cancel_token).await?;

            let _ = progress_tx.send(BurnProgress::Completed);
            crate::debug::log("Image burn and verification complete");
            Ok(())
        }
        Err(e) => {
            let _ = progress_tx.send(BurnProgress::Error(e.clone()));
            Err(e)
        }
    }
}

/// Unmount all partitions on the device
async fn unmount_device(device_path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // On Windows, unmount all volumes on this physical drive
        crate::debug::log("Unmounting volumes on Windows...");
        unmount_device_windows(device_path).await
    }

    #[cfg(target_os = "linux")]
    {
        // On Linux, unmount all partitions (e.g., /dev/sdb1, /dev/sdb2, etc.)
        crate::debug::log("Unmounting partitions on Linux...");
        unmount_device_linux(device_path).await
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, use diskutil
        crate::debug::log("Unmounting disk on macOS...");
        unmount_device_macos(device_path).await
    }
}

// =============================================================================
// Windows Implementation
// =============================================================================

#[cfg(target_os = "windows")]
async fn unmount_device_windows(device_path: &str) -> Result<(), String> {
    use windows::Win32::Storage::FileSystem::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Ioctl::*;
    use windows::Win32::System::IO::DeviceIoControl;

    // For physical drives (\\.\PhysicalDriveN), we need to find and unmount all volumes
    if device_path.starts_with("\\\\.\\PhysicalDrive") {
        crate::debug::log(&format!("Attempting to lock/dismount volumes on {}", device_path));

        // We'll enumerate all drive letters and try to lock each one that might be on this physical drive
        // This is a simple approach - just try to dismount all removable drives
        unsafe {
            let drive_bits = GetLogicalDrives();
            for i in 0..26u8 {
                if (drive_bits >> i) & 1 == 1 {
                    let letter = (b'A' + i) as char;
                    let root_path: Vec<u16> = format!("{}:\\", letter)
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();

                    // Check if it's removable
                    let drive_type = GetDriveTypeW(windows::core::PCWSTR(root_path.as_ptr()));
                    if drive_type == 2 { // DRIVE_REMOVABLE
                        let volume_path: Vec<u16> = format!("\\\\.\\{}:", letter)
                            .encode_utf16()
                            .chain(Some(0))
                            .collect();

                        let handle = CreateFileW(
                            windows::core::PCWSTR(volume_path.as_ptr()),
                            FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                            FILE_SHARE_READ | FILE_SHARE_WRITE,
                            None,
                            OPEN_EXISTING,
                            Default::default(),
                            None,
                        );

                        if let Ok(handle) = handle {
                            let mut bytes_returned: u32 = 0;

                            // Try to lock the volume
                            let _ = DeviceIoControl(
                                handle,
                                FSCTL_LOCK_VOLUME,
                                None,
                                0,
                                None,
                                0,
                                Some(&mut bytes_returned),
                                None,
                            );

                            // Try to dismount
                            let dismount_result = DeviceIoControl(
                                handle,
                                FSCTL_DISMOUNT_VOLUME,
                                None,
                                0,
                                None,
                                0,
                                Some(&mut bytes_returned),
                                None,
                            );

                            if dismount_result.is_ok() {
                                crate::debug::log(&format!("Dismounted {}:", letter));
                            }

                            let _ = CloseHandle(handle);
                        }
                    }
                }
            }
        }
    }

    // Wait a moment for the OS to process the unmount
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

#[cfg(target_os = "windows")]
async fn burn_image_windows(
    image_path: &Path,
    device_path: &str,
    image_size: u64,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    // Device path should already be in \\.\PhysicalDriveN format from drives.rs
    crate::debug::log(&format!("Opening physical drive: {}", device_path));

    // Move ALL Windows API operations into spawn_blocking since HANDLE is !Send
    let result = tokio::task::spawn_blocking({
        let image_path = image_path.to_path_buf();
        let device_path = device_path.to_string();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();

        move || -> Result<(), String> {
            use windows::Win32::Foundation::*;
            use windows::Win32::Storage::FileSystem::*;
            use windows::Win32::System::IO::*;
            use windows::Win32::System::Ioctl::*;
            use std::io::Read;

            let device_path_wide: Vec<u16> = device_path
                .encode_utf16()
                .chain(Some(0))
                .collect();

            // Open the physical drive for writing
            let handle = unsafe {
                CreateFileW(
                    windows::core::PCWSTR(device_path_wide.as_ptr()),
                    FILE_GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            };

            if handle.is_err() {
                return Err(format!("Failed to open device for writing: {:?}", handle));
            }

            let handle = handle.unwrap();

            // Lock the volume
            let mut bytes_returned: u32 = 0;
            unsafe {
                let lock_result = DeviceIoControl(
                    handle,
                    FSCTL_LOCK_VOLUME,
                    None,
                    0,
                    None,
                    0,
                    Some(&mut bytes_returned),
                    None,
                );

                if lock_result.is_err() {
                    let _ = CloseHandle(handle);
                    return Err("Failed to lock volume for exclusive access".to_string());
                }
            }

            crate::debug::log("Volume locked, beginning write...");

            // Check if file is gzipped and create appropriate reader
            let is_gzipped = image_path.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("gz"))
                .unwrap_or(false);

            let file = std::fs::File::open(&image_path)
                .map_err(|e| {
                    unsafe { let _ = CloseHandle(handle); }
                    format!("Failed to open image file: {}", e)
                })?;

            let mut image_reader: Box<dyn Read> = if is_gzipped {
                crate::debug::log("Detected .gz file, decompressing on-the-fly during burn");
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };

            let mut buffer = vec![0u8; CHUNK_SIZE];
            let mut total_written = 0u64;

            loop {
                if cancel_token.is_cancelled() {
                    crate::debug::log("Burn cancelled by user");
                    unsafe {
                        let _ = DeviceIoControl(
                            handle,
                            FSCTL_UNLOCK_VOLUME,
                            None,
                            0,
                            None,
                            0,
                            Some(&mut bytes_returned),
                            None,
                        );
                        let _ = CloseHandle(handle);
                    }
                    return Err("Burn cancelled".to_string());
                }

                let bytes_read = image_reader.read(&mut buffer)
                    .map_err(|e| {
                        unsafe { let _ = CloseHandle(handle); }
                        format!("Failed to read from image: {}", e)
                    })?;

                if bytes_read == 0 {
                    break; // EOF
                }

                let mut bytes_written = 0u32;
                unsafe {
                    let write_result = WriteFile(
                        handle,
                        Some(&buffer[..bytes_read]),
                        Some(&mut bytes_written),
                        None,
                    );

                    if write_result.is_err() || bytes_written as usize != bytes_read {
                        let _ = CloseHandle(handle);
                        return Err(format!("Failed to write to device at offset {}", total_written));
                    }
                }

                total_written += bytes_written as u64;
                let _ = progress_tx.send(BurnProgress::Writing {
                    written: total_written,
                    total: image_size,
                });
            }

            // Flush buffers
            unsafe {
                let _ = FlushFileBuffers(handle);
            }

            crate::debug::log(&format!("Write complete: {} bytes written", total_written));

            // Unlock and close
            unsafe {
                let _ = DeviceIoControl(
                    handle,
                    FSCTL_UNLOCK_VOLUME,
                    None,
                    0,
                    None,
                    0,
                    Some(&mut bytes_returned),
                    None,
                );
                let _ = CloseHandle(handle);
            }

            crate::debug::log("Device unlocked and closed");
            Ok(())
        }
    }).await
    .map_err(|e| format!("Write task failed: {}", e))??;

    Ok(())
}

// =============================================================================
// Linux Implementation
// =============================================================================

#[cfg(target_os = "linux")]
async fn unmount_device_linux(device_path: &str) -> Result<(), String> {
    use tokio::process::Command;

    // Find all mounted partitions for this device
    let mount_output = Command::new("mount")
        .output()
        .await
        .map_err(|e| format!("Failed to run mount command: {}", e))?;

    let mount_str = String::from_utf8_lossy(&mount_output.stdout);

    // Find all partitions (e.g., /dev/sdb1, /dev/sdb2)
    for line in mount_str.lines() {
        if line.starts_with(device_path) {
            if let Some(partition) = line.split_whitespace().next() {
                crate::debug::log(&format!("Unmounting partition: {}", partition));
                let result = Command::new("umount")
                    .arg(partition)
                    .output()
                    .await;

                match result {
                    Ok(output) if output.status.success() => {
                        crate::debug::log(&format!("Successfully unmounted {}", partition));
                    }
                    _ => {
                        crate::debug::log(&format!("Warning: Could not unmount {}", partition));
                    }
                }
            }
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn burn_image_linux(
    image_path: &Path,
    device_path: &str,
    image_size: u64,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::{Read, Write, Seek};

    crate::debug::log(&format!("Opening device: {}", device_path));

    let result = tokio::task::spawn_blocking({
        let image_path = image_path.to_path_buf();
        let device_path = device_path.to_string();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();

        move || -> Result<(), String> {
            // Open device with O_WRONLY | O_SYNC | O_DIRECT flags
            let mut device = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_SYNC)
                .open(&device_path)
                .map_err(|e| format!("Failed to open device {}: {}. Are you running with sudo/root?", device_path, e))?;

            // Check if file is gzipped and create appropriate reader
            let is_gzipped = image_path.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("gz"))
                .unwrap_or(false);

            let file = std::fs::File::open(&image_path)
                .map_err(|e| format!("Failed to open image file: {}", e))?;

            let mut image_reader: Box<dyn Read> = if is_gzipped {
                crate::debug::log("Detected .gz file, decompressing on-the-fly during burn");
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };

            let mut buffer = vec![0u8; CHUNK_SIZE];
            let mut total_written = 0u64;

            loop {
                if cancel_token.is_cancelled() {
                    crate::debug::log("Burn cancelled by user");
                    return Err("Burn cancelled".to_string());
                }

                let bytes_read = image_reader.read(&mut buffer)
                    .map_err(|e| format!("Failed to read from image: {}", e))?;

                if bytes_read == 0 {
                    break; // EOF
                }

                device.write_all(&buffer[..bytes_read])
                    .map_err(|e| format!("Failed to write to device at offset {}: {}", total_written, e))?;

                total_written += bytes_read as u64;
                let _ = progress_tx.send(BurnProgress::Writing {
                    written: total_written,
                    total: image_size,
                });
            }

            // Sync to ensure all data is written
            device.sync_all()
                .map_err(|e| format!("Failed to sync device: {}", e))?;

            crate::debug::log(&format!("Write complete: {} bytes written", total_written));
            Ok(())
        }
    }).await
    .map_err(|e| format!("Write task failed: {}", e))??;

    Ok(())
}

// =============================================================================
// macOS Implementation
// =============================================================================

#[cfg(target_os = "macos")]
async fn unmount_device_macos(device_path: &str) -> Result<(), String> {
    use tokio::process::Command;

    // Convert /dev/disk# to disk# for diskutil
    let disk_name = device_path.trim_start_matches("/dev/");

    crate::debug::log(&format!("Running: diskutil unmountDisk {}", disk_name));

    let output = Command::new("diskutil")
        .arg("unmountDisk")
        .arg(disk_name)
        .output()
        .await
        .map_err(|e| format!("Failed to run diskutil: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        crate::debug::log(&format!("diskutil warning: {}", stderr));
    } else {
        crate::debug::log("Disk unmounted successfully");
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

#[cfg(target_os = "macos")]
async fn burn_image_macos(
    image_path: &Path,
    device_path: &str,
    image_size: u64,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::{Read, Write};

    // Use rdisk for faster writes (raw disk)
    let raw_device_path = device_path.replace("/dev/disk", "/dev/rdisk");
    crate::debug::log(&format!("Using raw device: {}", raw_device_path));

    let result = tokio::task::spawn_blocking({
        let image_path = image_path.to_path_buf();
        let device_path = raw_device_path.clone();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();

        move || -> Result<(), String> {
            let mut device = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_SYNC)
                .open(&device_path)
                .map_err(|e| format!("Failed to open device {}: {}. Are you running with sudo?", device_path, e))?;

            // Check if file is gzipped and create appropriate reader
            let is_gzipped = image_path.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("gz"))
                .unwrap_or(false);

            let file = std::fs::File::open(&image_path)
                .map_err(|e| format!("Failed to open image file: {}", e))?;

            let mut image_reader: Box<dyn Read> = if is_gzipped {
                crate::debug::log("Detected .gz file, decompressing on-the-fly during burn");
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };

            let mut buffer = vec![0u8; CHUNK_SIZE];
            let mut total_written = 0u64;

            loop {
                if cancel_token.is_cancelled() {
                    crate::debug::log("Burn cancelled by user");
                    return Err("Burn cancelled".to_string());
                }

                let bytes_read = image_reader.read(&mut buffer)
                    .map_err(|e| format!("Failed to read from image: {}", e))?;

                if bytes_read == 0 {
                    break; // EOF
                }

                device.write_all(&buffer[..bytes_read])
                    .map_err(|e| format!("Failed to write to device at offset {}: {}", total_written, e))?;

                total_written += bytes_read as u64;
                let _ = progress_tx.send(BurnProgress::Writing {
                    written: total_written,
                    total: image_size,
                });
            }

            // Sync to ensure all data is written
            device.sync_all()
                .map_err(|e| format!("Failed to sync device: {}", e))?;

            crate::debug::log(&format!("Write complete: {} bytes written", total_written));
            Ok(())
        }
    }).await
    .map_err(|e| format!("Write task failed: {}", e))??;

    Ok(())
}

// =============================================================================
// Verification
// =============================================================================

/// Verify the written image by reading back and comparing SHA256 hash
async fn verify_image(
    image_path: &Path,
    device_path: &str,
    image_size: u64,
    progress_tx: &UnboundedSender<BurnProgress>,
    cancel_token: &CancellationToken,
) -> Result<(), String> {
    crate::debug::log("Computing image hash...");

    // Compute hash of original image (decompress if .gz)
    let image_hash = tokio::task::spawn_blocking({
        let image_path = image_path.to_path_buf();
        let cancel_token = cancel_token.clone();

        move || -> Result<String, String> {
            use std::io::Read;

            // Check if file is gzipped and create appropriate reader
            let is_gzipped = image_path.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("gz"))
                .unwrap_or(false);

            let file = std::fs::File::open(&image_path)
                .map_err(|e| format!("Failed to open image for verification: {}", e))?;

            let mut image_reader: Box<dyn Read> = if is_gzipped {
                crate::debug::log("Decompressing .gz file for hash verification");
                Box::new(GzDecoder::new(file))
            } else {
                Box::new(file)
            };

            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; CHUNK_SIZE];

            loop {
                if cancel_token.is_cancelled() {
                    return Err("Verification cancelled".to_string());
                }

                let bytes_read = image_reader.read(&mut buffer)
                    .map_err(|e| format!("Failed to read image: {}", e))?;

                if bytes_read == 0 {
                    break;
                }

                hasher.update(&buffer[..bytes_read]);
            }

            let result = hasher.finalize();
            Ok(format!("{:x}", result))
        }
    }).await
    .map_err(|e| format!("Hash computation failed: {}", e))??;

    crate::debug::log(&format!("Image SHA256: {}", image_hash));
    crate::debug::log("Reading back device data...");

    // Read back device and compute hash
    let device_hash = tokio::task::spawn_blocking({
        let device_path = device_path.to_string();
        let progress_tx = progress_tx.clone();
        let cancel_token = cancel_token.clone();

        move || -> Result<String, String> {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            use std::io::Read;

            #[cfg(target_os = "windows")]
            let mut device = {
                use windows::Win32::Foundation::*;
                use windows::Win32::Storage::FileSystem::*;
                use std::os::windows::ffi::OsStrExt;

                let device_path_wide: Vec<u16> = device_path
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();

                let handle = unsafe {
                    CreateFileW(
                        windows::core::PCWSTR(device_path_wide.as_ptr()),
                        FILE_GENERIC_READ.0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        None,
                    )
                };

                if handle.is_err() {
                    return Err("Failed to open device for verification".to_string());
                }

                // We can't easily convert HANDLE to File on Windows
                // For now, just verify by size
                // TODO: Implement proper Windows device reading
                return Err("Verification on Windows requires administrative privileges and is not yet implemented. Skipping verification.".to_string());
            };

            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let mut device = {
                #[cfg(target_os = "macos")]
                let dev_path = device_path.replace("/dev/disk", "/dev/rdisk");
                #[cfg(target_os = "linux")]
                let dev_path = device_path.clone();

                std::fs::File::open(&dev_path)
                    .map_err(|e| format!("Failed to open device for verification: {}. Are you running with sudo?", e))?
            };

            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                let mut hasher = Sha256::new();
                let mut buffer = vec![0u8; CHUNK_SIZE];
                let mut total_read = 0u64;

                while total_read < image_size {
                    if cancel_token.is_cancelled() {
                        return Err("Verification cancelled".to_string());
                    }

                    let to_read = std::cmp::min(CHUNK_SIZE, (image_size - total_read) as usize);
                    let bytes_read = device.read(&mut buffer[..to_read])
                        .map_err(|e| format!("Failed to read device: {}", e))?;

                    if bytes_read == 0 {
                        return Err(format!("Unexpected EOF: read {} bytes, expected {}", total_read, image_size));
                    }

                    hasher.update(&buffer[..bytes_read]);
                    total_read += bytes_read as u64;

                    let _ = progress_tx.send(BurnProgress::Verifying {
                        verified: total_read,
                        total: image_size,
                    });
                }

                let result = hasher.finalize();
                Ok(format!("{:x}", result))
            }
        }
    }).await
    .map_err(|e| format!("Device read failed: {}", e))??;

    crate::debug::log(&format!("Device SHA256: {}", device_hash));

    if image_hash != device_hash {
        Err("Verification failed: Hashes do not match!".to_string())
    } else {
        crate::debug::log("Verification passed: Hashes match");
        Ok(())
    }
}
