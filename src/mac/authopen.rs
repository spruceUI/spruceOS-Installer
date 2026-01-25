/// macOS-specific authorization and file opening using authopen
/// Based on Raspberry Pi Imager's implementation
use std::fs::File;
use std::io::{self, Read};
use std::mem;
use std::os::unix::io::{FromRawFd, RawFd};
use std::path::Path;
use std::process::{Command, Stdio};

const AUTHOPEN_PATH: &str = "/usr/libexec/authopen";

fn recv_fd_from_authopen(sock_fd: RawFd) -> io::Result<RawFd> {
    // Space for a single fd; 64 bytes is plenty for CMSG header + fd on 64-bit.
    let mut cmsgspace = [0u8; 64];
    let mut buf = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut _,
        iov_len: buf.len(),
    };

    let mut msg: libc::msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsgspace.as_mut_ptr() as *mut _;
    msg.msg_controllen = cmsgspace.len() as _;

    let n = unsafe { libc::recvmsg(sock_fd, &mut msg, 0) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "authopen closed socket without sending fd",
        ));
    }

    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !cmsg.is_null() {
        unsafe {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(cmsg) as *const RawFd;
                return Ok(*data);
            }
        }
        cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "no file descriptor received from authopen",
    ))
}

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
    // -o: Open with flags
    let device_str = device_path.to_string_lossy();

    // O_RDWR | O_SYNC | O_EXLOCK = 0x0002 | 0x0080 | 0x0020 = 0x00A2
    // O_EXLOCK provides exclusive lock to prevent macOS from interfering
    let flags = 0x00A2;

    let mut fds = [0; 2];
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(AuthOpenError::SystemError(format!(
            "Failed to create socketpair: {}",
            io::Error::last_os_error()
        )));
    }

    let parent_fd = fds[0];
    let child_fd = fds[1];

    let mut child = Command::new(AUTHOPEN_PATH)
        .arg("-stdoutpipe")
        .arg("-o")
        .arg(format!("{:#x}", flags))
        .arg(&*device_str)
        .stdin(Stdio::null())
        .stdout(unsafe { Stdio::from_raw_fd(child_fd) })
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AuthOpenError::SystemError(format!("Failed to spawn authopen: {}", e)))?;

    // Parent no longer needs the child side of the socketpair.
    unsafe { libc::close(child_fd) };

    crate::debug::log("authopen process spawned, waiting for file descriptor...");

    let received_fd = recv_fd_from_authopen(parent_fd);

    let mut stderr = String::new();
    if let Some(mut stderr_pipe) = child.stderr.take() {
        let _ = stderr_pipe.read_to_string(&mut stderr);
    }

    let status = child.wait()
        .map_err(|e| AuthOpenError::SystemError(
            format!("Failed to wait for authopen: {}", e)
        ))?;

    unsafe { libc::close(parent_fd) };

    if !status.success() {
        if let Ok(fd) = received_fd {
            unsafe { libc::close(fd) };
        }
        crate::debug::log(&format!("authopen failed - status: {:?}", status));
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

    let fd = match received_fd {
        Ok(fd) => fd,
        Err(e) => {
            return Err(AuthOpenError::SystemError(format!(
                "Failed to receive authopen file descriptor: {}",
                e
            )));
        }
    };

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
    Ok(file)
}

/// Burns an image to a device using osascript with administrator privileges.
/// This is more reliable on modern macOS as it uses the native authentication dialog.
///
/// Returns Ok(bytes_written) on success.
pub fn burn_with_privileges(
    image_path: &Path,
    device_path: &str,
    progress_callback: impl Fn(u64, u64) + Send + 'static,
) -> Result<u64, String> {

    let image_str = image_path.to_string_lossy();
    let is_gzipped = image_path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("gz"))
        .unwrap_or(false);

    // Get the decompressed size for progress tracking
    let total_size = if is_gzipped {
        // For gzipped files, we need to estimate or pre-scan
        // For now, use compressed size * 2 as rough estimate
        std::fs::metadata(image_path)
            .map(|m| m.len() * 2)
            .unwrap_or(0)
    } else {
        std::fs::metadata(image_path)
            .map(|m| m.len())
            .unwrap_or(0)
    };

    crate::debug::log(&format!("Starting privileged burn: {} -> {}", image_str, device_path));

    // Extract disk identifier (e.g., "disk6" from "/dev/rdisk6")
    let disk_id = device_path
        .trim_start_matches("/dev/")
        .trim_start_matches("r");

    // First, try to forcefully unmount and erase the disk using diskutil
    // This often helps with permission issues
    crate::debug::log(&format!("Force unmounting {} via diskutil...", disk_id));
    let _ = Command::new("diskutil")
        .args(["unmountDisk", "force", disk_id])
        .output();

    // Build the dd command
    let dd_command = if is_gzipped {
        format!(
            "gunzip -c '{}' | dd of='{}' bs=4m",
            image_str.replace("'", "'\\''"),
            device_path.replace("'", "'\\''")
        )
    } else {
        format!(
            "dd if='{}' of='{}' bs=4m",
            image_str.replace("'", "'\\''"),
            device_path.replace("'", "'\\''")
        )
    };

    // Use osascript to run with admin privileges
    // This shows a native macOS authentication dialog
    let script = format!(
        r#"do shell script "{}" with administrator privileges"#,
        dd_command.replace("\\", "\\\\").replace("\"", "\\\"")
    );

    crate::debug::log("Requesting administrator privileges via osascript...");

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("Failed to execute osascript: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        crate::debug::log(&format!("osascript failed: {}", stderr));

        if stderr.contains("User canceled") || stderr.contains("user canceled") {
            return Err("Authorization cancelled by user".to_string());
        }

        // Check for TCC/Full Disk Access issues
        if stderr.contains("Operation not permitted") || stderr.contains("not permitted") {
            return Err(format!(
                "macOS blocked disk access. Please grant Full Disk Access:\n\
                 1. Open System Settings → Privacy & Security → Full Disk Access\n\
                 2. Click + and add Terminal.app (or this application)\n\
                 3. Restart the application and try again\n\
                 \n\
                 Original error: {}",
                stderr.trim()
            ));
        }

        return Err(format!(
            "Burn failed: {}{}",
            stderr,
            if !stdout.is_empty() { format!("\n{}", stdout) } else { String::new() }
        ));
    }

    // Parse dd output for bytes written
    let output_str = String::from_utf8_lossy(&output.stderr);
    let bytes_written = parse_dd_output(&output_str).unwrap_or(total_size);

    crate::debug::log(&format!("Burn completed: {} bytes written", bytes_written));
    progress_callback(bytes_written, bytes_written);

    Ok(bytes_written)
}

/// Parse dd output to extract bytes transferred
fn parse_dd_output(output: &str) -> Option<u64> {
    // dd output format: "X bytes transferred in Y secs (Z bytes/sec)"
    // or: "X bytes (Y MB, Z MiB) copied, ..."
    for line in output.lines() {
        if line.contains("bytes") {
            // Try to extract the first number
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(num_str) = parts.first() {
                if let Ok(num) = num_str.parse::<u64>() {
                    return Some(num);
                }
            }
        }
    }
    None
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
