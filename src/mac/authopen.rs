// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

/// macOS-specific authorization and file opening using authopen
/// Based on Raspberry Pi Imager's implementation with socketpair FD passing
use std::fs::File;
use std::io::{self, Read};
use std::mem;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
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

    let device_str = device_path.to_string_lossy();

    // O_RDWR | O_SYNC = 0x0002 | 0x0080 = 0x0082
    let flags = 0x0082;

    // Create a Unix domain socketpair for receiving the FD from authopen
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
            let log_path = crate::debug::get_log_path();
            return Err(AuthOpenError::Failed(format!(
                "Permission denied - make sure you have admin rights.\n\nError: {}\n\nDebug log: {:?}\nClick 'Copy Log to Clipboard' to share this error.",
                stderr.trim(),
                log_path
            )));
        }

        // Generic authorization failure
        let log_path = crate::debug::get_log_path();
        return Err(AuthOpenError::Failed(format!(
            "Authorization failed: {}\n\nDebug log: {:?}\nClick 'Copy Log to Clipboard' to share this error.",
            stderr.trim(),
            log_path
        )));
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

    // Set F_NOCACHE to bypass kernel buffer cache and write directly to hardware
    // This prevents the "99% stall" issue where data sits in RAM cache waiting to flush
    let raw_fd = file.as_raw_fd();
    unsafe {
        if libc::fcntl(raw_fd, libc::F_NOCACHE, 1) == -1 {
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
