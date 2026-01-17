use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::mpsc;

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
    Completed,
    Error(String),
}

pub async fn extract_7z(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<ExtractProgress>,
) -> Result<(), String> {
    crate::debug::log_section("7z Extraction");
    crate::debug::log(&format!("Archive: {:?}", archive_path));
    crate::debug::log(&format!("Destination: {:?}", dest_dir));

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
    let seven_zip_path = temp_dir.join("7zr_spruce.exe");

    #[cfg(not(target_os = "windows"))]
    let seven_zip_path = temp_dir.join("7zr_spruce");

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

    // Run 7z to extract the archive
    // Command: 7zr x archive.7z -oDestination -y
    let output_arg = format!("-o{}", dest_dir.display());
    crate::debug::log(&format!("Running 7z extraction command with output arg: {}", output_arg));

    #[cfg(target_os = "windows")]
    let result = Command::new(&seven_zip_path)
        .arg("x")                           // Extract with full paths
        .arg(archive_path)                  // Archive to extract
        .arg(&output_arg)                   // Output directory
        .arg("-y")                          // Yes to all prompts
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await;

    #[cfg(not(target_os = "windows"))]
    let result = Command::new(&seven_zip_path)
        .arg("x")
        .arg(archive_path)
        .arg(&output_arg)
        .arg("-y")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    // Clean up the temp 7z executable
    let _ = std::fs::remove_file(&seven_zip_path);
    crate::debug::log("Cleaned up temp 7z binary");

    match result {
        Ok(output) => {
            crate::debug::log(&format!("7z exit status: {:?}", output.status));
            if output.status.success() {
                crate::debug::log("7z extraction completed successfully");
                let _ = progress_tx.send(ExtractProgress::Completed);
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                crate::debug::log(&format!("7z stdout: {}", stdout));
                crate::debug::log(&format!("7z stderr: {}", stderr));
                let err_msg = format!(
                    "7z extraction failed:\n{}\n{}",
                    stdout.trim(),
                    stderr.trim()
                );
                crate::debug::log(&format!("ERROR: {}", err_msg));
                let _ = progress_tx.send(ExtractProgress::Error(err_msg.clone()));
                Err(err_msg)
            }
        }
        Err(e) => {
            let err_msg = format!("Failed to run 7z: {}", e);
            crate::debug::log(&format!("ERROR: {}", err_msg));
            let _ = progress_tx.send(ExtractProgress::Error(err_msg.clone()));
            Err(err_msg)
        }
    }
}

/// Alias for backward compatibility with app.rs
pub async fn extract_7z_with_progress(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: mpsc::UnboundedSender<ExtractProgress>,
) -> Result<(), String> {
    extract_7z(archive_path, dest_dir, progress_tx).await
}
