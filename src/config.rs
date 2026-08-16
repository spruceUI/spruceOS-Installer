// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

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
// CONTROLLING UPDATE MODE FUNCTIONALITY
// ============================================================================
// Update mode allows users to preserve ROMs/saves while updating system files.
// You can control this feature on a per-repository basis or globally.
//
// PER-REPOSITORY CONTROL (Recommended):
//   Set the supports_update_mode field in each RepoOption below:
//   - true: Show update mode checkbox (for archives: .7z, .zip)
//   - false: Hide update mode checkbox (for raw images: .img.gz)
//
// GLOBAL DISABLE - Hide checkbox for all repositories:
//   1. Open src/app/ui.rs
//   2. Search for "Update existing installation (skip format)"
//   3. Comment out the entire checkbox block
//
// COMPLETE REMOVAL - Delete all update mode code:
//   Search for "update_mode" in these files and remove related code:
//   - src/app/state.rs: Field declaration and initialization
//   - src/app/ui.rs: Checkbox UI, window titles, button text
//   - src/app/logic.rs: Installation logic that checks update_mode
//   - src/config.rs: supports_update_mode and update_directories fields
//
// See README.md "STEP 10: Controlling Update Mode" for detailed instructions.
// ============================================================================

use eframe::egui;
use std::sync::Arc;

// ----------------------------------------------------------------------------
// BRANDING
// ----------------------------------------------------------------------------

/// The name of your OS (displayed in window title and UI)
/// Examples: "SpruceOS", "Onion", "MinUI"
pub const APP_NAME: &str = "BaseOS";

/// Volume label applied to formatted SD cards (max 11 characters, uppercase)
/// This is what the SD card will be named in file explorers
///
/// NOTE: BaseOS ships raw disk images, so the installer never formats a card
/// itself — this label is unused in practice and kept only for completeness.
pub const VOLUME_LABEL: &str = "BASEOS";

// ----------------------------------------------------------------------------
// INTERNAL IDENTIFIERS (auto-generated from APP_NAME)
// You generally don't need to change these unless you want custom values
// ----------------------------------------------------------------------------

/// Window title (displayed in title bar)
pub const WINDOW_TITLE: &str = "BaseOS Installer";

/// User-Agent string for HTTP requests to GitHub
pub const USER_AGENT: &str = env!("CARGO_PKG_NAME");

/// Prefix for temporary folders and files
pub const TEMP_PREFIX: &str = env!("CARGO_PKG_NAME");

// ----------------------------------------------------------------------------
// REPOSITORY OPTIONS
// ----------------------------------------------------------------------------

/// Asset display mapping for user-friendly device names
///
/// Maps filename patterns to human-readable display names and device lists
pub struct AssetDisplayMapping {
    /// Pattern to match in the asset filename (e.g., "RK3326")
    pub pattern: &'static str,
    /// Display name shown as the main title in UI (e.g., "RK3326 Chipset")
    pub display_name: &'static str,
    /// Comma-separated list of compatible devices (e.g., "Device A, Device B")
    pub devices: &'static str,
}

/// Repository configuration for download sources
///
/// Each repository entry contains:
/// - `name`: Display name shown in the UI button (e.g., "Stable", "Nightlies")
/// - `url`: GitHub repository in "owner/repo" format (e.g., "spruceUI/spruceOS")
/// - `info`: Description text shown below the Install button when this repo is selected
///           Use \n for line breaks in longer informative messages
///           Supports markdown-style links: [text](url)
/// - `display_name`: Optional full name used in success/error popups (e.g., Some("SpruceOS Stable"))
///                   Falls back to `name` if set to None
/// - `supports_update_mode`: Whether to show the update mode checkbox for this repo
///                           Set to true for archive-based installs (.7z, .zip)
///                           Set to false for raw disk images (.img.gz) that always do full burns
/// - `update_directories`: Directories to delete when updating (e.g., &["Retroarch", "spruce"])
///                         Paths are relative to SD card root
///                         NOTE: Only used when update mode is enabled
/// - `allowed_extensions`: Optional filter to only show assets with these extensions
///                         Use this to filter out update packages or show only specific formats
///                         Set to None to show all assets
/// - `asset_display_mappings`: Optional mappings to show user-friendly device names
///                             instead of technical filenames in the selection UI
/// - `supports_preserve_mode`: Whether to show "Preserve user data" checkbox in update mode
///                             When enabled, user data is backed up before deletion and restored after install
///                             The paths to preserve are defined in UPDATE_PRESERVE_PATHS
///
/// Example (archive-based repository):
/// ```
/// RepoOption {
///     name: "Stable",
///     url: "spruceUI/spruceOS",
///     info: "Stable releases with update support.\nSupported devices: Device X, Y, Z",
///     display_name: Some("SpruceOS Stable"),  // Optional: Full name for popups
///     supports_update_mode: true,  // Archives can be updated
///     update_directories: &["Retroarch", "spruce"],
///     allowed_extensions: Some(&[".7z", ".zip"]),  // Only show archives
///     asset_display_mappings: None,
///     supports_preserve_mode: true,  // Show preserve user data checkbox
/// }
/// ```
///
/// Example (raw disk image repository):
/// ```
/// RepoOption {
///     name: "TwigUI",
///     url: "spruceUI/twigUI",
///     info: "Raw disk images for GKD Pixel 2.\nFresh install only - wipes all data.",
///     display_name: None,  // Falls back to "TwigUI"
///     supports_update_mode: false,  // Raw images always do full burns
///     update_directories: &[],  // Not used for raw images
///     allowed_extensions: Some(&[".img.gz", ".img"]),  // Only raw images
///     asset_display_mappings: None,
///     supports_preserve_mode: false,  // No preserve for raw images
/// }
/// ```
pub struct RepoOption {
    pub name: &'static str,
    pub url: &'static str,
    pub info: &'static str,
    pub display_name: Option<&'static str>,  // Optional: Full name for popups/messages (falls back to name)
    pub supports_update_mode: bool,
    pub update_directories: &'static [&'static str],
    pub allowed_extensions: Option<&'static [&'static str]>,
    pub asset_display_mappings: Option<&'static [AssetDisplayMapping]>,
    pub supports_preserve_mode: bool,  // Show "Preserve user data" checkbox in update mode
}

/// Device name mappings for BaseOS release assets.
///
/// Assets are named `baseos-<device>-<version>.img.zip`. Matching is a plain
/// substring test against the filename and the FIRST match wins, so more
/// specific patterns must come first — `-rg34xx` would otherwise swallow
/// `-rg34xxsp`. The leading dash keeps patterns anchored to the device field.
pub const BASEOS_DEVICE_MAPPINGS: &[AssetDisplayMapping] = &[
    AssetDisplayMapping { pattern: "-rg35xxplus", display_name: "RG35XX Plus", devices: "Anbernic RG35XX Plus" },
    AssetDisplayMapping { pattern: "-rg35xxpro",  display_name: "RG35XX Pro",  devices: "Anbernic RG35XX Pro" },
    AssetDisplayMapping { pattern: "-rg35xxsp",   display_name: "RG35XX SP",   devices: "Anbernic RG35XX SP" },
    AssetDisplayMapping { pattern: "-rg35xxh",    display_name: "RG35XX H",    devices: "Anbernic RG35XX H" },
    AssetDisplayMapping { pattern: "-rg34xxsp",   display_name: "RG34XX SP",   devices: "Anbernic RG34XX SP" },
    AssetDisplayMapping { pattern: "-rg34xx",     display_name: "RG34XX",      devices: "Anbernic RG34XX" },
    AssetDisplayMapping { pattern: "-rg40xxh",    display_name: "RG40XX H",    devices: "Anbernic RG40XX H" },
    AssetDisplayMapping { pattern: "-rg40xxv",    display_name: "RG40XX V",    devices: "Anbernic RG40XX V" },
    AssetDisplayMapping { pattern: "-rgcubexx",   display_name: "RG CubeXX",   devices: "Anbernic RG CubeXX" },
    AssetDisplayMapping { pattern: "-rg28xx",     display_name: "RG28XX",      devices: "Anbernic RG28XX" },
    AssetDisplayMapping { pattern: "-rgsp",       display_name: "RG SP",       devices: "Anbernic RG SP" },
];

pub const REPO_OPTIONS: &[RepoOption] = &[
    RepoOption {
        name: "BaseOS",
        url: "pvaibhav/BaseOS",
        info: "A minimal base OS for Anbernic RG XX devices. 3 second boot time.\nRaw disk image — this erases the entire SD card.\n[Project page](https://github.com/pvaibhav/BaseOS)",
        display_name: Some("BaseOS"),
        supports_update_mode: false,   // Raw disk images always do a full burn
        update_directories: &[],       // Not used for raw images
        allowed_extensions: Some(&[".img.zip"]),  // Excludes .bosupd update packages
        asset_display_mappings: Some(BASEOS_DEVICE_MAPPINGS),
        supports_preserve_mode: false, // No preserve for raw images
    },
];

/// Index of the default repository selection (0 = first option)
pub const DEFAULT_REPO_INDEX: usize = 0;

// ----------------------------------------------------------------------------
// UPDATE PRESERVE PATHS
// ----------------------------------------------------------------------------
// Paths to preserve during update mode (relative to SD card root).
// These are backed up before deletion and restored after installation.
// Can be files or directories. Only used when preserve_data is enabled.
// Mirrors the on-device spruceBackup.sh backup paths.
// ----------------------------------------------------------------------------

pub const UPDATE_PRESERVE_PATHS: &[&str] = &[
    // Emulator configs and saves
    "App/spruceRestore/.lastUpdate",
    "Emu/PICO8/.lexaloffle",
    "Emu/.emu_setup/n64_controller/Custom.rmp",
    "Emu/DC/config",
    "Emu/NDS/backup",
    "Emu/NDS/backup-32",
    "Emu/NDS/backup-64",
    "Emu/NDS/config/drastic-A30.cfg",
    "Emu/NDS/config/drastic-Brick.cfg",
    "Emu/NDS/config/drastic-SmartPro.cfg",
    "Emu/NDS/config/drastic-SmartProS.cfg",
    "Emu/NDS/config/drastic-Flip.cfg",
    "Emu/NDS/config/drastic-Pixel2.cfg",
    "Emu/NDS/savestates",
    "Emu/NDS/resources/settings_A30.json",
    "Emu/NDS/resources/settings_Flip.json",
    "Emu/NDS/resources/settings_Pixel2.json",
    "Emu/SATURN/.yabasanshiro",
    // RetroArch configs (overlays/shaders/cheats not needed — RetroArch/ is no longer deleted)
    "RetroArch/.retroarch/config",
    "RetroArch/platform/retroarch-A30.cfg",
    "RetroArch/platform/retroarch-Brick.cfg",
    "RetroArch/platform/retroarch-Flip.cfg",
    "RetroArch/platform/retroarch-SmartPro.cfg",
    "RetroArch/platform/retroarch-SmartProS.cfg",
    "RetroArch/platform/retroarch-Pixel2.cfg",
    // Network services
    "spruce/bin/Syncthing/config",
    "spruce/etc/ssh/keys",
];

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

// ----------------------------------------------------------------------------
// BOXART SCRAPER CONFIGURATION
// ----------------------------------------------------------------------------

/// System folder mapping for boxart scraper
///
/// Maps ROM folder names to Libretro system names for thumbnail downloads
pub struct SystemMapping {
    /// Folder name on the SD card (e.g., "FC", "nes", "NES")
    pub folder_name: &'static str,
    /// Libretro system name used for thumbnail URLs
    pub libretro_name: &'static str,
    /// Subfolder where boxart images are saved (e.g., "Imgs")
    pub boxart_subfolder: &'static str,
}

pub const SYSTEM_MAPPINGS: &[SystemMapping] = &[
    SystemMapping { folder_name: "AMIGA", libretro_name: "Commodore - Amiga", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "ATARI", libretro_name: "Atari - 2600", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "ATARIST", libretro_name: "Atari - ST", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "ARCADE", libretro_name: "MAME", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "CPS1", libretro_name: "FBNeo - Arcade Games", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "CPS2", libretro_name: "FBNeo - Arcade Games", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "CPS3", libretro_name: "FBNeo - Arcade Games", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "ARDUBOY", libretro_name: "Arduboy Inc - Arduboy", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "CHAI", libretro_name: "ChaiLove", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "COLECO", libretro_name: "Coleco - ColecoVision", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "COMMODORE", libretro_name: "Commodore - 64", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "CPC", libretro_name: "Amstrad - CPC", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "DC", libretro_name: "Sega - Dreamcast", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "DOOM", libretro_name: "DOOM", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "DOS", libretro_name: "DOS", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "EIGHTHUNDRED", libretro_name: "Atari - 8-bit", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "FAIRCHILD", libretro_name: "Fairchild - Channel F", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "FBNEO", libretro_name: "FBNeo - Arcade Games", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "MAME2003PLUS", libretro_name: "MAME", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "FC", libretro_name: "Nintendo - Nintendo Entertainment System", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "FDS", libretro_name: "Nintendo - Family Computer Disk System", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "FIFTYTWOHUNDRED", libretro_name: "Atari - 5200", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "GB", libretro_name: "Nintendo - Game Boy", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "GBA", libretro_name: "Nintendo - Game Boy Advance", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "GBC", libretro_name: "Nintendo - Game Boy Color", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "GG", libretro_name: "Sega - Game Gear", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "GW", libretro_name: "Handheld Electronic Game", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "INTELLIVISION", libretro_name: "Mattel - Intellivision", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "LYNX", libretro_name: "Atari - Lynx", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "MD", libretro_name: "Sega - Mega Drive - Genesis", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "MS", libretro_name: "Sega - Master System - Mark III", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "MSU1", libretro_name: "Nintendo - Super Nintendo Entertainment System", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "MSUMD", libretro_name: "Sega - Mega Drive - Genesis", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "MSX", libretro_name: "Microsoft - MSX", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "N64", libretro_name: "Nintendo - Nintendo 64", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "NDS", libretro_name: "Nintendo - Nintendo DS", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "NEOCD", libretro_name: "SNK - Neo Geo CD", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "NEOGEO", libretro_name: "SNK - Neo Geo", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "NGP", libretro_name: "SNK - Neo Geo Pocket", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "NGPC", libretro_name: "SNK - Neo Geo Pocket Color", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "ODYSSEY", libretro_name: "Magnavox - Odyssey2", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "PCE", libretro_name: "NEC - PC Engine - TurboGrafx 16", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "PCECD", libretro_name: "NEC - PC Engine CD - TurboGrafx-CD", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "POKE", libretro_name: "Nintendo - Pokemon Mini", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "PS", libretro_name: "Sony - PlayStation", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "PSP", libretro_name: "Sony - PlayStation Portable", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "QUAKE", libretro_name: "Quake", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SATELLAVIEW", libretro_name: "Nintendo - Satellaview", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SATURN", libretro_name: "Sega - Saturn", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SCUMMVM", libretro_name: "ScummVM", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SEGACD", libretro_name: "Sega - Mega-CD - Sega CD", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SEGASGONE", libretro_name: "Sega - SG-1000", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SEVENTYEIGHTHUNDRED", libretro_name: "Atari - 7800", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SFC", libretro_name: "Nintendo - Super Nintendo Entertainment System", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SGB", libretro_name: "Nintendo - Game Boy", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SGFX", libretro_name: "NEC - PC Engine SuperGrafx", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SUFAMI", libretro_name: "Nintendo - Sufami Turbo", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "SUPERVISION", libretro_name: "Watara - Supervision", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "THIRTYTWOX", libretro_name: "Sega - 32X", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "TIC", libretro_name: "TIC-80", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "VB", libretro_name: "Nintendo - Virtual Boy", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "VECTREX", libretro_name: "GCE - Vectrex", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "VIC20", libretro_name: "Commodore - VIC-20", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "VIDEOPAC", libretro_name: "Philips - Videopac+", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "WOLF", libretro_name: "Wolfenstein 3D", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "WS", libretro_name: "Bandai - WonderSwan", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "WSC", libretro_name: "Bandai - WonderSwan Color", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "X68000", libretro_name: "Sharp - X68000", boxart_subfolder: "Imgs" },
    SystemMapping { folder_name: "ZXS", libretro_name: "Sinclair - ZX Spectrum", boxart_subfolder: "Imgs" },
];

/// Boxart path and naming configuration
pub struct BoxartConfig {
    /// Image filename pattern - use {game_name} as placeholder
    /// Use ".png" extension for standard PNG output, or ".qoi" when convert_to_qoi is enabled
    /// Examples: "{game_name}.png", "{game_name}.qoi"
    pub image_name_pattern: &'static str,
    /// Include ROM file extension in image name
    /// - false: "Game Name.gb" → "Game Name.png"
    /// - true: "Game Name.gb" → "Game Name.gb.png"
    pub include_extension_in_name: bool,
}

pub const BOXART_CONFIG: BoxartConfig = BoxartConfig {
    image_name_pattern: "{game_name}.qoi",
    include_extension_in_name: false,
};

/// Scraper behavior configuration
pub struct ScraperConfig {
    /// Skip downloading if image already exists
    pub skip_existing: bool,
    /// Automatically create missing boxart directories
    pub create_missing_dirs: bool,
    /// Maximum concurrent downloads (number of worker threads)
    pub max_concurrent_downloads: usize,
    /// Preferred region for tie-breaking (e.g., "USA", "Europe", "Japan")
    pub preferred_region: Option<&'static str>,
    /// Convert downloaded PNG boxart to QOI format
    /// When true, images are decoded, resized to fit within max_image_dimensions,
    /// and re-encoded as QOI before saving. The PNG is never written to disk.
    /// When false, the original PNG is saved as-is.
    /// NOTE: image_name_pattern in BoxartConfig should use ".qoi" extension when this is true
    pub convert_to_qoi: bool,
    /// Maximum image dimensions (width, height) for resized boxart
    /// Images larger than this are shrunk to fit (aspect ratio preserved, never enlarged)
    /// Only used when convert_to_qoi is true. Set to None to skip resizing.
    pub max_image_dimensions: Option<(u32, u32)>,
}

pub const SCRAPER_CONFIG: ScraperConfig = ScraperConfig {
    skip_existing: true,
    create_missing_dirs: true,
    max_concurrent_downloads: 8,
    preferred_region: Some("USA"),
    convert_to_qoi: true,
    max_image_dimensions: Some((640, 480)),
};

// ============================================================================
// THEME SETUP (internal use)
// ============================================================================

/// Initial visuals applied before the first frame.
///
/// The authoritative palette lives in `InstallerApp::get_theme_config`
/// (src/app/theme.rs); this only avoids a flash of egui's default colors at
/// startup. BaseOS is greyscale in both light and dark system themes.
pub fn setup_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = egui::Color32::from_rgb(36, 36, 36);
    visuals.panel_fill = egui::Color32::from_rgb(36, 36, 36);
    visuals.extreme_bg_color = egui::Color32::from_rgb(24, 24, 24);
    visuals.override_text_color = Some(egui::Color32::from_rgb(230, 230, 230));
    ctx.set_visuals(visuals);
}
