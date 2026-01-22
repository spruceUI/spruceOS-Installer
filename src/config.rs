// ============================================================================
// INSTALLER CONFIGURATION
// ============================================================================
// Edit this file to customize the installer for your OS project.
//
// QUICK START - To rebrand this installer, change these values:
//   1. APP_NAME        - Your OS name (e.g., "SpruceOS", "Onion", "MinUI")
//   2. VOLUME_LABEL    - SD card label, max 11 chars uppercase (e.g., "SPRUCEOS")
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
use std::sync::Arc;

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
    ("Stable", "spruceUI/spruceOS"),
    ("Nightlies", "spruceUI/spruceOSNightlies"),
    ("SprigUI", "spruceUI/sprigUI"),
];

/// Index of the default repository selection (0 = first option)
pub const DEFAULT_REPO_INDEX: usize = 0;

/// File extension to look for in GitHub releases (e.g., ".7z", ".zip")
/// The installer will download the first asset matching this extension
pub const ASSET_EXTENSION: &str = ".7z";

// ----------------------------------------------------------------------------
// WINDOW SETTINGS
// ----------------------------------------------------------------------------

/// Default window size (width, height)
pub const WINDOW_SIZE: (f32, f32) = (679.5, 420.0);

/// Minimum window size (width, height)
pub const WINDOW_MIN_SIZE: (f32, f32) = (679.5, 420.0);

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

// ----------------------------------------------------------------------------
// CUSTOM FONT CONFIGURATION
// ----------------------------------------------------------------------------
// To use a different font, replace the file at assets/Fonts/nunwen.ttf
// with your own TTF/OTF file and update CUSTOM_FONT_NAME if desired

/// Embedded custom font (TTF/OTF format)
pub const CUSTOM_FONT: &[u8] = include_bytes!("../assets/Fonts/nunwen.ttf");

/// Font family name (used to reference the font in the UI)
pub const CUSTOM_FONT_NAME: &str = "Nunwen";

/// Load custom fonts into egui
/// Call this during app initialization, before creating the UI
pub fn load_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Load the custom font data
    fonts.font_data.insert(
        CUSTOM_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(CUSTOM_FONT)),
    );

    // Set it as the first priority for proportional text (default UI text)
    fonts.families.entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, CUSTOM_FONT_NAME.to_owned());

    // Optionally also use it for monospace text (code, logs)
    // fonts.families.entry(egui::FontFamily::Monospace)
    //     .or_default()
    //     .insert(0, CUSTOM_FONT_NAME.to_owned());

    ctx.set_fonts(fonts);
}

// ============================================================================
// THEME SETUP (internal use)
// ============================================================================

pub fn setup_theme(ctx: &egui::Context) {
    use egui_thematic::ThemeConfig;

    let is_dark = ctx.style().visuals.dark_mode;
    let theme = if is_dark {
        ThemeConfig::gruvbox_dark_preset()
    } else {
        // TODO: not sure what light preset would fit spruceos branding,
        // pick one from theme editor
        ThemeConfig::gruvbox_dark_preset()
    };
    ctx.set_visuals(theme.to_visuals());
}
