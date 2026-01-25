/// macOS-specific direct device opening (when already running as root)
/// This bypasses authopen and opens the device directly with proper flags
use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::Path;

/// Opens a device file directly with elevated privileges
/// Assumes the process is already running as root (via osascript relaunch)
pub fn direct_open_device(device_path: &Path) -> Result<File, String> {
    crate::debug::log("Opening device directly (running as root)...");
    crate::debug::log(&format!("Device path: {:?}", device_path));

    // Check if running as root
    let euid = unsafe { libc::geteuid() };
    if euid != 0 {
        return Err(format!("Not running as root (euid={}). Cannot open device directly.", euid));
    }

    // O_RDWR | O_SYNC = 0x0002 | 0x0080 = 0x0082
    let flags = libc::O_RDWR | libc::O_SYNC;

    // Open device with standard OpenOptions
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(flags)
        .open(device_path)
        .map_err(|e| format!("Failed to open device {:?}: {}", device_path, e))?;

    crate::debug::log("Device opened successfully");

    // Set F_NOCACHE to bypass kernel buffer cache
    let fd = file.as_raw_fd();
    unsafe {
        if libc::fcntl(fd, libc::F_NOCACHE, 1) == -1 {
            crate::debug::log("Warning: Failed to set F_NOCACHE, writes may be buffered (slower)");
        } else {
            crate::debug::log("F_NOCACHE enabled - writing directly to hardware");
        }
    }

    Ok(file)
}
