/// macOS clipboard helper - multiple fallback methods
use std::process::Command;

pub fn copy_text(text: &str) -> Result<(), String> {
    // Write to temp file for methods that need it
    let temp_path = "/tmp/spruceos_clipboard_temp.txt";
    std::fs::write(temp_path, text).map_err(|e| {
        format!("Failed to write temp file: {}", e)
    })?;

    // Method 1: Use System Events via osascript to set clipboard from file
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
            let _ = std::fs::remove_file(temp_path);
            return Ok(());
        }
    }

    // Method 2: Direct osascript clipboard set
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
            let _ = std::fs::remove_file(temp_path);
            return Ok(());
        }
    }

    // Method 3: pbcopy via shell
    let output3 = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("/usr/bin/pbcopy < '{}'", temp_path))
        .output();

    let _ = std::fs::remove_file(temp_path);

    if let Ok(out) = output3 {
        if out.status.success() {
            return Ok(());
        }
    }

    Err("All clipboard methods failed".to_string())
}
