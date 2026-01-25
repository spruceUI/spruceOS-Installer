/// macOS clipboard helper - multiple fallback methods
use std::process::Command;

pub fn copy_text(text: &str) -> Result<(), String> {
    crate::debug::log(&format!("clipboard::copy_text called with {} bytes", text.len()));

    // Write to temp file for all methods that need it
    let temp_path = "/tmp/spruceos_clipboard_temp.txt";
    std::fs::write(temp_path, text).map_err(|e| {
        crate::debug::log(&format!("Failed to write temp file: {}", e));
        format!("Failed to write temp file: {}", e)
    })?;

    // Method 1: Use System Events via osascript to set clipboard from file
    // This sometimes works better than direct clipboard access
    crate::debug::log("Trying System Events clipboard method...");
    let script = format!(
        r#"tell application "System Events"
            set the clipboard to (read POSIX file "{}" as text)
        end tell"#,
        temp_path
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            crate::debug::log("System Events clipboard method succeeded");
            let _ = std::fs::remove_file(temp_path);
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        crate::debug::log(&format!("System Events failed: {}", stderr));
    }

    // Method 2: Direct osascript clipboard set
    crate::debug::log("Trying direct osascript clipboard method...");
    let script2 = format!(
        r#"set the clipboard to (read POSIX file "{}" as text)"#,
        temp_path
    );

    let output2 = Command::new("osascript")
        .arg("-e")
        .arg(&script2)
        .output();

    if let Ok(out) = output2 {
        if out.status.success() {
            crate::debug::log("Direct osascript clipboard method succeeded");
            let _ = std::fs::remove_file(temp_path);
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        crate::debug::log(&format!("Direct osascript failed: {}", stderr));
    }

    // Method 3: pbcopy via shell
    crate::debug::log("Trying pbcopy method...");
    let output3 = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("/usr/bin/pbcopy < '{}'", temp_path))
        .output();

    let _ = std::fs::remove_file(temp_path);

    if let Ok(out) = output3 {
        if out.status.success() {
            crate::debug::log("pbcopy method succeeded");
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        crate::debug::log(&format!("pbcopy failed: {}", stderr));
    }

    Err("All clipboard methods failed".to_string())
}
