# SpruceOS Installer

## To-Do

- Vertically center info text below Install button
- ~~Refactor app.rs into modular structure~~ ✓ Done
- Checkboxes for additional packages (themes, ports, games)
- Backup and restore functionality
- Scrape boxart for ROMs

---

## Overview

**SpruceOS Installer** is an all-in-one **downloader, extractor, formatter, and installer** for **SpruceOS** and other custom firmware projects.

- ✓ Download releases directly from GitHub
- ✓ Format SD cards (FAT32, supports >32GB on Windows)
- ✓ Extract archives (.7z, .zip) or burn raw images (.img, .img.gz, .img.xz)
- ✓ Cross-platform: Windows, Linux, macOS
- ✓ Update mode: preserve saves/ROMs while updating system files
- ✓ Multi-repository support with asset filtering

GitHub Actions automatically build releases per branch. If you'd like to use this installer for your own CFW project, let us know—we can create a branch for you or add you directly to the repository.

> **Please do not remove the Spruce or NextUI teams from the authors section.**
> Instead, add your name alongside the existing credits.

---

## For End Users

### macOS Users

The installer is distributed as a `.zip` containing a self-contained `.app` bundle.

#### **Initial Setup (First Time Only):**

**IMPORTANT:** macOS requires Terminal to have "Full Disk Access" to write to SD cards. Follow these steps:

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

### Windows/Linux Users

1. Download the installer for your platform
2. Run the executable
3. On Linux, the app will automatically request privileges via `pkexec` if needed

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
**Lines to change:** 28, 32, 40, 99-181, 184

##### **A. App Branding (Lines 28-40)**

```rust
// Line 28 - Your OS name (shown throughout the UI)
pub const APP_NAME: &str = "SpruceOS";  // ← Change to "YourOS"

// Line 32 - SD card volume label (MAX 11 CHARS, UPPERCASE)
pub const VOLUME_LABEL: &str = "SPRUCEOS";  // ← Change to "YOUROS" (11 char max!)

// Line 40 - Window title bar text
pub const WINDOW_TITLE: &str = "SpruceOS Installer";  // ← Change to "YourOS Installer"
```

**⚠️ Warning:** `VOLUME_LABEL` has a **hard 11-character limit** (FAT32 limitation). Use uppercase only.

---

##### **B. GitHub Repositories (Lines 99-181)** ⚠️ CRITICAL

This is where you define which GitHub repos to download from:

```rust
pub const REPO_OPTIONS: &[RepoOption] = &[
    RepoOption {
        name: "Stable",                              // ← Button label in UI
        url: "spruceUI/spruceOS",                   // ← YOUR GitHub repo (owner/repo format)
        info: "Stable releases of SpruceOS.\nSupported devices: Miyoo A30",  // ← Info text (use \n for line breaks)
        update_directories: &["Retroarch", "spruce"],  // ← Folders deleted during updates
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
        update_directories: &["System", "Apps"],  // What gets replaced during updates
        allowed_extensions: None,  // Show all file types
        asset_display_mappings: None,
    },
    RepoOption {
        name: "Beta",
        url: "yourorg/yourrepo-beta",
        info: "Beta builds - may be unstable!\nTesting new features.",
        update_directories: &["System"],
        allowed_extensions: Some(&[".7z", ".zip"]),  // Only show archives
        asset_display_mappings: None,
    },
];
```

---

##### **C. Default Selection (Line 184)**

```rust
// Line 184 - Which repo button is selected by default (0 = first, 1 = second, etc.)
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

##### **F. Advanced: Update Mode**

Update mode preserves user files (saves, ROMs, themes) while replacing system files:

```rust
update_directories: &["Retroarch", "spruce", "System"],  // These get deleted
// Everything else (Roms/, Saves/, etc.) is preserved!
```

**How it works:**
1. User checks "Update Mode" in UI
2. Installer mounts existing SD card (no format!)
3. Only deletes the specified directories
4. Extracts new files
5. User's saves/ROMs stay intact

</details>

---

#### **STEP 2: `Cargo.toml` - Project Metadata**

**Location:** `Cargo.toml`
**Lines:** 2, 5, 6

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
**Lines:** 6, 8, 10, 18

```xml
<!-- Line 6 - Bundle name (no spaces) -->
<key>CFBundleName</key>
<string>SpruceOSInstaller</string>  ← Change to YourOSInstaller

<!-- Line 8 - Display name (shown in Finder) -->
<key>CFBundleDisplayName</key>
<string>SpruceOS Installer</string>  ← Change to "YourOS Installer"

<!-- Line 10 - Bundle identifier (reverse DNS, must be unique) -->
<key>CFBundleIdentifier</key>
<string>com.spruceos.installer</string>  ← Change to com.yourcompany.installer

<!-- Line 18 - Executable name (MUST match binary from Cargo.toml!) -->
<key>CFBundleExecutable</key>
<string>spruceos-installer</string>  ← Change to match Cargo.toml name
```

**⚠️ Critical:** The `CFBundleExecutable` MUST exactly match the `name` field in `Cargo.toml` or macOS won't launch the app!

**⚠️ Important for macOS Users:** Make sure to document in your installer's README that macOS users need to grant Terminal "Full Disk Access" before running the installer (see the macOS Users section above for detailed instructions). This is a macOS security requirement for writing to removable drives.

---

#### **STEP 5: `app.manifest` - Windows UAC Config**

**Location:** `app.manifest` (root directory)
**Lines:** 6, 9

```xml
<!-- Line 6 - Application identifier -->
<assemblyIdentity name="SpruceOS.Installer" ... />
                        ↑ Change to "YourOS.Installer"

<!-- Line 9 - Description (shown in UAC prompt) -->
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

**Most important colors to change:**

```rust
// Line 7 - Theme name (cosmetic)
name: "SpruceOS".to_string(),  // ← Change to your project name

// Line 9 - Primary text color
override_text_color: Some([251, 241, 199, 255]),  // Cream - change to your brand

// Line 13 - Window background
override_extreme_bg_color: Some([29, 32, 33, 255]),  // Dark gray

// Line 24 - Accent/highlight color (selections, checkboxes)
override_selection_bg: Some([215, 180, 95, 255]),  // Gold - your brand color!

// Line 15 - Warning messages
override_warn_fg_color: Some([214, 93, 14, 255]),  // Orange

// Line 16 - Error messages
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
- `override_widget_inactive_fg_stroke_color` - Checkbox/button borders (line 40)
- `override_widget_active_bg_fill` - Checked checkbox background (line 51)
- `override_widget_active_fg_stroke_color` - Checkmark color (line 56)
- `override_widget_hovered_bg_stroke_color` - Hover border (line 45)

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
2. If renaming the file, update `src/config.rs` line 247:
   ```rust
   pub const CUSTOM_FONT_NAME: &str = "YourFont";  // Line 247
   ```

---

#### **STEP 9: GitHub Actions Workflows** (Optional - Cosmetic)

Update artifact names for consistency:

**`.github/workflows/build-windows.yml`:**
- Line 27: Change `spruceos-installer-windows.exe` to `yourname-installer-windows.exe`
- Line 32: Update artifact name

**`.github/workflows/build-macos.yml`:**
- Lines 53, 75, 94: Change `SpruceOSInstaller.app` to `YourOSInstaller.app`
- Line 100: Update artifact name

**`.github/workflows/build-linux.yml`:**
- Lines 18, 23, 28, 33: Update artifact names for all 4 architectures

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
- [ ] "Update Mode" checkbox lists correct directories
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

---

### 🎯 Platform Build Targets

GitHub Actions automatically builds for:

- **Windows:** x64
- **Linux:** x64, ARM64, i686 (32-bit), ARMv7
- **macOS:** Universal binary (Apple Silicon + Intel)

No local build environment needed - just push to GitHub!

---

## Building Locally (Optional)

### Prerequisites
- Rust (via [rustup.rs](https://rustup.rs/))
- Platform-specific dependencies:
  - **Windows:** MSVC build tools
  - **Linux:** Standard build tools
  - **macOS:** Xcode Command Line Tools

### Build Commands

```bash
# Debug build (fast compilation)
cargo build

# Release build (optimized)
cargo build --release --features icon

# Run directly (debug mode)
cargo run
```

**Tips:**
- Press **Ctrl+T** while running to open the theme editor
- Debug builds are in `target/debug/`
- Release builds are in `target/release/`

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
│   ├── state.rs         - AppState enum, InstallerApp struct (~224 lines)
│   ├── theme.rs         - ⚠️ COLORS: Theme configuration (~77 lines)
│   ├── logic.rs         - Installation orchestration (~1,500 lines)
│   └── ui.rs            - ⚠️ COLORS: UI rendering (~900 lines)
├── drives.rs            - Cross-platform drive detection
├── format.rs            - FAT32 formatting (>32GB support on Windows)
├── extract.rs           - 7z extraction with embedded binaries
├── burn.rs              - Raw image burning with SHA256 verification
├── copy.rs              - File copying with progress tracking
├── delete.rs            - Selective directory deletion (update mode)
├── eject.rs             - Safe drive ejection
├── github.rs            - GitHub API integration
├── fat32.rs             - Custom FAT32 formatter (Windows >32GB)
├── debug.rs             - Debug logging to file
└── mac/
    └── authopen.rs      - macOS privileged disk access
```

### Key Features

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
- SHA256 verification (Linux/macOS; Windows incomplete)
- Sector-aligned writes on Windows

**GitHub integration:**
- Fetches latest releases via GitHub API
- Chunked streaming for large downloads
- Rate limit detection and timeout handling
- Automatic filtering of source code archives

**macOS privileged access:**
- Uses native `authopen` utility (no code signing required!)
- Proper error differentiation (cancelled, denied, system error)
- File descriptor duplication for safe ownership transfer

---

## Acknowledgments

This project builds upon excellent open source work:

- **[7-Zip](https://www.7-zip.org/)** - We bundle the 7z binary (LGPL) for seamless archive extraction
- **[Raspberry Pi Imager](https://github.com/raspberrypi/rpi-imager)** - macOS `authopen` integration patterns
