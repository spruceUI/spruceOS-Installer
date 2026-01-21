#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod copy;
mod debug;
mod drives;
mod eject;
mod extract;
mod fat32;
mod format;
mod github;

use app::InstallerApp;
use config::{load_app_icon, WINDOW_MIN_SIZE, WINDOW_SIZE, WINDOW_TITLE};
use eframe::egui;
use std::sync::Arc;

// Function to check and request privileges on non-Windows platforms
#[cfg(not(windows))] 
fn check_and_request_privileges() {
    if unsafe { libc::geteuid() } != 0 {
        // We are not running as root. Attempt to relaunch with elevated privileges.
        println!("Requesting administrator privileges to write to disk...");

        if let Ok(current_exe) = std::env::current_exe() {
            let mut relaunch_command = if cfg!(target_os = "linux") {
                // pkexec doesn't preserve environment variables needed for GUI apps
                // We need to pass display-related env vars through via env command
                let mut cmd = std::process::Command::new("pkexec");
                cmd.arg("env");

                // Pass X11 variables
                if let Ok(display) = std::env::var("DISPLAY") {
                    cmd.arg(format!("DISPLAY={}", display));
                }
                if let Ok(xauth) = std::env::var("XAUTHORITY") {
                    cmd.arg(format!("XAUTHORITY={}", xauth));
                } else if let Ok(home) = std::env::var("HOME") {
                    cmd.arg(format!("XAUTHORITY={}/.Xauthority", home));
                }

                // Pass Wayland variables if present
                if let Ok(wayland) = std::env::var("WAYLAND_DISPLAY") {
                    cmd.arg(format!("WAYLAND_DISPLAY={}", wayland));
                }
                if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR") {
                    cmd.arg(format!("XDG_RUNTIME_DIR={}", xdg_runtime));
                }

                cmd.arg(current_exe);
                cmd
            } else if cfg!(target_os = "macos") {
                let script = format!(
                    "do shell script \"{}\" with administrator privileges",
                    current_exe.to_string_lossy().replace('"', "\\\"") // Escape quotes
                );
                let mut cmd = std::process::Command::new("osascript");
                cmd.arg("-e").arg(script);
                cmd
            } else {
                // Fallback for other non-Windows, non-Linux, non-macos platforms
                eprintln!("Elevated privileges needed but not supported on this platform.");
                std::process::exit(1);
            };

            let status = relaunch_command.status();

            if let Err(e) = status {
                eprintln!("Failed to relaunch with elevated privileges: {}. Please run manually.", e);
            }
        } else {
            eprintln!("Could not determine executable path to relaunch.");
        }

        // Exit the current unprivileged process.
        std::process::exit(0);
    }
}

// Windows platforms don't need this function as privilege elevation is handled by the manifest
#[cfg(windows)] 
fn check_and_request_privileges() {
    // No-op for Windows
}


fn main() -> eframe::Result<()> {
    // Call the privilege check at the very beginning of main
    check_and_request_privileges();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([WINDOW_SIZE.0, WINDOW_SIZE.1])
        .with_min_inner_size([WINDOW_MIN_SIZE.0, WINDOW_MIN_SIZE.1])
        .with_resizable(true);

    // Load custom icon if available
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|cc| {
            // Theme is applied in InstallerApp::new using setup_theme

            // Initialize image loaders for SVG support
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::new(InstallerApp::new(cc)))
        }),
    )
}
