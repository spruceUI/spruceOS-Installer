# SpruceOS Installer

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

**Steps to run:**
1. Download and extract the ZIP
2. **Easy method:** Double-click `launch-installer.command` to remove quarantine and launch
3. **Alternative:** Right-click the `.app` and select "Open", then click "Open" in the dialog

When writing to SD cards, you'll see a native macOS authorization dialog requesting your admin password (via `authopen`). This is normal and required.

**Note:** This app is not code-signed.

### Windows/Linux Users

1. Download the installer for your platform
2. Run the executable
3. On Linux, the app will automatically request privileges via `pkexec` if needed

---

## For Developers

### Project Structure

The installer is built in Rust using the `egui` framework. The codebase is modular:

```
src/
├── main.rs              - Entry point, privilege escalation
├── config.rs            - Branding, repositories, theme defaults
├── app/                 - Main application logic (modular)
│   ├── mod.rs           - Module coordinator
│   ├── state.rs         - AppState, InstallerApp struct, initialization
│   ├── theme.rs         - Theme configuration
│   ├── logic.rs         - Installation orchestration, async tasks
│   └── ui.rs            - UI rendering (eframe::App impl)
├── drives.rs            - Cross-platform drive detection
├── format.rs            - FAT32 formatting
├── extract.rs           - 7z extraction
├── burn.rs              - Raw image burning with verification
├── copy.rs              - File copying with progress
├── delete.rs            - Selective directory deletion
├── eject.rs             - Safe drive ejection
├── github.rs            - GitHub API integration
├── fat32.rs             - Custom FAT32 formatter (>32GB Windows)
├── debug.rs             - Debug logging
└── mac/                 - macOS-specific helpers
    └── authopen.rs      - Privileged disk access via authopen
```

### Quick Customization Guide

To rebrand this installer for your own CFW project:

#### 1. **Edit `src/config.rs`** - Branding & Repositories

Update these constants:

| Constant | Purpose | Example |
|----------|---------|---------|
| `APP_NAME` | Your OS name | `"MyOS"` |
| `WINDOW_TITLE` | Window title | `"MyOS Installer"` |
| `VOLUME_LABEL` | SD card label (max 11 chars) | `"MYOS"` |
| `REPO_OPTIONS` | GitHub repositories | See below |

**Repository Configuration:**

```rust
pub const REPO_OPTIONS: &[RepoOption] = &[
    RepoOption {
        name: "Stable",                    // Button label
        url: "yourorg/yourrepo",           // GitHub repo (owner/repo)
        info: "Stable releases.\nSupported: Device X, Y",  // Info text (\n for line breaks)
        update_directories: &["System"],   // Folders deleted during updates
        allowed_extensions: None,          // None = show all assets
        asset_display_mappings: None,      // None = use filenames as-is
    },
];
```

**Advanced: Asset Display Mappings**

If your releases contain technical filenames, use `asset_display_mappings` to show user-friendly names:

```rust
asset_display_mappings: Some(&[
    AssetDisplayMapping {
        pattern: "RK3326",                 // Matches filenames containing this
        display_name: "RK3326 Chipset",    // Friendly name
        devices: "RG351P/V/M, Odroid Go",  // Compatible devices
    },
]),
```

Users will see "RK3326 Chipset" instead of "MyOS-RK3326-v1.2.img.gz".

**Advanced: Extension Filtering**

Control which file types users see:

```rust
allowed_extensions: Some(&[".7z", ".zip"]),  // Only show archives
allowed_extensions: Some(&[".img.gz"]),      // Only show images
allowed_extensions: None,                    // Show all assets
```

**Advanced: Update Mode**

Update mode lets users preserve saves/ROMs while updating system files. Specify which directories to delete:

```rust
update_directories: &["Retroarch", "spruce", "System"],  // These are deleted
// ROMs, saves, themes in other folders are preserved!
```

#### 2. **Edit `src/app/theme.rs`** - Colors

The `get_theme_config()` method defines all UI colors in RGBA format:

```rust
override_text_color: Some([251, 241, 199, 255]),           // Primary text
override_extreme_bg_color: Some([29, 32, 33, 255]),        // Window background
override_selection_bg: Some([215, 180, 95, 255]),          // Highlights/accents
override_error_fg_color: Some([204, 36, 29, 255]),         // Error messages
// ... see src/app/theme.rs for all fields
```

**Pro tip:** Run the installer locally and press **Ctrl+T** to open the live theme editor. Adjust colors visually, then copy the generated `ThemeConfig` into `theme.rs`.

**Hardcoded UI colors** (also in `src/app/ui.rs`):
- Success messages: `Color32::from_rgb(104, 157, 106)` (green)
- Install button: `Color32::from_rgb(104, 157, 106)` (green)
- Cancel button: `Color32::from_rgb(251, 73, 52)` (red)

Search for `Color32::from_rgb` in `ui.rs` to update these.

#### 3. **Replace Icons & Fonts**

| Asset | Path | Usage |
|-------|------|-------|
| PNG icon | `assets/Icons/icon.png` | Window icon (64x64 or 128x128 recommended) |
| ICO icon | `assets/Icons/icon.ico` | Windows taskbar/explorer (multi-res preferred) |
| Font | `assets/Fonts/nunwen.ttf` | Custom UI font (update `config.rs` if renaming) |

#### 4. **Update Metadata**

- `Cargo.toml` - Change `name`, `description`, `authors` (keep original credits!)
- `assets/Mac/Info.plist` - Update `CFBundleName`, `CFBundleDisplayName`, `CFBundleIdentifier`

#### 5. **Push to GitHub**

GitHub Actions will automatically build Windows, Linux (x64 + ARM64), and macOS (ARM64 + x64) binaries. No local build required!

---

## Architecture Overview

### Module Breakdown

**`app/state.rs`** (~224 lines) - Core application state:
- `AppState` enum: Tracks installation progress (Idle, Downloading, Formatting, etc.)
- `InstallerApp` struct: Holds all app state (drives, progress, channels, UI flags)
- `new()`: Initializes app, starts background drive polling
- `get_available_disk_space()`: Cross-platform disk space checking

**`app/theme.rs`** (~77 lines) - Visual customization:
- `get_theme_config()`: Returns `ThemeConfig` with all RGBA color overrides

**`app/logic.rs`** (~1,500 lines) - Installation logic:
- `ensure_selection_valid()`: Drive selection validation
- `fetch_and_check_assets()`: Fetch GitHub releases, filter assets
- `start_installation()`: Main orchestration (download → format → extract → copy OR burn image)
- Asset filtering, auto-selection, cancellation handling
- Platform-specific mount helpers (`get_mount_path_after_format`)

**`app/ui.rs`** (~900 lines) - UI rendering:
- `impl eframe::App for InstallerApp`
- `update()`: Main render loop
- Drive selection dropdown, repository tabs, install button, progress bars
- Modal dialogs: asset selection, update preview, confirmation, completion
- Theme editor integration, debug log panel

### Key Features

**Cross-platform drive detection** (`drives.rs`):
- Windows: `GetLogicalDrives` + `IOCTL_STORAGE_GET_DEVICE_NUMBER`
- Linux: `/sys/block` + `/proc/mounts`
- macOS: `diskutil list -plist` + complex heuristics

**FAT32 formatting** (`format.rs`):
- Windows: Custom formatter for >32GB (bypasses 32GB OS limit), `diskpart` for partitioning
- Linux: `parted` + `mkfs.vfat`
- macOS: `diskutil eraseDisk` with retry logic

**Raw image burning** (`burn.rs`):
- Decompresses `.gz` on-the-fly
- Pre-scans to determine decompressed size
- SHA256 verification (Linux/macOS; Windows verification incomplete)
- Sector-aligned writes on Windows

**GitHub integration** (`github.rs`):
- Fetches latest release from repos
- Chunked streaming for large downloads
- Rate limit detection, timeout handling
- Automatic source code archive filtering

**macOS privileged access** (`mac/authopen.rs`):
- Uses native `authopen` utility (no code signing required)
- Proper error differentiation (cancelled, denied, system error)
- File descriptor duplication for ownership

---

## Building Locally

### Prerequisites
- Rust (via [rustup.rs](https://rustup.rs/))
- Platform-specific dependencies:
  - **Windows:** MSVC build tools
  - **Linux:** Standard build tools
  - **macOS:** Xcode Command Line Tools

### Build
```bash
git clone https://github.com/spruceUI/spruceOS-Installer.git
cd spruceOS-Installer
cargo build --release
```

Executable will be in `target/release/`.

### Development
```bash
cargo run  # Run in debug mode
```

Press **Ctrl+T** in the app to open the live theme editor.

---

## Acknowledgments

This project builds upon excellent open source work:

- **[7-Zip](https://www.7-zip.org/)** - We bundle the 7z binary (LGPL) for seamless archive extraction
- **[Raspberry Pi Imager](https://github.com/raspberrypi/rpi-imager)** - macOS `authopen` integration patterns

---

## To-Do

- ~~Center info text below Install button~~ ✓ Done
- ~~Refactor app.rs into modular structure~~ ✓ Done
- Checkboxes for additional packages (themes, ports, games)
- Backup and restore functionality
- Scrape boxart for ROMs
