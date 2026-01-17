#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub letter: char,
    pub path: String,
    pub label: String,
    pub size_bytes: u64,
    pub free_bytes: u64,
}

impl DriveInfo {
    pub fn display_name(&self) -> String {
        let size_gb = self.size_bytes as f64 / 1_073_741_824.0;
        if self.label.is_empty() {
            format!("{}: ({:.1} GB)", self.letter, size_gb)
        } else {
            format!("{}: {} ({:.1} GB)", self.letter, self.label, size_gb)
        }
    }
}

#[cfg(windows)]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    const DRIVE_REMOVABLE: u32 = 2;

    let mut drives = Vec::new();
    let drive_bits = unsafe { GetLogicalDrives() };

    for i in 0..26 {
        if (drive_bits >> i) & 1 == 1 {
            let letter = (b'A' + i) as char;
            let root_path: Vec<u16> = format!("{}:\\", letter).encode_utf16().chain(Some(0)).collect();

            let drive_type = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_path.as_ptr())) };

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

                let mut free_bytes_available = 0u64;
                let mut total_bytes = 0u64;
                let mut total_free_bytes = 0u64;

                unsafe {
                    let _ = GetDiskFreeSpaceExW(
                        windows::core::PCWSTR(root_path.as_ptr()),
                        Some(&mut free_bytes_available),
                        Some(&mut total_bytes),
                        Some(&mut total_free_bytes),
                    );
                }

                drives.push(DriveInfo {
                    letter,
                    path: format!("{}:", letter),
                    label,
                    size_bytes: total_bytes,
                    free_bytes: total_free_bytes,
                });
            }
        }
    }

    drives
}

#[cfg(not(windows))]
pub fn get_removable_drives() -> Vec<DriveInfo> {
    // Stub for non-Windows platforms (for development/testing)
    vec![
        DriveInfo {
            letter: 'E',
            path: "E:".to_string(),
            label: "TEST_DRIVE".to_string(),
            size_bytes: 32_000_000_000,
            free_bytes: 31_000_000_000,
        },
    ]
}
