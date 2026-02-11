# SpruceOS Installer

## To-Do
- Show error in pop-up when an install fails
- Checkboxes for additional packages (themes, ports, games)

---

# Key Features

**Cross-platform drive detection:**
- Windows: `GetLogicalDrives` + `IOCTL_STORAGE_GET_DEVICE_NUMBER`
- Linux: `/sys/block` + `/proc/mounts` + label detection
- macOS: `diskutil list -plist` with multi-heuristic filtering

**FAT32 formatting:**
- Windows: Custom formatter bypasses 32GB OS limit, diskpart partitioning
- Linux: `parted` + `mkfs.vfat`
- macOS: `diskutil eraseDisk` with automatic retry logic

**Raw image burning:**
- On-the-fly `.gz` decompression
- Pre-scans to determine decompressed size
- SHA256 verification (Linux only; disabled on Windows/macOS for reliability)
- Sector-aligned writes (Windows: 512-byte, macOS: 512-byte with F_NOCACHE)
- Direct hardware I/O on macOS (F_NOCACHE + O_SYNC flags prevent buffer cache stalls)

**GitHub integration:**
- Fetches latest releases via GitHub API
- Chunked streaming for large downloads
- Rate limit detection and timeout handling
- Automatic filtering of source code archives

**macOS privileged access:**
- Uses native `authopen` utility (no code signing required!)
- Unix domain socketpair for file descriptor passing (based on Raspberry Pi Imager)
- F_NOCACHE flag bypasses kernel buffer cache for direct hardware writes (prevents 99% freeze)
- O_SYNC flag ensures synchronous writes (data written before returning)
- 512-byte sector-aligned buffering for .gz decompression compatibility
- Proper error differentiation (cancelled, denied, system error)

**Boxart scraper:**
- Fuzzy matching with Levenshtein distance for ROM name matching
- Downloads from Libretro thumbnail database (60+ systems)
- **Arcade MAME XML integration** - Translates ROM codes to display names for accurate matching
- **Embedded MAME database** (4,900+ arcade games) for ARCADE, NEOGEO, CPS1/2/3, FBNEO, MAME2003PLUS
- Configurable paths, naming patterns, and folder structure
- Concurrent downloads with progress tracking (configurable worker count)
- Embedded database for zero runtime I/O (400KB compressed)
- Region-aware tie-breaking for multi-region games

---
## Overview

**SpruceOS Installer** is an all-in-one **downloader, extractor, formatter, and installer** for **SpruceOS** and other custom firmware projects.

- ✓ Download releases directly from GitHub
- ✓ **External asset hosting** via manifest.json (bypass GitHub's 2GB limit)
- ✓ **Parallel downloads** with pause/resume support (8 concurrent connections)
- ✓ Format SD cards (FAT32, supports >32GB on Windows)
- ✓ Extract archives (.7z, .zip) or burn raw images (.img, .img.gz)
- ✓ Cross-platform: Windows, Linux, macOS
- ✓ Update mode: preserve saves/ROMs while updating system files
- ✓ **Preserve user data**: backup/restore emulator configs, RetroArch settings, SSH keys during updates
- ✓ Multi-repository support with asset filtering
- ✓ **Boxart scraper**: Download cover art for ROMs from Libretro database

GitHub Actions automatically build releases per branch. If you'd like to use this installer for your own CFW project, let us know—we can create a branch for you or add you directly to the repository.

> **Please do not remove the Spruce or NextUI teams from the authors section.**
> Instead, add your name alongside the existing credits.

## Authors


- [SpruceOS Team](https://github.com/spruceUI)
- [NextUI Team](https://github.com/LoveRetro)
- [Tag](https://github.com/CMTag)
- [Helaas](https://github.com/Helaas)

---

## External Asset Hosting (manifest.json)

### Bypassing GitHub's 2GB File Limit

GitHub has a 2GB file size limit for release assets. For larger firmware images, the installer supports **external hosting** via a simple JSON manifest file.

**How it works:**
1. Host your large files on any CDN or file server with direct HTTP download URLs
2. Create a `manifest.json` file listing your assets
3. Upload `manifest.json` to your GitHub release
4. The installer automatically detects and uses the external URLs

**Example manifest.json:**
```json
{
  "version": "1.0",
  "display_name": "MyOS Hotfix 3.3.3",
  "assets": [
    {
      "name": "MyOS-Device1.img.gz",
      "url": "https://cdn.example.com/myos-device1.img.gz",
      "size": 3221225472,
      "display_name": "Device Model X",
      "devices": "Compatible with Device X, Y, Z"
    },
    {
      "name": "MyOS-Device2.img.gz",
      "url": "https://cdn.example.com/myos-device2.img.gz",
      "size": 2147483648,
      "display_name": "Device Model Y",
      "devices": "Compatible with Device A, B, C"
    }
  ]
}
```

**Supported hosting services:**
- ✅ CDN services (AWS S3, Cloudflare R2, DigitalOcean Spaces, Backblaze B2)
- ✅ Self-hosted web servers with direct download URLs
- ✅ Any service providing direct HTTP/HTTPS download links
- ❌ MEGA, GoFile, or other services requiring JavaScript/web interface

**Manifest fields:**

**Top-level fields:**
- `version` (required) - Manifest format version (currently "1.0")
- `display_name` (optional) - Repository display name shown in success/error messages (overrides config.rs display_name)
- `assets` (required) - Array of asset objects

**Asset fields:**
- `name` (required) - Filename with extension (determines archive vs. burn mode)
- `url` (required) - Direct HTTP/HTTPS download URL
- `size` (required) - File size in bytes
- `display_name` (optional) - User-friendly name shown in selection UI
- `devices` (optional) - Compatible devices description

**Note:** The installer is fully backward compatible. Repos without `manifest.json` work normally using GitHub release assets.

See `manifest-example.json` in the repository root for a complete example.

---

## Download Pause/Resume

The installer supports pausing and resuming downloads, with automatic progress preservation across sessions.

### Features

**Parallel Downloads:**
- Automatically uses 8 concurrent connections for faster downloads
- Falls back to single connection if server doesn't support HTTP Range requests
- Progress updates show real-time download percentage

**Pause/Resume Controls:**
- **Pause button** (orange) - Appears during active downloads
- **Resume button** - Automatically shown when a partial download is detected
- **Cancel button** (red) - Stops download and deletes partial files

**State Persistence:**
- Download progress is automatically saved to `.partial` files
- Safe to close the app - resume from where you left off on next launch
- Partial files are stored in your system's temp directory
- Cancel cleans up partial files; Pause preserves them

**How it works:**
1. Start a download - it uses 8 parallel chunks for speed
2. Click **Pause** to temporarily stop (progress saved)
3. Close app or leave it open
4. Relaunch installer - **Resume** button appears automatically
5. Click **Resume** to continue from exact byte position
6. Click **Cancel** anytime to abort and delete partial download

**Cross-platform:**
Works identically on Windows, Linux, and macOS with no configuration needed.

---

## For End Users

### Windows/Linux Users

- Download the installer for your platform

- On Linux you will need to mark the app as executable. When launched the app will automatically request privileges via `pkexec` if needed

### macOS Users

The installer is distributed as a `.zip` containing a self-contained `.app` bundle.

#### **Initial Setup (First Time Only):**

**IMPORTANT:** macOS requires Terminal to have "Full Disk Access" to write to SD cards. Follow these steps:

![Mac Full Disc Access](https://github.com/user-attachments/assets/a54aff52-cbad-40ca-a4ec-d826cbc40ede)

**NOT ALL MAC ARE THE SAME, VARIOUS VERSIONS ETC MAY MAKE THE BELOW INSTRUCTIONS DIFFERENT FOR YOU!**

https://support.apple.com/guide/mac-help/open-a-mac-app-from-an-unknown-developer-mh40616/mac

https://ordonez.tv/2024/11/04/how-to-run-unsigned-apps-in-macos-15-1/

1. **Grant Terminal Full Disk Access:**
   - Open **System Settings** (or **System Preferences** on older macOS)
   - Go to **Privacy & Security** → **Full Disk Access**
   - Click the **lock icon** (bottom left) and enter your password
   - Click the **+** button to add an application
   - Navigate to **Applications** → **Utilities** → select **Terminal.app**
   - Check the box next to Terminal in the list
   - **Quit and reopen Terminal** (important!)

   **Why?** macOS security prevents apps from accessing removable drives without this permission. Terminal needs access because it spawns the installer process.

2. **Download and Run the Installer:**
   - Download and extract the ZIP file
   - **Easy method:** Double-click `launch-installer.command` to automatically remove quarantine and launch
   - **Alternative:** Right-click "SpruceOSInstaller.app" and select "Open", then click "Open" in the dialog

3. **Authorization During Install:**
   - When writing to SD cards, you'll see a native macOS authorization dialog requesting your admin password (via `authopen`)
   - This is normal and required for disk operations

#### **Troubleshooting:**

**If the installer can't access your SD card:**
- Verify Terminal has Full Disk Access (see step 1 above)
- **Quit Terminal completely** and reopen it (changes don't apply to running Terminal sessions)
- Try running from Terminal manually:
  ```bash
  cd ~/Downloads/SpruceOSInstaller.app/Contents/MacOS
  ./spruceos-installer
  ```

**Note:** This app is not code-signed.

---

## For Developers: Complete Rebranding Guide

This guide walks you through **every single file** that needs changing to rebrand this installer for your own CFW project.

### 🎯 Quick Start Checklist

**Minimum viable rebrand (~15 minutes):**

- [ ] **1. Edit `src/config.rs`** - Change `APP_NAME`, `VOLUME_LABEL`, `WINDOW_TITLE`, and `REPO_OPTIONS`
- [ ] **2. Edit `Cargo.toml`** - Update `name`, `description`, `authors`
- [ ] **3. Replace `assets/Icons/icon.png` and `icon.ico`** - Your branding
- [ ] **4. Edit `assets/Mac/Info.plist`** - macOS bundle identifiers
- [ ] **5. Edit `app.manifest`** - Windows application name

**Full rebrand with custom theme (~45 minutes):**

- [ ] Complete the 5 steps above
- [ ] **6. Edit `src/app/theme.rs`** - Customize all colors
- [ ] **7. Update `src/app/ui.rs`** - Search for `Color32::from_rgb` and update button colors
- [ ] **8. Test locally** - `cargo build --release --features icon`
- [ ] **9. Push to GitHub** - Automated builds create releases

---

### 📁 Step-by-Step: File Changes

---

#### **STEP 1: `src/config.rs` - Core Configuration** ⚠️ CRITICAL

This is the **most important file** - it controls all branding and functionality.

<details>
<summary><strong>Click to expand detailed instructions</strong></summary>

**Location:** `src/config.rs`

##### **A. App Branding**

Search for these constants in the "BRANDING" section:

```rust
// Your OS name (shown throughout the UI)
pub const APP_NAME: &str = "SpruceOS";  // ← Change to "YourOS"

// SD card volume label (MAX 11 CHARS, UPPERCASE)
pub const VOLUME_LABEL: &str = "SPRUCEOS";  // ← Change to "YOUROS" (11 char max!)

// Window title bar text
pub const WINDOW_TITLE: &str = "SpruceOS Installer";  // ← Change to "YourOS Installer"
```

**⚠️ Warning:** `VOLUME_LABEL` has a **hard 11-character limit** (FAT32 limitation). Use uppercase only.

---

##### **B. GitHub Repositories** ⚠️ CRITICAL

Search for `pub const REPO_OPTIONS` - this is where you define which GitHub repos to download from:

```rust
pub const REPO_OPTIONS: &[RepoOption] = &[
    RepoOption {
        name: "Stable",                              // ← Button label in UI
        url: "spruceUI/spruceOS",                   // ← YOUR GitHub repo (owner/repo format)
        info: "Stable releases of SpruceOS.\nSupported devices: Miyoo A30",  // ← Info text (use \n for line breaks)
        display_name: Some("SpruceOS Stable"),      // ← OPTIONAL: Full name for success/error popups (falls back to name if None)
        supports_update_mode: true,                  // ← Show update mode checkbox (true for archives, false for raw images)
        supports_preserve_mode: true,                // ← Enable preserve/merge of user data during updates
        update_directories: &["Retroarch", "spruce"],  // ← Paths deleted during updates (can be selective)
        allowed_extensions: Some(&[".7z"]),          // ← File types to show (None = all)
        asset_display_mappings: None,                // ← User-friendly names (see advanced below)
    },
    // Add more repos as needed...
];
```

**Example for your project:**

```rust
pub const REPO_OPTIONS: &[RepoOption] = &[
    RepoOption {
        name: "Stable",
        url: "yourorg/yourrepo",  // ← Your GitHub username/repo
        info: "Official stable builds.\nSupported: Device X, Y, Z",
        supports_update_mode: true,   // Archives support updates
        supports_preserve_mode: false, // ← Set false unless you need spruce-specific preserve/merge logic
        update_directories: &["System", "Apps"],  // What gets replaced during updates
        allowed_extensions: None,  // Show all file types
        asset_display_mappings: None,
    },
    RepoOption {
        name: "Beta",
        url: "yourorg/yourrepo-beta",
        info: "Beta builds - may be unstable!\nTesting new features.",
        supports_update_mode: true,   // Archives support updates
        supports_preserve_mode: false, // ← Set false for non-spruce repos
        update_directories: &["System"],
        allowed_extensions: Some(&[".7z", ".zip"]),  // Only show archives
        asset_display_mappings: None,
    },
    RepoOption {
        name: "Raw Images",
        url: "yourorg/yourrepo-images",
        info: "Full disk images for fresh installs only.",
        supports_update_mode: false,  // Raw images (.img.gz) don't support updates
        supports_preserve_mode: false,
        update_directories: &[],  // Not used for raw images
        allowed_extensions: Some(&[".img.gz", ".img"]),  // Only raw images
        asset_display_mappings: None,
    },
];
```

**Clickable Links in Info Text:**

The `info` field supports markdown-style clickable links using the syntax `[text](url)`:

```rust
info: "Official stable builds.\nSupported: Device X, Y, Z\n[View Documentation](https://yourproject.com/docs)\n[Report Issues](https://github.com/yourorg/yourrepo/issues)",
```

- Links will be rendered as clickable hyperlinks in the UI
- Clicking opens the URL in the default browser
- You can have multiple links per line or mix links with regular text
- Use `\n` for line breaks as usual

**Example:**
```rust
RepoOption {
    name: "Stable",
    url: "yourorg/yourrepo",
    info: "Official builds with full support.\n[Documentation](https://docs.example.com) | [Discord](https://discord.gg/example)",
    // ...
},
```

---

##### **C. Default Selection**

Search for `DEFAULT_REPO_INDEX` - which repo button is selected by default:

```rust
// Which repo button is selected by default (0 = first, 1 = second, etc.)
pub const DEFAULT_REPO_INDEX: usize = 0;  // ← Change if needed
```

---

##### **D. Advanced: Asset Display Mappings**

If your releases have technical filenames like `MyOS-RK3326.img.gz`, use display mappings to show user-friendly names:

```rust
asset_display_mappings: Some(&[
    AssetDisplayMapping {
        pattern: "RK3326",  // Matches filenames containing this string
        display_name: "RK3326 Chipset",  // Friendly name shown to users
        devices: "Anbernic RG351P/V/M, Odroid Go Advance",  // Compatible devices
    },
    AssetDisplayMapping {
        pattern: "RK3588",
        display_name: "RK3588 Chipset",
        devices: "Gameforce Ace, Orange Pi 5",
    },
]),
```

**Result:** Users see "RK3326 Chipset - Compatible: Anbernic RG351P/V/M" instead of "MyOS-RK3326.img.gz"

---

##### **E. Advanced: Extension Filtering**

Control which file types users see per repository:

```rust
allowed_extensions: Some(&[".7z", ".zip"]),  // Only archives
allowed_extensions: Some(&[".img.gz"]),       // Only compressed images
allowed_extensions: None,                     // Show everything
```

**Common use cases:**
- Separate "full installer" repos (show only `.7z`) from "update package" repos (show only `.zip`)
- Hide experimental formats from stable releases
- Simplify UI when releases have many file types

---

##### **F. Advanced: Update Mode Control**

The `supports_update_mode` field controls whether the "Update Mode" checkbox appears for a repository:

```rust
supports_update_mode: true,   // Show checkbox - for archive-based installs (.7z, .zip)
supports_update_mode: false,  // Hide checkbox - for raw disk images (.img.gz, .img)
```

**When to use each:**
- **`true`**: Archive files (.7z, .zip) that can be extracted over existing files
- **`false`**: Raw disk images (.img.gz, .img) that always do full disk burns

**⚠️ Important:** Raw disk images ALWAYS wipe the entire drive, so update mode doesn't apply.

---

##### **G. Advanced: Update Mode Directories**

When update mode is enabled (archives only), the paths listed in `update_directories` get deleted before extraction. These can be top-level directories **or** selective subdirectory/file paths:

```rust
// Simple: delete entire top-level directories
update_directories: &["Retroarch", "spruce", "System"],

// Selective: delete specific subdirectories/files within parents
// (preserves user-added content in the parent directory)
update_directories: &["App/SystemApp1", "App/SystemApp2", "Emu/NES", "Emu/SNES"],
```

The SpruceOS repos use a shared constant `SPRUCE_UPDATE_DELETE_PATHS` that lists ~113 selective paths
mirroring the on-device updater's behavior. **Other CFW teams** should define their own list of
directories/files to delete, or use simple top-level directory names.

**How it works:**
1. User checks "Update Mode" checkbox (only visible when `supports_update_mode: true`)
2. Installer mounts existing SD card (no format!)
3. If "Preserve user data" is enabled: backs up user configs to local temp directory
4. Deletes the specified paths (directories are removed recursively, files are removed individually)
5. Extracts and copies new files
6. If "Preserve user data" is enabled: restores backed-up configs to SD card
7. User's saves/ROMs stay intact

</details>

---

#### **STEP 2: `Cargo.toml` - Project Metadata**

**Location:** `Cargo.toml`

Find the `[package]` section and update these fields:

```toml
[package]
name = "spruceos-installer"  # ← Change to "yourname-installer" (lowercase, hyphens only)
version = "1.0.0"
edition = "2021"
description = "SpruceOS SD Card Installer"  # ← Change description
authors = ["SpruceOS Team", "NextUI Team"]  # ← ADD your name (keep credits!)
```

**Example:**

```toml
name = "retrobox-installer"
description = "RetroBox CFW Installer"
authors = ["SpruceOS Team", "NextUI Team", "Your Name <you@example.com>"]
```

**⚠️ Important:** Keep original author credits per project guidelines!

---

#### **STEP 3: Icons - Visual Branding**

**Replace these files with your own:**

| File | Format | Recommended Size | Usage |
|------|--------|------------------|-------|
| `assets/Icons/icon.png` | PNG with transparency | 128x128 or 256x256 | Window icon (all platforms), macOS icon source |
| `assets/Icons/icon.ico` | Multi-resolution ICO | 16x16, 32x32, 48x48, 256x256 | Windows taskbar, file explorer |

**How to create a multi-resolution ICO:**
1. Create PNGs at multiple sizes (16x16, 32x32, 48x48, 256x256)
2. Use online converter (e.g., https://convertio.co/png-ico/) or ImageMagick:
   ```bash
   convert icon-16.png icon-32.png icon-48.png icon-256.png icon.ico
   ```

**⚠️ Common mistakes:**
- PNG without transparency (use RGBA, not RGB)
- Wrong ICO format (must be valid multi-res .ico, not renamed .png)
- Too small (minimum 64x64, recommended 128x128+)

---

#### **STEP 4: `assets/Mac/Info.plist` - macOS Bundle Config**

**Location:** `assets/Mac/Info.plist`

Search for each key and update its corresponding string value:

```xml
<!-- Bundle name (no spaces) -->
<key>CFBundleName</key>
<string>SpruceOSInstaller</string>  ← Change to YourOSInstaller

<!-- Display name (shown in Finder) -->
<key>CFBundleDisplayName</key>
<string>SpruceOS Installer</string>  ← Change to "YourOS Installer"

<!-- Bundle identifier (reverse DNS, must be unique) -->
<key>CFBundleIdentifier</key>
<string>com.spruceos.installer</string>  ← Change to com.yourcompany.installer

<!-- Executable name (MUST match binary from Cargo.toml!) -->
<key>CFBundleExecutable</key>
<string>spruceos-installer</string>  ← Change to match Cargo.toml name

<!-- Permission description shown to users -->
<key>NSSystemAdministrationUsageDescription</key>
<string>This app needs administrator privileges to write firmware images to SD cards.</string>  ← Update to reference your firmware

<!-- Removable volumes permission description -->
<key>NSRemovableVolumesUsageDescription</key>
<string>This app needs access to removable drives to install firmware.</string>  ← Update as needed
```

**⚠️ Critical:** The `CFBundleExecutable` MUST exactly match the `name` field in `Cargo.toml` or macOS won't launch the app!

**⚠️ Important for macOS Users:** Make sure to document in your installer's README that macOS users need to grant Terminal "Full Disk Access" before running the installer (see the macOS Users section above for detailed instructions). This is a macOS security requirement for writing to removable drives.

---

#### **STEP 5: `app.manifest` - Windows UAC Config**

**Location:** `app.manifest` (root directory)

Update these fields:

```xml
<!-- Application identifier -->
<assemblyIdentity name="SpruceOS.Installer" ... />
                        ↑ Change to "YourOS.Installer"

<!-- Description (shown in UAC prompt) -->
<description>SpruceOS SD Card Installer</description>
             ↑ Change to your description
```

This controls how Windows displays your app in:
- UAC (User Account Control) elevation prompts
- Task Manager
- Windows Registry entries

---

#### **STEP 6: `src/app/theme.rs` - Custom Colors** (Optional but Recommended)

**Location:** `src/app/theme.rs`
**All color values are in RGBA format: `[Red, Green, Blue, Alpha]` (0-255)**

<details>
<summary><strong>Click to expand theme customization guide</strong></summary>

##### **Quick Method: Live Theme Editor** 🎨

1. Build and run locally: `cargo run`
2. Press **Ctrl+T** to open the live theme editor
3. Adjust colors visually with color pickers
4. Copy the generated `ThemeConfig` code
5. Paste into `src/app/theme.rs` (replace entire `get_theme_config()` method)

##### **Manual Method: Edit Colors Directly**

Find the `get_theme_config()` method and update the `ThemeConfig` fields:

**Most important colors to change:**

```rust
// Theme name (cosmetic)
name: "SpruceOS".to_string(),  // ← Change to your project name

// Primary text color
override_text_color: Some([251, 241, 199, 255]),  // Cream - change to your brand

// Window background
override_extreme_bg_color: Some([29, 32, 33, 255]),  // Dark gray

// Accent/highlight color (selections, checkboxes)
override_selection_bg: Some([215, 180, 95, 255]),  // Gold - your brand color!

// Warning messages
override_warn_fg_color: Some([214, 93, 14, 255]),  // Orange

// Error messages
override_error_fg_color: Some([204, 36, 29, 255]),  // Red
```

**Full color reference:**

| Field | Current Color | Purpose |
|-------|---------------|---------|
| `override_text_color` | [251, 241, 199, 255] | Main UI text |
| `override_weak_text_color` | [124, 111, 100, 255] | Secondary/dimmed text |
| `override_hyperlink_color` | [131, 165, 152, 255] | Clickable links |
| `override_faint_bg_color` | [48, 48, 48, 255] | Input fields, panels |
| `override_extreme_bg_color` | [29, 32, 33, 255] | Window background |
| `override_warn_fg_color` | [214, 93, 14, 255] | Warning text |
| `override_error_fg_color` | [204, 36, 29, 255] | Error text |
| `override_selection_bg` | [215, 180, 95, 255] | Highlight/accent |

**Button/widget colors:**
- `override_widget_inactive_fg_stroke_color` - Checkbox/button borders
- `override_widget_active_bg_fill` - Checked checkbox background
- `override_widget_active_fg_stroke_color` - Checkmark color
- `override_widget_hovered_bg_stroke_color` - Hover border

</details>

---

#### **STEP 7: `src/app/ui.rs` - Hardcoded Button Colors**

**Location:** `src/app/ui.rs`

Some UI elements use hardcoded colors outside the theme system. Search for `Color32::from_rgb` and update:

```rust
// Success messages (search for "Color32::from_rgb(104, 157, 106)")
Color32::from_rgb(104, 157, 106)  // Green

// Install button (search for install button color)
.fill(egui::Color32::from_rgb(104, 157, 106))  // Green

// Cancel button (search for cancel button color)
.fill(egui::Color32::from_rgb(251, 73, 52))  // Red
```

**How to find them:**
1. Open `src/app/ui.rs`
2. Search for `Color32::from_rgb`
3. Update RGB values to match your brand

---

#### **STEP 8: Fonts** (Optional)

**Location:** `assets/Fonts/nunwen.ttf`

To use a custom font:
1. Replace `assets/Fonts/nunwen.ttf` with your TTF/OTF file
2. If renaming the file, search for `CUSTOM_FONT_NAME` in `src/config.rs` and update it:
   ```rust
   pub const CUSTOM_FONT_NAME: &str = "YourFont";  // ← Change to match your font file
   ```

---

#### **STEP 9: GitHub Actions Workflows** (Optional - Cosmetic)

Update artifact names for consistency (search for the old names and replace):

**`.github/workflows/build-windows.yml`:**
- Search for `spruceos-installer-windows.exe` → Change to `yourname-installer-windows.exe`
- Update the corresponding artifact name

**`.github/workflows/build-macos.yml`:**
- Search for `SpruceOSInstaller.app` → Change to `YourOSInstaller.app`
- Update the corresponding artifact name

**`.github/workflows/build-linux.yml`:**
- Search for `spruceos-installer` → Update artifact names for all 4 architectures

---

#### **STEP 10: Controlling Update Mode** (Optional)

Update mode allows users to preserve ROMs/saves while updating system files. You have several options for controlling this feature:

##### **Option 1: Per-Repository Control (Recommended)**

The `supports_update_mode` field in each `RepoOption` controls whether the update mode checkbox appears:

```rust
RepoOption {
    name: "Stable",
    supports_update_mode: true,   // Show checkbox for archives
    // ...
},
RepoOption {
    name: "Raw Images",
    supports_update_mode: false,  // Hide checkbox for disk images
    // ...
},
```

**When to use:**
- Set `true` for archive-based repositories (.7z, .zip) that support updates
- Set `false` for raw disk images (.img.gz) that always do full burns
- This is automatically configured correctly in the default SpruceOS repos

##### **Option 2: Completely Hide the UI Checkbox**

To disable update mode for ALL repositories, hide the checkbox from users:

1. Open `src/app/ui.rs`
2. Search for `"Update existing installation (skip format)"`
3. Comment out the entire block containing the checkbox
   - Look for the comment `// Update mode checkbox (only show when not in progress AND repo supports it)`
   - Comment from that line through the matching `// END HIDE UPDATE MODE` comment

**Result:** Users won't see the update mode option on any repository.

##### **Option 3: Complete Removal**

For a thorough removal, delete update mode code from these files (search for `update_mode` in each):

**Files to modify:**
- `src/app/state.rs` - Remove the `update_mode: bool` field
- `src/app/ui.rs` - Remove checkbox UI and conditional display logic
- `src/app/logic.rs` - Remove update mode conditional checks
- `src/config.rs` - Optionally remove `update_directories` field from `RepoOption`

**⚠️ Warning:** Option 2 requires more testing. Option 1 is safer and easier to reverse.

##### **Finding Update Mode Code**

All update mode code can be found by searching for:
- `update_mode` (the boolean flag)
- `update_directories` (in config.rs)
- `"Update existing installation"` (the UI text)
- `PreviewingUpdate` (the preview modal state)

Files are marked with `// HIDE UPDATE MODE` comments for easy identification.

---

#### **STEP 11: Boxart Scraper Configuration** (Optional - For ROM Projects)

The boxart scraper downloads cover art for ROM files from Libretro's thumbnail database. All settings are in `src/config.rs`.

<details>
<summary><strong>Click to expand scraper customization guide</strong></summary>

##### **A. System Folder Mappings**

**Location:** `src/config.rs` → `SYSTEM_MAPPINGS`

Maps your ROM folder names to Libretro system names. Customize if your folder structure differs:

```rust
pub const SYSTEM_MAPPINGS: &[SystemMapping] = &[
    SystemMapping {
        folder_name: "FC",           // Your folder name on SD card
        libretro_name: "Nintendo - Nintendo Entertainment System",  // Libretro database name
        boxart_subfolder: "Imgs",    // Where to save downloaded images
    },
    SystemMapping {
        folder_name: "nes",          // Alternative folder name (case-insensitive lookup)
        libretro_name: "Nintendo - Nintendo Entertainment System",
        boxart_subfolder: ".images", // Different boxart location
    },
    // Add/modify entries for your system folders...
];
```

**Common customizations:**
- Change `folder_name` if your OS uses different names (e.g., "nes" instead of "FC", "genesis" instead of "MD")
- Change `boxart_subfolder` if you store images elsewhere (e.g., ".images", "boxart", same folder as ROMs)
- Add new systems if your OS supports additional platforms

**Example for different folder structure:**
```rust
SystemMapping {
    folder_name: "playstation",      // Your custom folder name
    libretro_name: "Sony - PlayStation",  // Libretro name (don't change)
    boxart_subfolder: "covers",       // Your custom boxart location
},
```

##### **B. Image Naming Pattern**

**Location:** `src/config.rs` → `BOXART_CONFIG`

Controls how scraped images are named:

```rust
pub const BOXART_CONFIG: BoxartConfig = BoxartConfig {
    image_name_pattern: "{game_name}.png",     // Pattern with {game_name} placeholder
    include_extension_in_name: false,          // Whether to include ROM extension
};
```

**Pattern examples:**
```rust
image_name_pattern: "{game_name}.png",         // Standard: "Super Mario Bros.png"
image_name_pattern: "{game_name}-image.png",   // EmulationStation style
image_name_pattern: "{game_name}_boxart.png",  // Custom suffix
image_name_pattern: "boxart-{game_name}.png",  // Custom prefix
```

**Extension handling:**

The `include_extension_in_name` flag controls whether `{game_name}` includes the ROM's file extension:

```rust
// Example ROM file: "Super Mario Bros.gb"

include_extension_in_name: false,  // Default
// Result: "Super Mario Bros.png"

include_extension_in_name: true,   // Include extension
// Result: "Super Mario Bros.gb.png"
```

**Use cases:**
- `false` - Most systems (cleaner names, frontend agnostic)
- `true` - Systems where frontend expects extension in boxart name

**Choose based on your frontend's requirements** - different UIs expect different naming conventions.

##### **C. Scraper Behavior Settings**

**Location:** `src/config.rs` → `SCRAPER_CONFIG`

```rust
pub const SCRAPER_CONFIG: ScraperConfig = ScraperConfig {
    skip_existing: true,              // Skip if image already exists (saves bandwidth)
    create_missing_dirs: true,        // Auto-create boxart folders
    max_concurrent_downloads: 8,      // Number of simultaneous downloads
    preferred_region: Some("USA"),    // Region preference for tie-breaking
};
```

**Field descriptions:**
- `skip_existing`: Set `false` to re-download all images (useful for updating scraped art)
- `create_missing_dirs`: Set `false` if you want manual folder management
- `max_concurrent_downloads`: Higher = faster, but may hit rate limits (4-16 recommended)
- `preferred_region`: Options: `"USA"`, `"Europe"`, `"Japan"`, or `None` (no preference)

##### **D. Example: Complete Custom Configuration**

Here's a full example for a hypothetical system with different conventions:

```rust
// System mappings with custom folder names and paths
pub const SYSTEM_MAPPINGS: &[SystemMapping] = &[
    SystemMapping {
        folder_name: "nes",
        libretro_name: "Nintendo - Nintendo Entertainment System",
        boxart_subfolder: ".covers",  // Hidden folder
    },
    SystemMapping {
        folder_name: "genesis",
        libretro_name: "Sega - Mega Drive - Genesis",
        boxart_subfolder: "art",      // Custom folder name
    },
    // ... more systems
];

// Custom paths and naming
pub const BOXART_CONFIG: BoxartConfig = BoxartConfig {
    image_name_pattern: "{game_name}-box.png", // Custom suffix
    include_extension_in_name: true,           // Include ROM extension in image name
};

// Tuned behavior
pub const SCRAPER_CONFIG: ScraperConfig = ScraperConfig {
    skip_existing: false,           // Always re-download
    create_missing_dirs: true,      // Auto-create folders
    max_concurrent_downloads: 4,    // Conservative rate (slower but safer)
    preferred_region: Some("Europe"), // European region preference
};
```

##### **E. Path Resolution Example**

With these settings, here's how paths are built:

**ROM file:** `/mnt/sdcard/games/nes/Super Mario Bros (USA).nes`

**Boxart saved to:** `/mnt/sdcard/games/nes/.covers/Super Mario Bros.nes-box.png`

**Breakdown:**
- `games` - user-selected ROM folder path
- `nes` - from system mapping `folder_name`
- `.covers` - from system mapping `boxart_subfolder`
- `Super Mario Bros.nes-box.png` - from `image_name_pattern` with `include_extension_in_name: true`
  - ROM name: "Super Mario Bros (USA).nes"
  - Extension kept: ".nes"
  - Region stripped: "(USA)" removed
  - Pattern applied: `{game_name}-box.png` → `Super Mario Bros.nes-box.png`

**If `include_extension_in_name: false` (default):**
- Result would be: `/mnt/sdcard/games/nes/.covers/Super Mario Bros-box.png` (no .nes)

##### **F. Adding New Systems**

If your OS supports systems not in the default list, add them to `SYSTEM_MAPPINGS`:

1. Find the Libretro system name from: https://github.com/libretro-thumbnails
2. Add a new `SystemMapping` entry:

```rust
SystemMapping {
    folder_name: "YOUR_FOLDER",           // Your folder name
    libretro_name: "EXACT_LIBRETRO_NAME", // From GitHub repo list
    boxart_subfolder: "Imgs",             // Where to save images
},
```

**Important:** The `libretro_name` must exactly match a repository name from the libretro-thumbnails organization.

##### **G. Arcade Systems & MAME XML Integration**

**Special Handling for Arcade ROMs:**

Arcade systems use MAME-style ROM naming where the ROM filename is a short code (e.g., `mslug.zip`) but the Libretro thumbnail database uses full display names (e.g., `Metal Slug - Super Vehicle-001.png`). The scraper automatically handles this translation using an embedded MAME XML database.

**Supported Arcade Systems:**
- `ARCADE` - Main MAME arcade games
- `NEOGEO` - SNK Neo Geo arcade
- `CPS1`, `CPS2`, `CPS3` - Capcom Play System 1, 2, and 3
- `FBNEO` - FinalBurn Neo arcade games
- `MAME2003PLUS` - MAME 2003-Plus romset

**How It Works:**

1. **ROM File Detection**: Scraper detects arcade system by folder name
2. **XML Lookup**: Translates ROM code to display name using embedded database
   ```
   Example: mslug.zip → "Metal Slug - Super Vehicle-001"
   ```
3. **Fuzzy Matching**: Uses display name to match against Libretro thumbnails
4. **Download**: Downloads the correctly matched boxart

**Database Details:**
- **Location**: `assets/boxartdb/mame_names.xml` (embedded at compile time)
- **Size**: 400KB (stripped, optimized format)
- **Entries**: 4,900+ arcade game mappings
- **Source**: Based on MAME/FBNeo naming conventions

**XML Format** (simplified for optimization):
```xml
<gameList>
  <game>
    <path>./mslug.zip</path>
    <name>Metal Slug - Super Vehicle-001</name>
  </game>
  <!-- ... 4900+ more entries ... -->
</gameList>
```

**Success Rate**: In testing, the arcade scraper achieves ~96-98% success rate on properly named MAME ROMs.

**Behavior for Unknown ROMs**: If a ROM code isn't found in the XML database, the scraper **skips it entirely** rather than attempting fuzzy matching with the ROM code. This prevents incorrect matches and keeps your boxart library clean.

**No Configuration Required**: Arcade XML lookup happens automatically for the supported systems listed above. No special configuration needed in `config.rs`.

##### **H. Testing Your Configuration**

1. Build the installer with your changes
2. Open the scraper UI (🖼 Scrape Boxart button)
3. Select your SD card's ROM folder
4. Pick a test system with a few ROMs
5. Click "Scrape Selected Folders"
6. Verify images appear in the correct location with correct names

</details>

---

#### **STEP 12: Preserve User Data Configuration** (Optional - For Update Mode)

When update mode is enabled, the installer can back up and restore user-specific files across updates. This is **entirely controlled by the `supports_preserve_mode` flag** on each repository — set it to `false` and none of the preserve/merge logic runs.

<details>
<summary><strong>Click to expand preserve mode configuration guide</strong></summary>

##### **A. For Other CFW/OS Teams (Non-Spruce)**

**The simplest approach: set `supports_preserve_mode: false` on all your repos.** This disables the entire preserve system — no backup, no restore, no config merging. Your update mode will simply delete `update_directories` and extract the new release, which is all most projects need.

```rust
RepoOption {
    name: "Stable",
    supports_update_mode: true,
    supports_preserve_mode: false,  // ← No preserve logic runs. Simple delete + extract.
    update_directories: &["System", "Apps"],
    // ...
},
```

When `supports_preserve_mode` is `false`:
- The "Preserve user data" checkbox is hidden from the UI
- No backup or restore operations occur during updates
- The spruce-specific config merge code never executes
- The spruce-specific `SPRUCE_UPDATE_DELETE_PATHS` constant is not used (you define your own `update_directories`)
- `UPDATE_PRESERVE_PATHS` is ignored
- `src/preserve.rs` is never called

**In short: with `supports_preserve_mode: false`, the preserve system is completely inert.** The installer behaves as a straightforward delete-and-extract updater with no spruce-specific behavior.

##### **B. SpruceOS-Specific Preserve System**

The following sections describe the preserve system as configured for SpruceOS. This is only relevant if you set `supports_preserve_mode: true` and want to use or adapt the spruce-specific logic.

##### **C. Per-Repository Control**

The `supports_preserve_mode` field on each `RepoOption` controls whether the "Preserve user data" checkbox appears when update mode is active:

```rust
RepoOption {
    name: "Stable",
    supports_update_mode: true,
    supports_preserve_mode: true,   // Show preserve checkbox in update mode
    // ...
},
RepoOption {
    name: "TwigUI",
    supports_update_mode: false,
    supports_preserve_mode: false,  // No preserve for raw image repos
    // ...
},
```

##### **D. Static Preserve Paths**

**Location:** `src/config.rs` → `UPDATE_PRESERVE_PATHS`

This constant lists paths (relative to SD card root) that get backed up before deletion and blindly restored after installation, overwriting new defaults with the user's existing files:

```rust
pub const UPDATE_PRESERVE_PATHS: &[&str] = &[
    // Emulator configs
    "Emu/PICO8/.lexaloffle",
    "Emu/DC/config",
    "Emu/NDS/backup",
    "Emu/NDS/config/drastic-A30.cfg",
    // RetroArch configs (overlays/shaders/cheats not needed — RetroArch/ is no longer deleted)
    "RetroArch/.retroarch/config",
    // Network services
    "spruce/bin/Syncthing/config",
    "spruce/etc/ssh/keys",
    // Add your custom paths here...
];
```

- Each entry is a path relative to the SD card root
- Can be a file or directory (directories are backed up recursively)
- Paths that don't exist on the SD card are silently skipped

##### **E. Dynamic Config Merge (SpruceOS-Specific)**

In addition to the static backup/restore above, the installer performs **smart config merging** for spruce-specific JSON config files. This mirrors the on-device updater's `merge_configs.py` behavior:

- **Emu configs** (`Emu/*/config.json`): Dynamically discovered at backup time. On restore, the user's `"selected"` values are merged into the new release's config — but only if the selected value still exists in the new config's `"options"` array. This prevents broken references to removed options.
- **Spruce system config** (`Saves/spruce/spruce-config.json`): Same smart merge logic.
- **Theme configs** (`Themes/*/config*.json`): Dynamically discovered and blindly restored (plain copy, no merge).

This logic lives in `src/preserve.rs` (`backup_dynamic_configs()` and `restore_and_merge_configs()`). It only runs when `supports_preserve_mode: true`.

##### **F. Selective Deletion (SpruceOS-Specific)**

The SpruceOS repos use `SPRUCE_UPDATE_DELETE_PATHS` — a shared constant listing ~113 selective paths that mirror the on-device updater's `delete_files.sh`. Instead of deleting entire top-level directories (e.g., all of `App/`), it deletes specific subdirectories and files within each parent, preserving:

| Preserved Item | Why |
|------|------|
| Custom apps in `App/` | User-installed apps like PortMaster, BootLogo |
| Custom `Emu/` folders | User-created emulator folders with custom names |
| `RetroArch/` (entire dir) | User-added overlays, shaders, cheats |
| `spruce/bin`, `spruce/bin64` | PyUI binaries and platform-specific files |

**Other CFW teams** don't use this constant — they define their own `update_directories` list (see STEP 1, Section G).

##### **G. How It Works**

**Update mode with preserve ON (default for spruce repos):**
1. Mount SD card
2. **Static backup**: Copy all `UPDATE_PRESERVE_PATHS` from SD to local temp directory
3. **Dynamic backup**: Scan and copy emu/theme/spruce configs from SD to temp (spruce-specific)
4. **Delete**: Remove `update_directories` paths from SD card
5. **Install**: Extract and copy new release files to SD
6. **Smart merge**: Merge backed-up emu/spruce configs into new release configs (spruce-specific)
7. **Static restore**: Copy remaining backed-up files from temp to SD (overwriting new defaults)
8. Clean up temp backup directory

**Update mode with preserve OFF (hard reset):**
1. Mount SD card
2. **Delete**: Remove `update_directories` paths from SD card
3. **Install**: Extract and copy new release files to SD
4. *(No backup/restore — user gets fresh default configs)*

In both cases, Roms, BIOS, and Saves directories are **not** in `update_directories`, so they are always kept.

##### **H. UI Behavior**

- The "Preserve user data" checkbox only appears when:
  - Update mode is checked
  - The selected repository has `supports_preserve_mode: true`
- The checkbox defaults to ON (checked)
- When the user clicks through to the Update Preview modal:
  - **Preserve ON**: Shows list of preserved data categories
  - **Preserve OFF**: Shows a warning that user configs will be lost
- The checkbox resets to ON after installation completes, errors, or is cancelled

##### **I. Implementation Files**

| File | What it does |
|------|-------------|
| `src/config.rs` | `UPDATE_PRESERVE_PATHS`, `SPRUCE_UPDATE_DELETE_PATHS`, `supports_preserve_mode` field |
| `src/preserve.rs` | Static backup/restore + dynamic config merge (spruce-specific smart merge logic) |
| `src/delete.rs` | Deletes directories and files listed in `update_directories` |
| `src/app/state.rs` | `BackingUp`/`Restoring` app states, `preserve_data` field |
| `src/app/logic.rs` | Wires backup before delete, restore after copy |
| `src/app/ui.rs` | Checkbox UI, preview modal, state wiring |

</details>

---

### 🧪 Testing Your Rebrand

#### **Local Build Test:**

```bash
# Clone your fork/branch
git clone https://github.com/yourorg/yourrepo-installer.git
cd yourrepo-installer

# Build with icon support
cargo build --release --features icon

# Binary location:
# Windows: target/release/yourname-installer.exe
# Linux: target/release/yourname-installer
# macOS: target/release/yourname-installer
```

#### **Verification Checklist:**

- [ ] Window title shows your custom name
- [ ] Icons display correctly (taskbar, window)
- [ ] Repository dropdown shows your repos
- [ ] Colors match your brand
- [ ] Update Mode: If enabled, checkbox lists correct directories; if disabled, checkbox is hidden
- [ ] Preserve Mode: "Preserve user data" checkbox appears when update mode is checked (if repo supports it)
- [ ] Download works from your GitHub repo
- [ ] SD card gets labeled with your `VOLUME_LABEL`
- [ ] macOS: Terminal has Full Disk Access granted (if testing on macOS)
- [ ] macOS: App bundle opens and can access SD card (if testing on macOS)

#### **GitHub Actions Test:**

1. Push changes to GitHub
2. Go to Actions tab
3. Manually trigger "Build All Platforms" workflow
4. Check artifacts:
   - Windows: `yourname-installer-windows.exe`
   - macOS: `YourOS-Installer-macOS-Universal.zip`
   - Linux: 4 binaries for different architectures

---

### ⚠️ Common Pitfalls

| Problem | Cause | Solution |
|---------|-------|----------|
| macOS can't access SD card | Terminal doesn't have Full Disk Access permission | Grant Terminal Full Disk Access in System Settings → Privacy & Security, then quit/reopen Terminal |
| macOS app won't launch | `CFBundleExecutable` doesn't match `Cargo.toml` name | Make them identical |
| Volume label too long | `VOLUME_LABEL` > 11 characters | Shorten to 11 chars max |
| Wrong files in dropdown | GitHub repo URL format wrong | Use "owner/repo" format (no https://) |
| Colors don't apply | Updated `theme.rs` but not `ui.rs` hardcoded colors | Search `Color32::from_rgb` in ui.rs |
| Build fails on GitHub | Binary name changed but workflows not updated | Update `.github/workflows/*.yml` artifact names |
| Icon not showing | PNG doesn't have transparency or wrong format | Use RGBA PNG, valid multi-res ICO |

---

### 📊 Summary: Files Changed

**Critical (must change):**
1. ✅ `src/config.rs` - App name, repos, volume label
2. ✅ `Cargo.toml` - Package metadata
3. ✅ `assets/Icons/` - Both PNG and ICO files
4. ✅ `assets/Mac/Info.plist` - macOS bundle config
5. ✅ `app.manifest` - Windows app identifier

**Recommended (for full rebrand):**
6. ✅ `src/app/theme.rs` - All UI colors
7. ✅ `src/app/ui.rs` - Hardcoded button colors

**Optional (cosmetic/advanced):**
8. ⬜ `assets/Fonts/nunwen.ttf` - Custom font
9. ⬜ `.github/workflows/*.yml` - Artifact names
10. ⬜ `.vscode/launch.json` - Debug config (if using VS Code)
11. ⬜ `src/config.rs` `UPDATE_PRESERVE_PATHS` - Customize backup paths for update mode

---

### 🎯 Platform Build Targets

GitHub Actions automatically builds for:

- **Windows:** x64
- **Linux:** x64, ARM64, i686 (32-bit), ARMv7
- **macOS:** Universal binary (Apple Silicon + Intel)

No local build environment needed - just push to GitHub!

---

**🎨 Developer Note: Theme Editor & Releases**
- Run `cargo run` locally to test changes - Press **Ctrl+T** to open the live theme editor
- Use GitHub Actions to build releases:
  - **"Build All Platforms"** - Creates beta builds for testing (all platforms)
  - **"Release Latest"** - Creates production "latest" release (hides theme button, keeps Ctrl+T)
- No local cross-platform build setup needed - GitHub Actions handles everything!

---

## Architecture Overview

### Module Structure

The installer uses a modular architecture (refactored from a single ~2300 line file):

```
src/
├── main.rs              - Entry point, privilege escalation
├── config.rs            - ⚠️ BRANDING: App name, repos, constants
├── app/                 - Main application (modular)
│   ├── mod.rs           - Module coordinator
│   ├── state.rs         - AppState enum, InstallerApp struct
│   ├── theme.rs         - ⚠️ COLORS: Theme configuration
│   ├── logic.rs         - Installation orchestration
│   └── ui.rs            - ⚠️ COLORS: UI rendering
├── drives.rs            - Cross-platform drive detection
├── format.rs            - FAT32 formatting (>32GB support on Windows)
├── extract.rs           - 7z extraction with embedded binaries
├── burn.rs              - Raw image burning (.img/.gz) with sector alignment
├── copy.rs              - File copying with progress tracking
├── delete.rs            - Selective directory deletion (update mode)
├── preserve.rs          - Backup/restore user data during updates
├── eject.rs             - Safe drive ejection
├── github.rs            - GitHub API integration
├── fat32.rs             - Custom FAT32 formatter (Windows >32GB)
├── debug.rs             - Debug logging to file
├── boxart_scraper.rs    - ⚠️ CONFIG: ROM boxart scraper with fuzzy matching
├── boxart_db.rs         - Embedded Libretro thumbnail database
├── mame_db.rs           - Embedded MAME XML database (arcade ROM code → display name)
└── mac/
    └── authopen.rs      - macOS privileged disk access
```


## Acknowledgments

- **[SpruceOS Team](https://github.com/spruceUI)** - Core development
- **[NextUI Team](https://github.com/LoveRetro)** - Design and GUI enhancements
- **[Tag](https://github.com/CMTag)** - Mac app bundles and so much more!
- **[Helaas](https://github.com/Helaas)** - macOS testing, debugging, and research
- **[7-Zip](https://www.7-zip.org/)** - We bundle the 7z binary (LGPL) for seamless archive extraction
- **[Raspberry Pi Imager](https://github.com/raspberrypi/rpi-imager)** - macOS authopen implementation patterns
- **[balenaEtcher](https://github.com/balena-io/etcher)** - Inspiration and methodology
