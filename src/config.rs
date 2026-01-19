// ============================================================================
// INSTALLER CONFIGURATION
// ============================================================================
// Edit this file to customize the installer for your OS project.
//
// QUICK START - To rebrand this installer, change these values:
//   1. APP_NAME        - Your OS name (e.g., "SpruceOS", "Onion", "MinUI")
//   2. VOLUME_LABEL    - SD card label, max 11 chars uppercase (e.g., "SPRUCE")
//   3. REPO_OPTIONS    - Your GitHub repositories
//
// ALSO UPDATE THESE EXTERNAL FILES:
//   - Cargo.toml: name, description, authors fields
//   - assets/Mac/Info.plist: CFBundleName, CFBundleDisplayName, CFBundleIdentifier
//   - assets/Icons/icon.png and icon.ico: Your app icons
//   - .github/workflows/*.yml: Artifact names (optional, cosmetic only)
//
// ============================================================================

use eframe::egui;

// ----------------------------------------------------------------------------
// BRANDING
// ----------------------------------------------------------------------------

/// The name of your OS (displayed in window title and UI)
/// Examples: "SpruceOS", "Onion", "MinUI"
pub const APP_NAME: &str = "SpruceOS";

/// Volume label applied to formatted SD cards (max 11 characters, uppercase)
/// This is what the SD card will be named in file explorers
pub const VOLUME_LABEL: &str = "SPRUCEOS";

// ----------------------------------------------------------------------------
// INTERNAL IDENTIFIERS (auto-generated from APP_NAME)
// You generally don't need to change these unless you want custom values
// ----------------------------------------------------------------------------

/// Window title (displayed in title bar)
pub const WINDOW_TITLE: &str = concat!(env!("CARGO_PKG_NAME"), " Installer");

/// User-Agent string for HTTP requests to GitHub
pub const USER_AGENT: &str = env!("CARGO_PKG_NAME");

/// Prefix for temporary folders and files
pub const TEMP_PREFIX: &str = env!("CARGO_PKG_NAME");

// ----------------------------------------------------------------------------
// REPOSITORY OPTIONS
// ----------------------------------------------------------------------------
// Each entry is (Display Name, GitHub repo in "owner/repo" format)

pub const REPO_OPTIONS: &[(&str, &str)] = &[
    ("SpruceOS (Stable)", "spruceUI/spruceOS"),
    ("SpruceOS (Nightlies)", "spruceUI/spruceOSNightlies"),
    ("SprigUI (Stable)", "spruceUI/sprigUI"),
];

/// Index of the default repository selection (0 = first option)
pub const DEFAULT_REPO_INDEX: usize = 0;

/// File extension to look for in GitHub releases (e.g., ".7z", ".zip")
/// The installer will download the first asset matching this extension
pub const ASSET_EXTENSION: &str = ".7z";

// ----------------------------------------------------------------------------
// THEME COLORS
// ----------------------------------------------------------------------------
// All colors use RGB values (0-255)

/// Dark background color (main window background)
pub const COLOR_BG_DARK: egui::Color32 = egui::Color32::from_rgb(45, 45, 45);

/// Medium background color (input fields, panels)
pub const COLOR_BG_MEDIUM: egui::Color32 = egui::Color32::from_rgb(55, 55, 55);

/// Light background color (buttons, interactive elements)
pub const COLOR_BG_LIGHT: egui::Color32 = egui::Color32::from_rgb(70, 70, 70);

/// Primary accent color (headings, highlights, progress bars)
pub const COLOR_ACCENT: egui::Color32 = egui::Color32::from_rgb(212, 168, 75);

/// Dimmed accent color (hover states, selections)
pub const COLOR_ACCENT_DIM: egui::Color32 = egui::Color32::from_rgb(170, 135, 60);

/// Primary text color
pub const COLOR_TEXT: egui::Color32 = egui::Color32::from_rgb(180, 175, 165);

/// Dimmed text color (labels, secondary text)
pub const COLOR_TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(139, 133, 120);

/// Success color (completion messages)
pub const COLOR_SUCCESS: egui::Color32 = egui::Color32::from_rgb(91, 154, 91);

/// Error color (error messages)
pub const COLOR_ERROR: egui::Color32 = egui::Color32::from_rgb(180, 90, 90);

/// Warning color (warning messages, destructive action alerts)
pub const COLOR_WARNING: egui::Color32 = egui::Color32::from_rgb(212, 168, 75);

/// Spinner color (loading indicator during eject)
pub const COLOR_SPINNER: egui::Color32 = egui::Color32::from_rgb(170, 135, 60);

// ----------------------------------------------------------------------------
// WINDOW SETTINGS
// ----------------------------------------------------------------------------

/// Default window size (width, height)
pub const WINDOW_SIZE: (f32, f32) = (500.0, 400.0);

/// Minimum window size (width, height)
pub const WINDOW_MIN_SIZE: (f32, f32) = (400.0, 300.0);

// ----------------------------------------------------------------------------
// ICON CONFIGURATION
// ----------------------------------------------------------------------------
// To customize the application icon:
//
// 1. Window icon (title bar, taskbar - all platforms):
//    - Place your icon at: assets/Icons/icon.png
//    - Recommended: 64x64 or 128x128 PNG with transparency
//
// 2. Windows executable icon (file explorer, taskbar):
//    - Place your icon at: assets/Icons/icon.ico
//    - Uncomment the IDI_ICON1 line in app.rc
//    - Recommended: Multi-resolution .ico (16x16, 32x32, 48x48, 256x256)
//
// 3. Rebuild the application
// ----------------------------------------------------------------------------

/// Embedded window icon (PNG format)
/// Change this path to use a different icon file
#[cfg(feature = "icon")]
pub const APP_ICON_PNG: &[u8] = include_bytes!("../assets/Icons/icon.png");

/// Load the application icon for the window
/// Returns None if no icon is configured or if loading fails
pub fn load_app_icon() -> Option<egui::IconData> {
    #[cfg(feature = "icon")]
    {
        let image = image::load_from_memory(APP_ICON_PNG).ok()?.into_rgba8();
        let (width, height) = image.dimensions();
        Some(egui::IconData {
            rgba: image.into_raw(),
            width,
            height,
        })
    }
    #[cfg(not(feature = "icon"))]
    {
        None
    }
}

// ============================================================================
// THEME SETUP (internal use)
// ============================================================================

pub fn setup_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    // Window and panel backgrounds
    visuals.panel_fill = COLOR_BG_DARK;
    visuals.window_fill = COLOR_BG_DARK;
    visuals.extreme_bg_color = COLOR_BG_DARK;
    visuals.faint_bg_color = COLOR_BG_MEDIUM;

    // Window styling (for popup dialogs)
    visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 2.0),
        blur: 8.0,
        spread: 0.0,
        color: egui::Color32::from_black_alpha(100),
    };
    visuals.window_stroke = egui::Stroke::new(1.0, COLOR_ACCENT_DIM);
    visuals.window_rounding = egui::Rounding::same(8.0);

    // Popup menu styling
    visuals.popup_shadow = visuals.window_shadow;

    // Widget colors
    // noninteractive = labels, text, and other non-clickable elements
    visuals.widgets.noninteractive.bg_fill = COLOR_BG_MEDIUM;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, COLOR_TEXT);

    // inactive = buttons and interactive elements when not hovered
    visuals.widgets.inactive.bg_fill = COLOR_BG_LIGHT;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, COLOR_TEXT);

    visuals.widgets.hovered.bg_fill = COLOR_ACCENT_DIM;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    visuals.widgets.active.bg_fill = COLOR_ACCENT;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, COLOR_BG_DARK);

    visuals.widgets.open.bg_fill = COLOR_BG_LIGHT;
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, COLOR_TEXT);

    // Selection color
    visuals.selection.bg_fill = COLOR_ACCENT_DIM;
    visuals.selection.stroke = egui::Stroke::new(1.0, COLOR_ACCENT);

    // Hyperlink color
    visuals.hyperlink_color = COLOR_ACCENT;

    ctx.set_visuals(visuals);
}
