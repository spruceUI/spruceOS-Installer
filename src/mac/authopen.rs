/// macOS-specific authorization and file opening using authopen
/// Based on Raspberry Pi Imager's implementation
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::{FromRawFd, RawFd};
use std::path::Path;
use std::process::{Command, Stdio};

const AUTHOPEN_PATH: &str = "/usr/libexec/authopen";

/// Opens a device file with elevated privileges using macOS authopen utility
/// Returns a File handle that can be used for reading/writing to the device
pub fn auth_open_device(device_path: &Path) -> Result<File, String> {
    // Verify authopen exists
    if !std::path::Path::new(AUTHOPEN_PATH).exists() {
        return Err(format!("authopen utility not found at {}", AUTHOPEN_PATH));
    }

    crate::debug::log("Using authopen for privileged device access");
    crate::debug::log(&format!("Opening device: {:?}", device_path));

    // Prepare authopen command
    // authopen -stdoutpipe -o <flags> <path>
    // -stdoutpipe: Write the file descriptor number to stdout
    // -o: Open with flags (O_RDWR = 0x0002, O_SYNC = 0x0080, O_EXLOCK = 0x0020)
    let device_str = device_path.to_string_lossy();

    // O_RDWR | O_SYNC = 0x0002 | 0x0080 = 0x0082
    let flags = 0x0082;

    let mut child = Command::new(AUTHOPEN_PATH)
        .arg("-stdoutpipe")
        .arg("-o")
        .arg(format!("{:#x}", flags))
        .arg(&*device_str)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("AUTHOPEN_SPAWN_FAILED: {}", e))?;

    crate::debug::log("authopen process spawned, waiting for file descriptor...");

    // Read FD number from stdout
    let mut stdout = child.stdout.take().ok_or("No stdout")?;
    let mut fd_str = String::new();
    stdout.read_to_string(&mut fd_str)
        .map_err(|e| format!("Failed to read FD from authopen: {}", e))?;

    // Wait for authopen to exit
    let output = child.wait_with_output()
        .map_err(|e| format!("Failed to wait for authopen: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout_output = String::from_utf8_lossy(&output.stdout);
        crate::debug::log(&format!("authopen failed - status: {:?}", output.status));
        crate::debug::log(&format!("authopen stdout: {}", stdout_output.trim()));
        crate::debug::log(&format!("authopen stderr: {}", stderr.trim()));
        return Err(format!("AUTHOPEN_AUTHORIZATION_DENIED: {}", stderr.trim()));
    }

    // Parse FD number
    let fd: RawFd = fd_str.trim().parse()
        .map_err(|e| format!("Failed to parse FD number '{}': {}", fd_str.trim(), e))?;

    crate::debug::log(&format!("Received file descriptor: {}", fd));

    // Duplicate the FD so we own it
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(format!("Failed to dup FD: {}", io::Error::last_os_error()));
    }

    // Close the original FD from authopen
    unsafe { libc::close(fd) };

    // Convert duplicated FD to File
    let file = unsafe { File::from_raw_fd(dup_fd) };

    crate::debug::log("Device opened successfully via authopen");
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Only run manually as it requires user interaction
    fn test_authopen() {
        let result = auth_open_device(Path::new("/dev/disk2"));
        match result {
            Ok(_file) => println!("Successfully opened device"),
            Err(e) => println!("Failed to open device: {}", e),
        }
    }
}
