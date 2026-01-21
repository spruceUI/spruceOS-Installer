// Centralized debug logging for the installer
// Logs are written to a temp file and can be copied to SD card after installation

use crate::config::{APP_NAME, TEMP_PREFIX};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref DEBUG_LOG: Mutex<DebugLog> = Mutex::new(DebugLog::new());
}

pub struct DebugLog {
    path: PathBuf,
    enabled: bool,
}

impl DebugLog {
    fn new() -> Self {
        let log_filename = format!("{}_debug.txt", TEMP_PREFIX);
        
        // On macOS with elevation, current_dir changes and temp_dir changes.
        // Use /tmp explicitly for finding logs easily.
        #[cfg(target_os = "macos")]
        let path = PathBuf::from("/tmp").join(&log_filename);
        
        #[cfg(not(target_os = "macos"))]
        let path = std::env::current_dir()
            .map(|cwd| {
                let target = cwd.join("target");
                if target.exists() {
                    target.join(&log_filename)
                } else {
                    cwd.join(&log_filename)
                }
            })
            .unwrap_or_else(|_| std::env::temp_dir().join(&log_filename));

        println!("[DEBUG] Initializing log at: {:?}", path);

        // Try to create the file to verify write permissions
        // If it fails (e.g. running as different user in restricted dir), fall back to temp
        let final_path = if std::fs::File::create(&path).is_ok() {
            path
        } else {
            let temp_path = std::env::temp_dir().join(&log_filename);
            println!("[DEBUG] Failed to write to preferred path, falling back to: {:?}", temp_path);
            temp_path
        };

        // Write header
        if let Ok(mut f) = std::fs::File::create(&final_path) {
            let _ = writeln!(f, "=== {} Installer Debug Log ===", APP_NAME);
            let _ = writeln!(f, "Log file: {:?}", final_path);
            let _ = writeln!(f, "Timestamp: {:?}", std::time::SystemTime::now());
            let _ = writeln!(f, "Platform: {}", std::env::consts::OS);
            let _ = writeln!(f, "Arch: {}", std::env::consts::ARCH);
            let _ = writeln!(f, "");
        }

        Self {
            path: final_path,
            enabled: true,
        }
    }
}

/// Log a debug message
pub fn log(message: &str) {
    // Also print to stdout for VS Code debug console visibility
    println!("[DEBUG] {}", message);

    if let Ok(debug_log) = DEBUG_LOG.lock() {
        if debug_log.enabled {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&debug_log.path)
            {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = writeln!(f, "[{}] {}", timestamp, message);
            }
        }
    }
}

/// Log a section header
pub fn log_section(section: &str) {
    log(&format!("\n=== {} ===", section));
}

/// Get the path to the debug log file
pub fn get_log_path() -> PathBuf {
    if let Ok(debug_log) = DEBUG_LOG.lock() {
        debug_log.path.clone()
    } else {
        let log_filename = format!("{}_debug.txt", TEMP_PREFIX);
        std::env::temp_dir().join(log_filename)
    }
}

/// Copy the debug log to a destination directory (e.g., SD card)
pub fn copy_log_to(dest_dir: &std::path::Path) -> Result<PathBuf, String> {
    let log_path = get_log_path();
    let dest_path = dest_dir.join("installer_debug.txt");

    // Add final log entry before copying
    log("Copying debug log to SD card...");
    log(&format!("Destination: {:?}", dest_path));

    std::fs::copy(&log_path, &dest_path)
        .map_err(|e| format!("Failed to copy debug log: {}", e))?;

    Ok(dest_path)
}
