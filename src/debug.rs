// Centralized debug logging for SpruceOS Installer
// Logs are written to a temp file and can be copied to SD card after installation

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
        let path = std::env::temp_dir().join("spruce_installer_debug.txt");

        // Clear existing log and write header
        if let Ok(mut f) = std::fs::File::create(&path) {
            let _ = writeln!(f, "=== SpruceOS Installer Debug Log ===");
            let _ = writeln!(f, "Log file: {:?}", path);
            let _ = writeln!(f, "Timestamp: {:?}", std::time::SystemTime::now());
            let _ = writeln!(f, "Platform: {}", std::env::consts::OS);
            let _ = writeln!(f, "Arch: {}", std::env::consts::ARCH);
            let _ = writeln!(f, "");
        }

        Self {
            path,
            enabled: true,
        }
    }
}

/// Log a debug message
pub fn log(message: &str) {
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
        std::env::temp_dir().join("spruce_installer_debug.txt")
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
