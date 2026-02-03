// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod boxart_db;
mod boxart_scraper;
mod burn;
mod config;
mod copy;
mod debug;
mod delete;
mod download_state;
mod drives;
mod eject;
mod extract;
mod fat32;
mod format;
mod github;
mod manifest;

#[cfg(target_os = "macos")]
mod mac;

use app::InstallerApp;
use config::{load_app_icon, load_custom_fonts, WINDOW_MIN_SIZE, WINDOW_SIZE, WINDOW_TITLE};
use eframe::egui;
use std::path::Path;
use std::sync::Arc;

// Helper function to launch with pkexec
#[cfg(all(not(windows), not(target_os = "macos")))]
fn launch_with_pkexec(exe_path: &Path) -> Result<(), String> {
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

    cmd.arg(exe_path);

    cmd.status()
        .map(|_| ())
        .map_err(|e| format!("pkexec failed: {}", e))
}

// Helper function to launch with sudo in a terminal
#[cfg(all(not(windows), not(target_os = "macos")))]
fn launch_with_sudo_terminal(exe_path: &Path) -> Result<(), String> {
    // Try common terminal emulators in order of preference
    let terminals = [
        ("x-terminal-emulator", vec!["-e"]),     // Debian/Ubuntu default
        ("gnome-terminal", vec!["--"]),          // GNOME
        ("konsole", vec!["-e"]),                 // KDE
        ("xfce4-terminal", vec!["-e"]),          // XFCE
        ("mate-terminal", vec!["-e"]),           // MATE
        ("xterm", vec!["-e"]),                   // Fallback
    ];

    for (term, args) in &terminals {
        if which::which(term).is_ok() {
            let mut cmd = std::process::Command::new(term);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.arg("sudo");

            // Preserve DISPLAY for GUI
            if let Ok(display) = std::env::var("DISPLAY") {
                cmd.arg(format!("DISPLAY={}", display));
            }

            cmd.arg(exe_path);

            if cmd.status().is_ok() {
                return Ok(());
            }
        }
    }

    Err("No suitable terminal emulator found".to_string())
}

// Helper function to launch with kdesudo (KDE)
#[cfg(all(not(windows), not(target_os = "macos")))]
fn launch_with_kdesudo(exe_path: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new("kdesudo");
    cmd.arg(exe_path);

    cmd.status()
        .map(|_| ())
        .map_err(|e| format!("kdesudo failed: {}", e))
}

// Helper function to launch with gksudo (older GNOME/GTK)
#[cfg(all(not(windows), not(target_os = "macos")))]
fn launch_with_gksudo(exe_path: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new("gksudo");
    cmd.arg(exe_path);

    cmd.status()
        .map(|_| ())
        .map_err(|e| format!("gksudo failed: {}", e))
}

// Function to check and request privileges on non-Windows platforms
#[cfg(all(not(windows), not(target_os = "macos")))]
fn check_and_request_privileges() {
    if unsafe { libc::geteuid() } != 0 {
        // We are not running as root. Attempt to relaunch with elevated privileges.
        println!("Requesting administrator privileges to write to disk...");

        if let Ok(current_exe) = std::env::current_exe() {
            // Try privilege escalation methods in order of preference

            // 1. Try pkexec first (PolicyKit - most GUI-friendly, common on modern systems)
            if which::which("pkexec").is_ok() {
                println!("Using pkexec for privilege escalation...");
                if launch_with_pkexec(&current_exe).is_ok() {
                    std::process::exit(0);
                }
                eprintln!("pkexec failed, trying fallback methods...");
            }

            // 2. Try kdesudo (KDE environments)
            if which::which("kdesudo").is_ok() {
                println!("Using kdesudo for privilege escalation...");
                if launch_with_kdesudo(&current_exe).is_ok() {
                    std::process::exit(0);
                }
                eprintln!("kdesudo failed, trying fallback methods...");
            }

            // 3. Try gksudo (older GNOME/GTK environments)
            if which::which("gksudo").is_ok() {
                println!("Using gksudo for privilege escalation...");
                if launch_with_gksudo(&current_exe).is_ok() {
                    std::process::exit(0);
                }
                eprintln!("gksudo failed, trying fallback methods...");
            }

            // 4. Fallback to sudo in a terminal (most universal - works on any system with sudo)
            if which::which("sudo").is_ok() {
                println!("Using sudo in terminal for privilege escalation...");
                if launch_with_sudo_terminal(&current_exe).is_ok() {
                    std::process::exit(0);
                }
                eprintln!("sudo terminal launch failed.");
            }

            // If all methods failed, provide helpful error message
            eprintln!("\n===========================================");
            eprintln!("ERROR: Unable to obtain root privileges");
            eprintln!("===========================================");
            eprintln!("This application requires root/administrator access to:");
            eprintln!("  - Format SD cards");
            eprintln!("  - Write disk images");
            eprintln!("  - Access block devices");
            eprintln!();
            eprintln!("No privilege escalation tool found.");
            eprintln!();
            eprintln!("Please install one of the following:");
            eprintln!("  - pkexec:  sudo apt install policykit-1");
            eprintln!("  - or run manually with: sudo {}", current_exe.display());
            eprintln!("===========================================");

            std::process::exit(1);
        } else {
            eprintln!("ERROR: Could not determine executable path to relaunch.");
            std::process::exit(1);
        }
    }
}

// macOS doesn't need privilege elevation at app start
// It uses authopen to request privileges per-operation
#[cfg(target_os = "macos")]
fn check_and_request_privileges() {
    // No-op on macOS - authopen handles privilege elevation when needed
    // The app should be launched from Terminal with Full Disk Access
}

fn main() -> eframe::Result<()> {
    // Call the privilege check at the very beginning of main (not needed on Windows due to manifest)
    #[cfg(not(windows))]
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
            // Load custom fonts first (if configured)
            load_custom_fonts(&cc.egui_ctx);

            // Theme is applied in InstallerApp::new using setup_theme

            // Initialize image loaders for SVG support
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::new(InstallerApp::new(cc)))
        }),
    )
}
