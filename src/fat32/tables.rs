// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

pub(crate) const SECTOR_SIZE: u32 = 512;

pub(crate) fn initialize_fat_tables(
    fat_start: u64,
    fat_size_sectors: u32,
    num_fats: u8,
    mut write: impl FnMut(u64, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    // Keep writes bounded and aligned for the Windows unbuffered disk handle.
    #[repr(align(4096))]
    struct FatBuffer([u8; 64 * 1024]);
    let mut buffer = FatBuffer([0u8; 64 * 1024]);
    let table_bytes = fat_size_sectors as u64 * SECTOR_SIZE as u64;

    for fat in 0..num_fats {
        let start = fat_start + fat as u64 * table_bytes;
        // Reserve the media descriptor, end-of-chain marker and root directory.
        buffer.0[0..4].copy_from_slice(&0x0FFFFFF8u32.to_le_bytes());
        buffer.0[4..8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
        buffer.0[8..12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
        let mut offset = 0;
        while offset < table_bytes {
            let length = (table_bytes - offset).min(buffer.0.len() as u64) as usize;
            write(start + offset, &buffer.0[..length])
                .map_err(|error| format!("Failed to initialize FAT{}: {}", fat + 1, error))?;
            buffer.0[..12].fill(0);
            offset += length as u64;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_both_complete_tables_without_touching_surrounding_data() {
        // Include the old 16-sector boundary and a FAT sized for a 256 GiB card.
        for sectors in [1, 16, 17, 127, 128, 129, 65_536] {
            let start = 4096;
            let table_bytes = sectors as usize * SECTOR_SIZE as usize;
            let end = start + 2 * table_bytes;
            let mut disk = vec![0xA5; end + 512];
            initialize_fat_tables(start as u64, sectors, 2, |offset, bytes| {
                assert_eq!(offset % SECTOR_SIZE as u64, 0);
                assert_eq!(bytes.as_ptr() as usize % 4096, 0);
                assert_eq!(bytes.len() % SECTOR_SIZE as usize, 0);
                assert!(bytes.len() <= 64 * 1024);
                let offset = offset as usize;
                disk[offset..offset + bytes.len()].copy_from_slice(bytes);
                Ok(())
            })
            .unwrap();

            assert!(disk[..start].iter().all(|&byte| byte == 0xA5));
            assert!(disk[end..].iter().all(|&byte| byte == 0xA5));
            for fat in 0..2 {
                let table = &disk[start + fat * table_bytes..start + (fat + 1) * table_bytes];
                assert_eq!(
                    &table[..12],
                    &[0xF8, 0xFF, 0xFF, 0x0F, 0xFF, 0xFF, 0xFF, 0x0F, 0xFF, 0xFF, 0xFF, 0x0F]
                );
                assert!(
                    table[12..].iter().all(|&byte| byte == 0),
                    "stale allocation entries in FAT {} ({sectors} sectors)",
                    fat + 1
                );
            }
        }
    }

    #[test]
    fn stops_on_write_failure_in_either_table() {
        // Each 129-sector FAT needs a full chunk and a one-sector tail.
        for fail_at in 0..4 {
            let mut calls = 0;
            let error = initialize_fat_tables(4096, 129, 2, |_, _| {
                let call = calls;
                calls += 1;
                if call == fail_at {
                    Err("injected write failure".to_string())
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert_eq!(calls, fail_at + 1);
            assert_eq!(
                error,
                format!(
                    "Failed to initialize FAT{}: injected write failure",
                    fail_at / 2 + 1
                )
            );
        }
    }
}
