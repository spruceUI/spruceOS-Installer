// Custom FAT32 formatter that works for drives > 32GB
// Windows artificially limits FAT32 to 32GB, but the filesystem supports up to 2TB
// This writes directly to the physical disk to avoid issues with unmounted volumes

use tokio::sync::mpsc;

use crate::format::FormatProgress;

#[cfg(windows)]
const SECTOR_SIZE: u32 = 512;
#[cfg(windows)]
const RESERVED_SECTORS: u16 = 32;
#[cfg(windows)]
const NUM_FATS: u8 = 2;
#[cfg(windows)]
const PARTITION_START_SECTOR: u64 = 2048; // Standard 1MB alignment

#[cfg(windows)]
#[derive(Debug)]
struct Fat32Params {
    sectors_per_cluster: u8,
    total_sectors: u64,
    fat_size_sectors: u32,
    root_cluster: u32,
}

#[cfg(windows)]
fn calculate_params(total_bytes: u64) -> Fat32Params {
    let total_sectors = total_bytes / SECTOR_SIZE as u64;

    // Choose cluster size based on volume size (Microsoft recommendations)
    let sectors_per_cluster: u8 = if total_bytes <= 64 * 1024 * 1024 {
        1 // 512 bytes - up to 64MB
    } else if total_bytes <= 128 * 1024 * 1024 {
        2 // 1KB - up to 128MB
    } else if total_bytes <= 256 * 1024 * 1024 {
        4 // 2KB - up to 256MB
    } else if total_bytes <= 8u64 * 1024 * 1024 * 1024 {
        8 // 4KB - up to 8GB
    } else if total_bytes <= 16u64 * 1024 * 1024 * 1024 {
        16 // 8KB - up to 16GB
    } else if total_bytes <= 32u64 * 1024 * 1024 * 1024 {
        32 // 16KB - up to 32GB
    } else {
        64 // 32KB - above 32GB
    };

    // Calculate FAT size
    let data_sectors = total_sectors.saturating_sub(RESERVED_SECTORS as u64);
    let cluster_count = data_sectors / sectors_per_cluster as u64;
    let fat_size_sectors = ((cluster_count + 2) * 4 + SECTOR_SIZE as u64 - 1) / SECTOR_SIZE as u64;

    Fat32Params {
        sectors_per_cluster,
        total_sectors,
        fat_size_sectors: fat_size_sectors as u32,
        root_cluster: 2,
    }
}

#[cfg(windows)]
fn create_boot_sector(params: &Fat32Params, volume_label: &str) -> [u8; 512] {
    let mut boot = [0u8; 512];

    // Jump instruction
    boot[0] = 0xEB;
    boot[1] = 0x58;
    boot[2] = 0x90;

    // OEM name
    boot[3..11].copy_from_slice(b"MSWIN4.1");

    // Bytes per sector
    boot[11] = (SECTOR_SIZE & 0xFF) as u8;
    boot[12] = ((SECTOR_SIZE >> 8) & 0xFF) as u8;

    // Sectors per cluster
    boot[13] = params.sectors_per_cluster;

    // Reserved sectors
    boot[14] = (RESERVED_SECTORS & 0xFF) as u8;
    boot[15] = ((RESERVED_SECTORS >> 8) & 0xFF) as u8;

    // Number of FATs
    boot[16] = NUM_FATS;

    // Root entry count (0 for FAT32)
    boot[17] = 0;
    boot[18] = 0;

    // Total sectors 16-bit (0 for FAT32)
    boot[19] = 0;
    boot[20] = 0;

    // Media type (F8 = fixed disk)
    boot[21] = 0xF8;

    // FAT size 16-bit (0 for FAT32)
    boot[22] = 0;
    boot[23] = 0;

    // Sectors per track
    boot[24] = 63;
    boot[25] = 0;

    // Number of heads
    boot[26] = 255;
    boot[27] = 0;

    // Hidden sectors (sectors before partition = partition start)
    boot[28..32].copy_from_slice(&(PARTITION_START_SECTOR as u32).to_le_bytes());

    // Total sectors 32-bit
    let total_32 = if params.total_sectors > u32::MAX as u64 {
        u32::MAX
    } else {
        params.total_sectors as u32
    };
    boot[32..36].copy_from_slice(&total_32.to_le_bytes());

    // FAT32 specific fields
    // FAT size 32-bit
    boot[36..40].copy_from_slice(&params.fat_size_sectors.to_le_bytes());

    // Ext flags
    boot[40] = 0;
    boot[41] = 0;

    // FS version
    boot[42] = 0;
    boot[43] = 0;

    // Root cluster
    boot[44..48].copy_from_slice(&params.root_cluster.to_le_bytes());

    // FSInfo sector
    boot[48] = 1;
    boot[49] = 0;

    // Backup boot sector
    boot[50] = 6;
    boot[51] = 0;

    // Drive number
    boot[64] = 0x80;

    // Reserved
    boot[65] = 0;

    // Extended boot signature
    boot[66] = 0x29;

    // Volume serial number
    let serial: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0x12345678);
    boot[67..71].copy_from_slice(&serial.to_le_bytes());

    // Volume label (11 bytes, space-padded)
    let mut label_bytes = [0x20u8; 11];
    let label_src = volume_label.as_bytes();
    let copy_len = label_src.len().min(11);
    label_bytes[..copy_len].copy_from_slice(&label_src[..copy_len]);
    boot[71..82].copy_from_slice(&label_bytes);

    // File system type
    boot[82..90].copy_from_slice(b"FAT32   ");

    // Boot signature
    boot[510] = 0x55;
    boot[511] = 0xAA;

    boot
}

#[cfg(windows)]
fn create_fsinfo_sector() -> [u8; 512] {
    let mut fsinfo = [0u8; 512];

    // FSInfo signature
    fsinfo[0..4].copy_from_slice(&0x41615252u32.to_le_bytes());

    // Second signature
    fsinfo[484..488].copy_from_slice(&0x61417272u32.to_le_bytes());

    // Free cluster count (unknown)
    fsinfo[488..492].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());

    // Next free cluster (unknown)
    fsinfo[492..496].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());

    // Trail signature
    fsinfo[508..512].copy_from_slice(&0xAA550000u32.to_le_bytes());

    fsinfo
}

#[cfg(windows)]
fn create_fat_sector_with_entries() -> [u8; 512] {
    let mut fat = [0u8; 512];

    // Entry 0: Media type
    fat[0..4].copy_from_slice(&0x0FFFFFF8u32.to_le_bytes());

    // Entry 1: End of chain marker
    fat[4..8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

    // Entry 2: End of chain for root directory
    fat[8..12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

    fat
}

/// Format using physical disk access (works even when volume isn't mounted)
#[cfg(windows)]
pub async fn format_fat32_large(
    disk_number: u32,
    volume_label: &str,
    total_bytes: u64,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
) -> Result<(), String> {
    use windows::Win32::Foundation::{HANDLE, CloseHandle, GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, SetFilePointerEx, WriteFile, FILE_BEGIN,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        FILE_FLAG_NO_BUFFERING, FILE_FLAG_WRITE_THROUGH,
    };
    use windows::core::PCWSTR;

    let _ = progress_tx.send(FormatProgress::Formatting);

    // Calculate partition size (total disk minus the 1MB alignment at start)
    let partition_size = total_bytes.saturating_sub(PARTITION_START_SECTOR * SECTOR_SIZE as u64);
    let params = calculate_params(partition_size);

    // Open the physical disk for raw access with proper flags
    let disk_path: Vec<u16> = format!("\\\\.\\PhysicalDrive{}", disk_number)
        .encode_utf16()
        .chain(Some(0))
        .collect();

    let handle = unsafe {
        CreateFileW(
            PCWSTR(disk_path.as_ptr()),
            (GENERIC_READ.0 | GENERIC_WRITE.0).into(),
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH,
            None,
        )
    }.map_err(|e| format!("Failed to open disk {}: {}", disk_number, e))?;

    // Helper to write a sector at a specific position
    let write_sector = |h: HANDLE, offset: u64, data: &[u8; 512]| -> Result<(), String> {
        let mut new_pos = 0i64;
        unsafe {
            SetFilePointerEx(h, offset as i64, Some(&mut new_pos), FILE_BEGIN)
                .map_err(|e| format!("Seek failed: {}", e))?;

            let mut written = 0u32;
            WriteFile(h, Some(data), Some(&mut written), None)
                .map_err(|e| format!("Write failed: {}", e))?;

            if written != 512 {
                return Err(format!("Incomplete write: {} bytes", written));
            }
        }
        Ok(())
    };

    // Calculate the byte offset where the partition starts
    let partition_offset = PARTITION_START_SECTOR * SECTOR_SIZE as u64;

    // Write boot sector at partition start
    let boot_sector = create_boot_sector(&params, volume_label);
    write_sector(handle, partition_offset, &boot_sector)
        .map_err(|e| format!("Failed to write boot sector: {}", e))?;

    // Write FSInfo sector
    let fsinfo = create_fsinfo_sector();
    write_sector(handle, partition_offset + SECTOR_SIZE as u64, &fsinfo)
        .map_err(|e| format!("Failed to write FSInfo: {}", e))?;

    // Write backup boot sector at sector 6
    write_sector(handle, partition_offset + 6 * SECTOR_SIZE as u64, &boot_sector)
        .map_err(|e| format!("Failed to write backup boot sector: {}", e))?;

    // Write backup FSInfo at sector 7
    write_sector(handle, partition_offset + 7 * SECTOR_SIZE as u64, &fsinfo)
        .map_err(|e| format!("Failed to write backup FSInfo: {}", e))?;

    // Write FAT tables
    let fat_start = partition_offset + RESERVED_SECTORS as u64 * SECTOR_SIZE as u64;
    let fat_first_sector = create_fat_sector_with_entries();
    let zero_sector = [0u8; 512];

    // First FAT - first sector with entries
    write_sector(handle, fat_start, &fat_first_sector)
        .map_err(|e| format!("Failed to write FAT1: {}", e))?;

    // Clear a few more sectors of FAT1
    for i in 1..16.min(params.fat_size_sectors) {
        write_sector(handle, fat_start + i as u64 * SECTOR_SIZE as u64, &zero_sector)
            .map_err(|e| format!("Failed to clear FAT1: {}", e))?;
    }

    // Second FAT
    let fat2_start = fat_start + params.fat_size_sectors as u64 * SECTOR_SIZE as u64;
    write_sector(handle, fat2_start, &fat_first_sector)
        .map_err(|e| format!("Failed to write FAT2: {}", e))?;

    // Clear a few more sectors of FAT2
    for i in 1..16.min(params.fat_size_sectors) {
        write_sector(handle, fat2_start + i as u64 * SECTOR_SIZE as u64, &zero_sector)
            .map_err(|e| format!("Failed to clear FAT2: {}", e))?;
    }

    // Write root directory cluster
    let data_start = fat_start + (NUM_FATS as u64 * params.fat_size_sectors as u64 * SECTOR_SIZE as u64);

    // Create root directory sector with volume label
    let mut root_sector = [0u8; 512];
    let mut label_bytes = [0x20u8; 11];
    let label_src = volume_label.as_bytes();
    let copy_len = label_src.len().min(11);
    label_bytes[..copy_len].copy_from_slice(&label_src[..copy_len]);
    root_sector[0..11].copy_from_slice(&label_bytes);
    root_sector[11] = 0x08; // Volume label attribute

    write_sector(handle, data_start, &root_sector)
        .map_err(|e| format!("Failed to write root directory: {}", e))?;

    // Clear rest of root directory cluster
    let sectors_per_cluster = params.sectors_per_cluster as u32;
    for i in 1..sectors_per_cluster {
        write_sector(handle, data_start + i as u64 * SECTOR_SIZE as u64, &zero_sector)
            .map_err(|e| format!("Failed to clear root directory: {}", e))?;
    }

    // Close handle
    unsafe { let _ = CloseHandle(handle); }

    let _ = progress_tx.send(FormatProgress::Completed);
    Ok(())
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub async fn format_fat32_large(
    _disk_number: u32,
    _volume_label: &str,
    _total_bytes: u64,
    progress_tx: mpsc::UnboundedSender<FormatProgress>,
) -> Result<(), String> {
    let _ = progress_tx.send(FormatProgress::Formatting);
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    let _ = progress_tx.send(FormatProgress::Completed);
    eprintln!("Warning: FAT32 format simulation - not running on Windows.");
    Ok(())
}
