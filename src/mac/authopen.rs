/// macOS-specific authorization and file opening using authopen
/// Based on Raspberry Pi Imager's implementation
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::{FromRawFd, RawFd};
use std::path::Path;
use std::process::{Command, Stdio};

const AUTHOPEN_PATH: &str = "/usr/libexec/authopen";

/// Error types for authopen operations
#[derive(Debug)]
pub enum AuthOpenError {
    /// User cancelled the authorization dialog
    Cancelled,
    /// Authorization or file opening failed
    Failed(String),
    /// System error (authopen not found, spawn failed, etc.)
    SystemError(String),
}

impl std::fmt::Display for AuthOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthOpenError::Cancelled => write!(f, "Authorization cancelled by user"),
            AuthOpenError::Failed(msg) => write!(f, "Authorization failed: {}", msg),
            AuthOpenError::SystemError(msg) => write!(f, "System error: {}", msg),
        }
    }
}

pub type AuthOpenResult = Result<File, AuthOpenError>;

/// Opens a device file with elevated privileges using macOS authopen utility
/// Returns a File handle that can be used for reading/writing to the device
pub fn auth_open_device(device_path: &Path) -> AuthOpenResult {
    // Verify authopen exists
    if !std::path::Path::new(AUTHOPEN_PATH).exists() {
        return Err(AuthOpenError::SystemError(
            format!("authopen utility not found at {}", AUTHOPEN_PATH)
        ));
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
        .map_err(|e| AuthOpenError::SystemError(format!("Failed to spawn authopen: {}", e)))?;

    crate::debug::log("authopen process spawned, waiting for file descriptor...");

    // Read FD number from stdout
    let mut stdout = child.stdout.take().ok_or_else(||
        AuthOpenError::SystemError("Failed to capture authopen stdout".to_string())
    )?;
    let mut fd_str = String::new();
    stdout.read_to_string(&mut fd_str)
        .map_err(|e| AuthOpenError::SystemError(
            format!("Failed to read FD from authopen: {}", e)
        ))?;

    // Wait for authopen to exit
    let output = child.wait_with_output()
        .map_err(|e| AuthOpenError::SystemError(
            format!("Failed to wait for authopen: {}", e)
        ))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout_output = String::from_utf8_lossy(&output.stdout);
        crate::debug::log(&format!("authopen failed - status: {:?}", output.status));
        crate::debug::log(&format!("authopen stdout: {}", stdout_output.trim()));
        crate::debug::log(&format!("authopen stderr: {}", stderr.trim()));

        // Detect user cancellation
        let stderr_lower = stderr.to_lowercase();
        if stderr_lower.contains("cancel") ||
           stderr_lower.contains("user cancel") ||
           stderr_lower.contains("authentication cancelled") {
            crate::debug::log("User cancelled authorization");
            return Err(AuthOpenError::Cancelled);
        }

        // Check for common error patterns
        if stderr_lower.contains("permission denied") ||
           stderr_lower.contains("not permitted") ||
           stderr_lower.contains("operation not permitted") {
            return Err(AuthOpenError::Failed(format!(
                "Permission denied - make sure you have admin rights: {}",
                stderr.trim()
            )));
        }

        // Generic authorization failure
        return Err(AuthOpenError::Failed(stderr.trim().to_string()));
    }

    // Parse FD number
    let fd: RawFd = fd_str.trim().parse()
        .map_err(|e| AuthOpenError::SystemError(
            format!("Failed to parse FD number '{}': {}", fd_str.trim(), e)
        ))?;

    crate::debug::log(&format!("Received file descriptor: {}", fd));

    // Validate FD is a valid file descriptor
    if fd < 0 {
        return Err(AuthOpenError::SystemError(
            format!("Invalid file descriptor: {}", fd)
        ));
    }

    // Duplicate the FD so we own it
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(AuthOpenError::SystemError(
            format!("Failed to dup FD: {}", io::Error::last_os_error())
        ));
    }

    // Close the original FD from authopen
    unsafe { libc::close(fd) };

    // Convert duplicated FD to File
    let file = unsafe { File::from_raw_fd(dup_fd) };

    crate::debug::log("Device opened successfully via authopen");

    // Set F_NOCACHE to bypass kernel buffer cache and write directly to hardware
    // This prevents the "99% stall" issue where data sits in RAM cache waiting to flush
    use std::os::unix::io::AsRawFd;
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
