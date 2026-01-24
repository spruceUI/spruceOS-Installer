## To-Do
- The info could be centered vertically in the space below the button instead of straddling it?
- Checkboxes for various additional packages:
    - all themes
    - all A30 ports
    - all free games
    - PortMaster (are we making this a separate archive?)
- Backup and restore (update current installation instead of just fresh ones?)
- Scrape boxart for roms

# SpruceOS Installer

## Overview

**SpruceOS Installer** is an all-in-one **downloader, extractor, formatter, and installer** made for **SpruceOS**.

It can be easily edited and adapted to work with **any custom firmware (CFW)** that requires files to be copied onto a **FAT32 SD card**, with little to no hassle.

GitHub Actions are set up to automatically **build and create releases per branch**.  
If you’d like to use this program for your own project, let us know—we can create a branch for you or add you directly to the repository.

> **Please do not remove the Spruce or NextUI teams from the authors section.**  
> Instead, add your name alongside the existing credits.


## macOS Users

The installer is distributed as a `.zip` containing a self-contained `.app` bundle. No system installation is required.

**Steps to run:**

1. Download the ZIP file from the GitHub release.
2. Extract the ZIP — you will get the following bundle and files:

    ```
    SpruceOS Installer.app/
    ├── Contents/
    │   ├── MacOS/
    │   │   └── spruceos-installer
    │   ├── Info.plist
    │   └── Resources/
    │       └── AppIcon.icns
    └── launch-installer.command (optional launcher script)
    ```

3. **Easy Method:** Double-click `launch-installer.command` to automatically remove quarantine and launch the app
4. **Alternative:** Right-click "SpruceOSInstaller.app" and select "Open", then click "Open" in the dialog

**About Authorization:**

When writing to SD cards, the installer uses macOS's built-in `authopen` utility to request privileged disk access. You'll see a native macOS authorization dialog asking for your admin password. This is normal and required for direct disk writing.

The installer will show specific error messages if authorization fails:
- "Authorization cancelled by user" - You clicked Cancel in the auth dialog
- "Permission denied" - Your account doesn't have admin privileges
- System errors will show detailed diagnostic information

**Note:** This app is not code-signed. For production use, consider signing with an Apple Developer certificate.

# SpruceOS Installer — Developer Guide

## Overview

**SpruceOS Installer** is an all-in-one Rust installer for flashing SD cards with SpruceOS (or other custom firmware).  
This guide is intended for **developers** who want to **rebrand or customize the installer** for their own OS project.

> **Note:** All builds are handled automatically via **GitHub Actions**.  
> Developers only need to create their **own branch** with the desired customizations — no local build setup is required.

---

## Rebranding the Installer

To adapt this installer for your project, update the following in your branch:

### 1. `src/config.rs` — Core Customization

Edit these constants:

| Field | Purpose | Example |
|-------|---------|---------|
| `APP_NAME` | Display name of your OS (window title, UI) | `"SpruceOS"` |
| `VOLUME_LABEL` | FAT32 SD card label (max 11 chars, uppercase) | `"SPRUCEOS"` |
| `REPO_OPTIONS` | Array of repository configurations (see below) | See example below |
| `DEFAULT_REPO_INDEX` | Index of the default repo selection (0 = first) | `0` |
| `WINDOW_SIZE` | Default window size (width, height) | `(679.5, 420.0)` |
| `WINDOW_MIN_SIZE` | Minimum window size (width, height) | `(679.5, 420.0)` |

**Repository Configuration (`REPO_OPTIONS`):**

Each repository is defined using a `RepoOption` struct with the following fields:

| Field | Type | Purpose | Example |
|-------|------|---------|---------|
| `name` | `&str` | Display name shown in the UI button | `"Stable"`, `"Nightlies"` |
| `url` | `&str` | GitHub repository in "owner/repo" format | `"spruceUI/spruceOS"` |
| `info` | `&str` | Description text shown below Install button (use `\n` for line breaks) | `"Stable releases.\nSupported: Device X"` |
| `update_directories` | `&[&str]` | Directories to delete when updating (preserves other files) | `&["Retroarch", "spruce"]` |
| `allowed_extensions` | `Option<&[&str]>` | Filter to only show specific file types (None = show all) | `Some(&[".7z", ".zip"])` |
| `asset_display_mappings` | `Option<&[AssetDisplayMapping]>` | Show user-friendly names instead of filenames | See below |

**Basic Example:**
```rust
pub const REPO_OPTIONS: &[RepoOption] = &[
    RepoOption {
        name: "Stable",
        url: "spruceUI/spruceOS",
        info: "Stable releases of spruceOS.\nSupported devices: Miyoo A30",
        update_directories: &["Retroarch", "spruce"],
        allowed_extensions: Some(&[".7z"]),  // Only show .7z archives
        asset_display_mappings: None,
    },
    RepoOption {
        name: "Nightlies",
        url: "spruceUI/spruceOSNightlies",
        info: "Nightly development builds.\n⚠️ Warning: May be unstable!",
        update_directories: &["Retroarch", "spruce"],
        allowed_extensions: None,  // Show all assets
        asset_display_mappings: None,
    },
];
```

---

### Advanced Repository Features

#### Update Mode (`update_directories`)

Update mode allows users to update an existing installation without losing saves, ROMs, or other personal files. When a user checks "Update Mode" in the UI:

1. The installer **does NOT format** the SD card
2. It **only deletes** the directories specified in `update_directories`
3. All other files (saves, ROMs, screenshots, etc.) are preserved
4. New files are extracted and copied over

**Example:**
```rust
RepoOption {
    name: "Stable",
    url: "spruceUI/spruceOS",
    info: "...",
    update_directories: &["Retroarch", "spruce", "System"],  // These folders will be deleted
    // ROMs, saves, themes in other folders are preserved!
    allowed_extensions: Some(&[".7z"]),
    asset_display_mappings: None,
}
```

**Common Patterns:**
- CFW core files: `&["System", "usr", "bin"]`
- Frontend updates: `&["Retroarch", "spruce", "EmulationStation"]`
- Full refresh: `&["."]` (deletes everything - use with caution!)

#### Extension Filtering (`allowed_extensions`)

Some projects release multiple file types in the same GitHub release (e.g., full OS images + update packages). Use `allowed_extensions` to control which files users see.

**Examples:**

Show only full OS images (hide update packages):
```rust
allowed_extensions: Some(&[".img.gz", ".img.xz"]),
```

Show only archives (hide raw images):
```rust
allowed_extensions: Some(&[".7z", ".zip"]),
```

Show all assets (no filtering):
```rust
allowed_extensions: None,
```

**Use Cases:**
- Prevent users from accidentally selecting update packages when they need full images
- Hide experimental formats (e.g., show only .7z if .zip is for legacy support)
- Simplify UI when releases have many file types

#### Asset Display Mappings (`asset_display_mappings`)

When releases contain technical filenames like `UnofficialOS-RK3326.img.gz`, users may not know which file is for their device. Asset display mappings let you show friendly names instead.

**Example Setup:**
```rust
RepoOption {
    name: "UnofficialOS",
    url: "RetroGFX/UnofficialOS",
    info: "Select your device from the list below.",
    update_directories: &["System", "usr"],
    allowed_extensions: Some(&[".img.gz"]),  // Only show full images
    asset_display_mappings: Some(&[
        AssetDisplayMapping {
            pattern: "RK3326",  // Matches "UnofficialOS-RK3326.img.gz"
            display_name: "RK3326 Chipset",
            devices: "Anbernic RG351P/V/M, Odroid Go Advance/Super",
        },
        AssetDisplayMapping {
            pattern: "RK3588",  // Matches "UnofficialOS-RK3588.img.gz"
            display_name: "RK3588 Chipset",
            devices: "Gameforce Ace, Orange Pi 5, Radxa Rock 5b",
        },
    ]),
}
```

**UI Display:**

Instead of:
```
❯ UnofficialOS-RK3326.img.gz
  UnofficialOS-RK3588.img.gz
```

Users see:
```
❯ RK3326 Chipset
  Compatible: Anbernic RG351P/V/M, Odroid Go Advance/Super

  RK3588 Chipset
  Compatible: Gameforce Ace, Orange Pi 5, Radxa Rock 5b
```

**Pattern Matching:**
- The `pattern` field is checked with `asset.name.contains(pattern)`
- Use unique identifiers from your filenames (chipset names, device codes, etc.)
- Patterns are case-sensitive

**When to Use:**
- Multi-device OS projects with device-specific builds
- Technical filenames that aren't user-friendly
- When you need to explain device compatibility in the UI

**When to Skip:**
- Single-device projects
- Already clear filenames (e.g., "SpruceOS-MiyooA30.7z")
- Releases with only one asset

---

**Asset Detection:**

The installer automatically detects and downloads compatible files from GitHub releases:
- **Archive mode**: `.7z`, `.zip` (formats SD card, extracts, and copies files)
- **Image mode**: `.img.gz`, `.img.xz`, `.img` (burns raw image directly to device)
- **Source code archives** (`Source code.zip`, `Source code.tar.gz`) are automatically filtered out
- Extension filtering (if configured) is applied before showing assets to users
- If multiple assets remain after filtering, users see a selection modal with display names (if configured)

> **Notes:**
> - `WINDOW_TITLE`, `USER_AGENT`, and `TEMP_PREFIX` are auto-generated from `APP_NAME`. You usually **do not need to change these**.
> - The `setup_theme()` function in `config.rs` uses the Gruvbox Dark preset. This is a fallback; the actual theme is customized in `app.rs`.
> - `ASSET_EXTENSION` constant still exists for backward compatibility but is **deprecated** and no longer used.

---

### 2. `src/app.rs` — Theme Colors & UI Customization

The installer's visual theme is defined in the `get_theme_config()` method (around line 136 in `app.rs`). This method returns a `ThemeConfig` with color overrides in **RGBA format** `[R, G, B, A]` (values 0-255).

**Key color fields to customize:**

| Field | Purpose | SpruceOS Default (RGBA) |
|-------|---------|------------------------|
| `override_text_color` | Primary text color | `[251, 241, 199, 255]` (cream) |
| `override_weak_text_color` | Secondary/dimmed text | `[124, 111, 100, 255]` (gray) |
| `override_hyperlink_color` | Clickable links | `[131, 165, 152, 255]` (teal) |
| `override_faint_bg_color` | Input fields, panels | `[48, 48, 48, 255]` (dark gray) |
| `override_extreme_bg_color` | Window background | `[29, 32, 33, 255]` (near black) |
| `override_warn_fg_color` | Warning messages | `[214, 93, 14, 255]` (orange) |
| `override_error_fg_color` | Error messages | `[204, 36, 29, 255]` (red) |
| `override_selection_bg` | Text selection, highlights | `[215, 180, 95, 255]` (gold) |
| `override_widget_inactive_bg_fill` | Inactive buttons | `[215, 180, 95, 255]` (gold) |
| `override_widget_inactive_fg_stroke_color` | Inactive button border | `[104, 157, 106, 255]` (green) |
| `override_widget_hovered_bg_stroke_color` | Hovered button border | `[215, 180, 95, 255]` (gold) |
| `override_widget_active_bg_stroke_color` | Active button border | `[215, 180, 95, 255]` (gold) |

> **Note:** Set a field to `None` to use the default egui value. The theme config has many more fields for fine-grained control — see the full list in the `ThemeConfig` struct.

**Hardcoded UI colors** (also in `app.rs`):
- Line ~1036, 1097: Success message color `Color32::from_rgb(104, 157, 106)` (green)
- Line ~1397: Install button fill `Color32::from_rgb(104, 157, 106)` (green)
- Line ~1417: Cancel button fill `Color32::from_rgb(251, 73, 52)` (red)

To change these, search for `Color32::from_rgb` in `app.rs` and update the RGB values.

---

### 3. Icons

Customize the application icon:

| Icon | Path | Usage |
|------|------|-------|
| PNG | `assets/Icons/icon.png` | Window, title bar (all platforms) |
| ICO | `assets/Icons/icon.ico` | Windows Explorer, taskbar |

> Notes:
> - PNG: Recommended 64x64 or 128x128 with transparency
> - ICO: Multi-resolution preferred (16x16, 32x32, 48x48, 256x256)
> - The icon is loaded via `APP_ICON_PNG` in `config.rs` (requires the `icon` feature enabled)

---

### 4. Custom Font

The installer uses a custom font for all UI text. To use your own font:

**Replace the font file:**
```bash
# Replace the existing font with your own TTF/OTF file
cp /path/to/your/font.ttf assets/Fonts/nunwen.ttf
```

**Update the font configuration in `src/config.rs`:**

| Constant | Purpose | Default |
|----------|---------|---------|
| `CUSTOM_FONT` | Path to the embedded font file | `"../assets/Fonts/nunwen.ttf"` |
| `CUSTOM_FONT_NAME` | Display name for the font (optional, cosmetic) | `"Nunwen"` |

**Example:**
```rust
// If you want to use a different filename:
pub const CUSTOM_FONT: &[u8] = include_bytes!("../assets/Fonts/YourFont.ttf");
pub const CUSTOM_FONT_NAME: &str = "YourFont";
```

> **Notes:**
> - Supports TTF and OTF font formats
> - The font is embedded in the binary, so no external font files are needed at runtime
> - The custom font applies to all UI text (buttons, labels, dropdowns, etc.)
> - To also use the font for monospace text (logs), uncomment the Monospace section in `load_custom_fonts()`

---

### 5. External Files to Update

To fully rebrand the installer, also update:

- `Cargo.toml` — `name`, `description`, `authors`  
- `assets/Mac/Info.plist` — `CFBundleName`, `CFBundleDisplayName`, `CFBundleIdentifier`  
- `.github/workflows/*.yml` — Artifact names (optional cosmetic change)

---

## Advanced Notes

- **Internal Identifiers** (`WINDOW_TITLE`, `USER_AGENT`, `TEMP_PREFIX`) are auto-generated from `APP_NAME`; modifying them is optional.
- `setup_theme(ctx)` in `config.rs` is a fallback that applies the Gruvbox Dark preset. The actual theme used by the installer is defined in `app.rs` via `get_theme_config()`.
- `REPO_OPTIONS` can include multiple repos (e.g., stable, nightlies, forks). The user can select between them via button tabs in the UI. Each repo's `info` text is displayed below the Install button.
- The installer uses `egui` and `egui_thematic` for the UI. The theme can be edited live using the built-in theme editor (press Ctrl+T in the app).
- All color values in `ThemeConfig` use RGBA format `[R, G, B, A]` where each value is 0-255.
- **Asset Selection & Filtering**: When a release contains multiple downloadable files, the installer intelligently handles them:
  - **Source code filtering**: GitHub's auto-generated source archives are always filtered out
  - **Extension filtering**: If `allowed_extensions` is configured, only matching files are shown
  - **Display mappings**: If `asset_display_mappings` is configured, technical filenames are replaced with user-friendly names and device compatibility info
  - **Single asset** → Auto-proceeds to installation
  - **Multiple files with same base name** → Auto-selects by priority (.7z > .zip > .img.gz > .img.xz > .img)
  - **Multiple different files** → Shows selection modal with friendly names (if configured) for user to choose
- **Update Mode**: Users can check "Update Mode" to preserve saves/ROMs while updating system files. Only directories specified in `update_directories` are deleted before installation.

---

## Platform-Specific Implementation Details

### macOS Privileged Disk Access

The installer uses macOS's `authopen` utility for secure, user-approved disk access without requiring the entire application to run as root or be code-signed.

**Key Features:**
- **Error Differentiation**: The installer distinguishes between:
  - User cancellation (user clicked "Cancel" in auth dialog)
  - Permission denial (insufficient privileges)
  - System errors (authopen not found, FD passing failures)
- **File Descriptor Validation**: Validates FD before use to prevent edge cases
- **Detailed Logging**: All authopen operations are logged to the debug log for troubleshooting

**Implementation** (`src/mac/authopen.rs`):
- Uses `AuthOpenError` enum for proper error handling
- Reads FD via stdout parsing (text-based, works without code signing)
- Duplicates FD for safe ownership transfer
- Based on Raspberry Pi Imager's proven patterns

**For Developers**: This approach works perfectly with unsigned apps. For signed apps, consider adding Authorization Services API for credential caching and smoother UX.

### Windows Large FAT32 Formatting

Windows artificially limits the built-in `format` command to 32GB for FAT32, but the filesystem supports up to 2TB. The installer includes a custom FAT32 formatter that writes directly to the physical disk to bypass this limitation.

**Implementation** (`src/fat32.rs`):
- Opens physical disk with `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`
- Writes boot sector, FAT tables, and root directory manually
- Works with drives larger than 32GB without requiring third-party tools

**Note**: A previous implementation using `FSCTL_ALLOW_EXTENDED_DASD_IO` caused "Access denied" errors and was reverted. The current approach is stable and tested.

### Cross-Platform Clipboard

The installer uses the `arboard` crate for reliable clipboard access across all platforms. This replaced the previous egui-based clipboard which was unreliable on macOS.

**Features**:
- Copy debug logs to clipboard with one click
- Works on Windows, macOS, and Linux
- Provides user feedback on success/failure

---

## Recommended Workflow for Developers

### Quick Start (Minimal Customization)

1. Fork or clone the repository.
2. Create a **new branch** for your customizations (or use an existing branch).
3. **Update `src/config.rs`:**
   - Set `APP_NAME` to your OS name (e.g., `"MyOS"`)
   - Set `VOLUME_LABEL` to your SD card label (max 11 chars, e.g., `"MYOS"`)
   - Update `REPO_OPTIONS` with your GitHub repositories:
     ```rust
     pub const REPO_OPTIONS: &[RepoOption] = &[
         RepoOption {
             name: "Stable",
             url: "yourorg/yourrepo",
             info: "Description shown in UI.\nSupported devices: X, Y, Z",
             update_directories: &["System"],  // Folders to delete during updates
             allowed_extensions: None,          // Show all asset types
             asset_display_mappings: None,      // Use filenames as-is
         },
     ];
     ```
4. Replace `assets/Icons/icon.png` and `icon.ico` with your branding
5. **(Optional)** Replace `assets/Fonts/nunwen.ttf` with your custom font
6. Update `Cargo.toml` and `assets/Mac/Info.plist` with your project info
7. Push your branch to GitHub.

> GitHub Actions will automatically build Windows, Linux (x64 + ARM64), and macOS (ARM64 + x64) binaries — **no local build setup required**.

---

### Full Theme Customization (Using the Live Theme Editor)

The installer includes a **built-in theme editor** that lets you customize colors visually and export the theme config directly. This is much faster than manually editing RGBA values in code.

#### Step 1: Build and Run the Installer

First, build the installer locally so you can use the theme editor:

```bash
# Install Rust if you haven't already
# https://rustup.rs/

# Clone and build
git clone https://github.com/spruceUI/spruceOS-Installer.git
cd spruceOS-Installer
cargo run
```

#### Step 2: Open the Theme Editor

With the installer running, press **Ctrl+T** to open the theme editor panel. This will open on the right side of the window.

#### Step 3: Customize Colors Visually

The theme editor provides:
- **Color pickers** for all theme elements (text, backgrounds, borders, buttons, etc.)
- **Live preview** — changes apply immediately to the UI
- **RGBA sliders** for precise color control
- **Preset themes** you can use as starting points

Adjust the colors until you're happy with how the installer looks with your branding.

#### Step 4: Export the Theme Config

At the bottom of the theme editor panel, there's a **"Copy Theme Config"** button (or similar export option). Click it to copy the complete `ThemeConfig` struct to your clipboard.

The copied output will look like this:

```rust
ThemeConfig {
    name: "YourTheme".to_string(),
    dark_mode: true,
    override_text_color: Some([251, 241, 199, 255]),
    override_weak_text_color: Some([124, 111, 100, 255]),
    // ... all other color overrides
}
```

#### Step 5: Paste into Your Code

1. Open `src/app.rs` and find the `get_theme_config()` method (around line 136)
2. Replace the entire `ThemeConfig { ... }` block with your copied config
3. Update the `name` field to match your project name
4. Save the file

#### Step 6: Customize Hardcoded UI Colors (Optional)

Some UI elements use hardcoded colors outside the theme system. Search for `Color32::from_rgb` in `app.rs` to find and update:

- **Line ~1036, 1097**: Success message green `(104, 157, 106)`
- **Line ~1397**: Install button green `(104, 157, 106)`
- **Line ~1417**: Cancel button red `(251, 73, 52)`

Replace the RGB values to match your brand colors.

#### Step 7: Test and Push

```bash
# Test your changes
cargo run

# Commit and push to your branch
git add src/app.rs
git commit -m "Update theme colors for [YourProject]"
git push
```

GitHub Actions will automatically build your customized installer for all platforms.

---

### Tips for Theme Customization

- **Start with a preset**: The theme editor includes several presets (Gruvbox, Solarized, etc.). Pick one close to your brand and adjust from there.
- **Test readability**: Make sure text is readable against backgrounds, especially for secondary text colors.
- **Match your brand**: Use your project's official brand colors for accents, buttons, and highlights.
- **Check all states**: Interact with buttons, dropdowns, and inputs to see hover/active/inactive states.
- **Dark mode only**: The installer currently only supports dark themes. Light theme support is not implemented.

---

> **PLEASE:** Keep the original spruceOS authors in `Cargo.toml` and `Info.plist` for credit. Add your name alongside ours.

---

## Acknowledgments

This project builds upon the excellent work of others in the open source community:

- **[7-Zip](https://www.7-zip.org/)** - We use the 7z compression/decompression engine for extracting installation archives. 7-Zip is licensed under the GNU LGPL license. The 7z binary is bundled with the installer for seamless operation.

- **[Raspberry Pi Imager](https://github.com/raspberrypi/rpi-imager)** - The macOS authorization and privileged disk access implementation is based on techniques from the Raspberry Pi Imager project. Their authopen integration patterns helped us provide secure, user-friendly SD card writing on macOS without requiring code signing.

We're grateful to these projects for making robust, cross-platform tools possible.
