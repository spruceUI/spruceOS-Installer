use crate::config::TEMP_PREFIX;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// Embed platform-specific 7z binaries
#[cfg(target_os = "windows")]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Windows/7zr.exe");

#[cfg(target_os = "linux")]
const SEVEN_ZIP_EXE: &[u8] = include_bytes!("../assets/Linux/7zzs");

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

    // Extract 7z binary to temp directory with platform-appropriate name
    let temp_dir = std::env::temp_dir();

    #[cfg(target_os = "windows")]
    let seven_zip_path = temp_dir.join(format!("7zr_{}.exe", TEMP_PREFIX));

    #[cfg(not(target_os = "windows"))]
    let seven_zip_path = temp_dir.join(format!("7zr_{}", TEMP_PREFIX));

    // Write the embedded 7z executable to temp
    crate::debug::log(&format!("Extracting 7z binary to: {:?}", seven_zip_path));
    std::fs::write(&seven_zip_path, SEVEN_ZIP_EXE)
        .map_err(|e| format!("Failed to extract 7z tool: {}", e))?;
    crate::debug::log("7z binary extracted successfully");

    // On Unix, make the binary executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&seven_zip_path)
            .map_err(|e| format!("Failed to get file permissions: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&seven_zip_path, perms)
            .map_err(|e| format!("Failed to set executable permission: {}", e))?;
    }

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

    // Take stdout for progress parsing
    let stdout = child.stdout.take()
        .ok_or_else(|| "Failed to capture 7z stdout".to_string())?;

    let mut reader = BufReader::new(stdout);
    let mut last_percent: u8 = 0;
    let mut buffer = Vec::new();

    // Read stdout looking for progress - 7z uses \r for progress updates
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                crate::debug::log("Extraction cancelled by user");
                let _ = child.kill().await;
                let _ = std::fs::remove_file(&seven_zip_path);
                let _ = progress_tx.send(ExtractProgress::Cancelled);
                return Err("Extraction cancelled".to_string());
            }
            read_result = reader.read_until(b'\r', &mut buffer) => {
                match read_result {
                    Ok(0) => {
                        // EOF - process finished output
                        break;
                    }
                    Ok(_) => {
                        // Parse the buffer for percentage
                        if let Ok(line) = std::str::from_utf8(&buffer) {
                            if let Some(percent) = parse_7z_percentage(line) {
                                if percent != last_percent {
                                    last_percent = percent;
                                    let _ = progress_tx.send(ExtractProgress::Progress { percent });
                                }
                            }
                        }
                        buffer.clear();
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

    // Clean up the temp 7z executable
    let _ = std::fs::remove_file(&seven_zip_path);
    crate::debug::log("Cleaned up temp 7z binary");

    if status.success() {
        crate::debug::log("7z extraction completed successfully");
        let _ = progress_tx.send(ExtractProgress::Completed);
        Ok(())
    } else {
        let err_msg = format!("7z extraction failed with exit code: {:?}", status.code());
        crate::debug::log(&format!("ERROR: {}", err_msg));
        let _ = progress_tx.send(ExtractProgress::Error(err_msg.clone()));
        Err(err_msg)
    }
}

/// Parse percentage from 7z output line
/// 7z with -bsp1 outputs progress like " 45% 12 - filename" or just " 45%"
fn parse_7z_percentage(line: &str) -> Option<u8> {
    // Look for pattern like "45%" anywhere in the line
    if let Some(percent_pos) = line.find('%') {
        let before_percent = &line[..percent_pos];
        let num_str: String = before_percent
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
    }
    None
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
