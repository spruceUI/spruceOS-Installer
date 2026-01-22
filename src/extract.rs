use crate::config::TEMP_PREFIX;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

// Embed platform-specific 7z binaries
#[cfg(target_os = "windows")]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Windows/7zr.exe");

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Linux-x86_64/7zzs");

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Linux-aarch64/7zzs");

#[cfg(all(target_os = "linux", target_arch = "x86"))]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Linux-i686/7zzs");

#[cfg(all(target_os = "linux", target_arch = "arm"))]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Linux-armv7/7zzs");

#[cfg(target_os = "macos")]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Mac/7zz");

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone)]
pub enum ExtractProgress {
    Started,
    Extracting,
    Progress { percent: u8 },
    Completed,
    Cancelled,
    Error(String),
}

pub async fn extract_7z(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<ExtractProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    crate::debug::log_section("7z Extraction");
    crate::debug::log(&format!("Archive: {:?}", archive_path));
    crate::debug::log(&format!("Destination: {:?}", dest_dir));

    // Check for cancellation before starting
    if cancel_token.is_cancelled() {
        let _ = progress_tx.send(ExtractProgress::Cancelled);
        return Err("Extraction cancelled".to_string());
    }

    let _ = progress_tx.send(ExtractProgress::Started);

    // Verify archive exists
    if !archive_path.exists() {
        crate::debug::log("ERROR: Archive not found");
        return Err(format!("Archive not found: {:?}", archive_path));
    }
    crate::debug::log("Archive file exists");

    // Ensure destination directory exists
    if !dest_dir.exists() {
        crate::debug::log("Creating destination directory...");
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| format!("Failed to create destination directory: {}", e))?;
    }
    crate::debug::log("Destination directory ready");

    let _ = progress_tx.send(ExtractProgress::Extracting);

    // On macOS, try to use the bundled 7zz from the app bundle first
    // This avoids Gatekeeper quarantine issues since the app is already unquarantined
    #[cfg(target_os = "macos")]
    let (seven_zip_path, is_bundled) = {
        // Try to find 7zz in app bundle: Contents/Resources/7zz
        let bundled_path = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                // exe is at: SpruceOSInstaller.app/Contents/MacOS/spruceos-installer
                // We want: SpruceOSInstaller.app/Contents/Resources/7zz
                exe.parent()  // Contents/MacOS
                    .and_then(|p| p.parent())  // Contents
                    .map(|contents| contents.join("Resources/7zz"))
            })
            .filter(|path| path.exists());

        if let Some(path) = bundled_path {
            crate::debug::log(&format!("Using bundled 7zz from app bundle: {:?}", path));
            (path, true)
        } else {
            crate::debug::log("Bundled 7zz not found, extracting to temp...");
            // Fallback to temp extraction
            let bin_dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
            let temp_path = bin_dir.join(format!("7zr_{}", TEMP_PREFIX));
            std::fs::write(&temp_path, SEVEN_ZIP_EXE)
                .map_err(|e| format!("Failed to extract 7z tool: {}", e))?;
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&temp_path)
                .map_err(|e| format!("Failed to get file permissions: {}", e))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&temp_path, perms)
                .map_err(|e| format!("Failed to set executable permission: {}", e))?;
            crate::debug::log(&format!("Extracted 7z binary to: {:?}", temp_path));
            (temp_path, false)
        }
    };

    // On non-macOS platforms, extract 7z binary to temp/cache directory (always temp-extracted, never bundled)
    #[cfg(not(target_os = "macos"))]
    let (seven_zip_path, is_bundled) = {
        #[cfg(target_os = "linux")]
        let bin_dir = {
            // If running as root via sudo or pkexec, try to use the actual user's cache directory
            if unsafe { libc::geteuid() } == 0 {
                // First check for SUDO_USER (command-line sudo)
                if let Ok(sudo_user) = std::env::var("SUDO_USER") {
                    let user_home = std::path::PathBuf::from(format!("/home/{}", sudo_user));
                    if user_home.exists() {
                        let user_cache = user_home.join(".cache");
                        crate::debug::log(&format!("Extracting 7z binary using cache dir for sudo user {}: {:?}", sudo_user, user_cache));
                        user_cache
                    } else {
                        dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                    }
                }
                // Check for PKEXEC_UID (GUI elevation via pkexec)
                else if let Ok(pkexec_uid) = std::env::var("PKEXEC_UID") {
                    if let Ok(uid) = pkexec_uid.parse::<u32>() {
                        let pwd = unsafe { libc::getpwuid(uid) };
                        if !pwd.is_null() {
                            let username = unsafe {
                                std::ffi::CStr::from_ptr((*pwd).pw_name)
                                    .to_string_lossy()
                                    .to_string()
                            };
                            let user_home = std::path::PathBuf::from(format!("/home/{}", username));
                            if user_home.exists() {
                                let user_cache = user_home.join(".cache");
                                crate::debug::log(&format!("Extracting 7z binary using cache dir for pkexec user {} (UID {}): {:?}", username, uid, user_cache));
                                user_cache
                            } else {
                                dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                            }
                        } else {
                            dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                        }
                    } else {
                        dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                    }
                }
                else {
                    dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
                }
            } else {
                dirs::cache_dir().unwrap_or_else(std::env::temp_dir)
            }
        };
        #[cfg(not(target_os = "linux"))]
        let bin_dir = std::env::temp_dir();

        #[cfg(target_os = "windows")]
        let temp_path = bin_dir.join(format!("7zr_{}.exe", TEMP_PREFIX));
        #[cfg(not(target_os = "windows"))]
        let temp_path = bin_dir.join(format!("7zr_{}", TEMP_PREFIX));

        crate::debug::log(&format!("Extracting 7z binary to: {:?}", temp_path));
        std::fs::write(&temp_path, SEVEN_ZIP_EXE)
            .map_err(|e| format!("Failed to extract 7z tool: {}", e))?;
        crate::debug::log("7z binary extracted successfully");

        // On Unix (Linux), make the binary executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&temp_path)
                .map_err(|e| format!("Failed to get file permissions: {}", e))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&temp_path, perms)
                .map_err(|e| format!("Failed to set executable permission: {}", e))?;
        }

        (temp_path, false)
    };

    // Run 7z to extract the archive with -bsp1 for progress output
    let output_arg = format!("-o{}", dest_dir.display());
    crate::debug::log(&format!("Running 7z extraction command with output arg: {}", output_arg));

    #[cfg(target_os = "windows")]
    let mut child = Command::new(&seven_zip_path)
        .arg("x")
        .arg(archive_path)
        .arg(&output_arg)
        .arg("-y")
        .arg("-bsp1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to start 7z: {}", e))?;

    #[cfg(not(target_os = "windows"))]
    let mut child = Command::new(&seven_zip_path)
        .arg("x")
        .arg(archive_path)
        .arg(&output_arg)
        .arg("-y")
        .arg("-bsp1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start 7z: {}", e))?;

    crate::debug::log(&format!("7z process started (PID: {:?})", child.id()));

    // Take stdout for progress parsing
    let mut stdout = child.stdout.take()
        .ok_or_else(|| "Failed to capture 7z stdout".to_string())?;

    // Take stderr for real-time logging
    let mut stderr = child.stderr.take()
        .ok_or_else(|| "Failed to capture 7z stderr".to_string())?;

    // Log stderr in real-time instead of buffering
    let stderr_handle = tokio::spawn(async move {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            match stderr.read(&mut chunk).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let text = String::from_utf8_lossy(&chunk[..n]);
                    if !text.trim().is_empty() {
                        crate::debug::log(&format!("7z stderr: {}", text.trim()));
                    }
                    buffer.extend_from_slice(&chunk[..n]);
                }
                Err(e) => {
                    crate::debug::log(&format!("Error reading 7z stderr: {}", e));
                    break;
                }
            }
        }
        buffer
    });

    let mut last_percent: u8 = 0;
    let mut buffer = [0u8; 1024];
    let mut last_output_time = std::time::Instant::now();

    // Read stdout looking for progress
    // 7z with -bsp1 uses backspaces or carriage returns, so we read raw chunks
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                crate::debug::log("Extraction cancelled by user");
                let _ = child.kill().await;
                // Only delete if it's a temp-extracted binary, not a bundled one
                if !is_bundled {
                    let _ = std::fs::remove_file(&seven_zip_path);
                }
                let _ = progress_tx.send(ExtractProgress::Cancelled);
                return Err("Extraction cancelled".to_string());
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                // Check if we've received output recently (within 5 minutes)
                let elapsed = last_output_time.elapsed();
                if elapsed > std::time::Duration::from_secs(300) {
                    crate::debug::log(&format!("Extraction timeout: no output for {} seconds", elapsed.as_secs()));
                    let _ = child.kill().await;
                    if !is_bundled {
                        let _ = std::fs::remove_file(&seven_zip_path);
                    }
                    let _ = progress_tx.send(ExtractProgress::Error("Extraction timed out (no progress for 5 minutes)".to_string()));
                    return Err("Extraction timed out - the process may have hung".to_string());
                }
            }
            read_result = stdout.read(&mut buffer) => {
                match read_result {
                    Ok(0) => {
                        // EOF - process finished output
                        crate::debug::log("7z stdout reached EOF");
                        break;
                    }
                    Ok(n) => {
                        last_output_time = std::time::Instant::now();
                        // Parse the buffer for percentage
                        let text = String::from_utf8_lossy(&buffer[..n]);
                        if let Some(percent) = parse_last_percentage(&text) {
                            if percent != last_percent {
                                last_percent = percent;
                                crate::debug::log(&format!("Extraction progress: {}%", percent));
                                let _ = progress_tx.send(ExtractProgress::Progress { percent });
                            }
                        }
                    }
                    Err(e) => {
                        crate::debug::log(&format!("Error reading 7z output: {}", e));
                        break;
                    }
                }
            }
        }
    }

    // Wait for process to complete
    let status = child.wait().await
        .map_err(|e| format!("Failed to wait for 7z: {}", e))?;

    // Clean up the temp 7z executable (only if not bundled)
    if !is_bundled {
        let _ = std::fs::remove_file(&seven_zip_path);
        crate::debug::log("Cleaned up temp 7z binary");
    } else {
        crate::debug::log("Keeping bundled 7z binary (from app bundle)");
    }

    if status.success() {
        crate::debug::log("7z extraction completed successfully");
        let _ = progress_tx.send(ExtractProgress::Completed);
        Ok(())
    } else {
        // Get stderr output from background task
        let stderr_output = if let Ok(buf) = stderr_handle.await {
            String::from_utf8_lossy(&buf).trim().to_string()
        } else {
            String::new()
        };

        let exit_code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
        let err_msg = if stderr_output.is_empty() {
            format!("7z extraction failed with exit code: {}", exit_code)
        } else {
            format!("7z extraction failed (code {}): {}", exit_code, stderr_output)
        };

        crate::debug::log(&format!("ERROR: {}", err_msg));
        let _ = progress_tx.send(ExtractProgress::Error(err_msg.clone()));
        Err(err_msg)
    }
}

/// Parse the last percentage from a text chunk
/// Finds the last occurrence of "N%" in the text
fn parse_last_percentage(text: &str) -> Option<u8> {
    // Collect all matches of '%'
    text.match_indices('%')
        .fold(None, |acc, (idx, _)| {
            // Check preceding characters for digits
            let prefix = &text[..idx];
            let num_str: String = prefix
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();

            if !num_str.is_empty() {
                if let Ok(percent) = num_str.parse::<u8>() {
                    return Some(percent.min(100));
                }
            }
            acc
        })
}

/// Main entry point for extraction with cancellation support
pub async fn extract_7z_with_progress(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<ExtractProgress>,
    cancel_token: CancellationToken,
) -> Result<(), String> {
    extract_7z(archive_path, dest_dir, progress_tx, cancel_token).await
}
