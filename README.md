## To-Do

- ~~x64 Linux hangs for a sec before ejecting safely, possible bug, it does eject though.~~ idk if this is really fixed it only happens sometimes for me?
- arm64 fails to extract
- inform user of where to find logfile

## Wishlist
- merge twig flasher with rest of spruce version
- checkboxes for various additional packages:
    - all themes
    - all A30 ports
    - all free games
    - PortMaster (are we making this a separate archive?)
- backup and restore (update current installation instead of just fresh ones?)
- scrape boxart for roms
- offline installer / updater mode - provide your own 7z or gz file instead of downloading

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
    ```

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
| `REPO_OPTIONS` | Array of repositories to fetch releases from | `[("Stable", "spruceUI/spruceOS"), ("Nightlies", "spruceUI/spruceOSNightlies")]` |
| `DEFAULT_REPO_INDEX` | Index of the default repo selection (0 = first) | `0` |
| `ASSET_EXTENSION` | File extension to download from releases | `".7z"` or `".zip"` |
| `WINDOW_SIZE` | Default window size (width, height) | `(679.5, 420.0)` |
| `WINDOW_MIN_SIZE` | Minimum window size (width, height) | `(679.5, 420.0)` |

> **Notes:**
> - `WINDOW_TITLE`, `USER_AGENT`, and `TEMP_PREFIX` are auto-generated from `APP_NAME`. You usually **do not need to change these**.
> - The `setup_theme()` function in `config.rs` uses the Gruvbox Dark preset. This is a fallback; the actual theme is customized in `app.rs`.

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

### 4. External Files to Update

To fully rebrand the installer, also update:

- `Cargo.toml` — `name`, `description`, `authors`  
- `assets/Mac/Info.plist` — `CFBundleName`, `CFBundleDisplayName`, `CFBundleIdentifier`  
- `.github/workflows/*.yml` — Artifact names (optional cosmetic change)

---

## Advanced Notes

- **Internal Identifiers** (`WINDOW_TITLE`, `USER_AGENT`, `TEMP_PREFIX`) are auto-generated from `APP_NAME`; modifying them is optional.
- `setup_theme(ctx)` in `config.rs` is a fallback that applies the Gruvbox Dark preset. The actual theme used by the installer is defined in `app.rs` via `get_theme_config()`.
- `REPO_OPTIONS` can include multiple repos (e.g., stable, nightlies, forks). The user can select between them via a dropdown in the UI.
- The installer uses `egui` and `egui_thematic` for the UI. The theme can be edited live using the built-in theme editor (press Ctrl+T in the app).
- All color values in `ThemeConfig` use RGBA format `[R, G, B, A]` where each value is 0-255.

---

## Recommended Workflow for Developers

1. Fork or clone the repository.
2. Create a **new branch** for your customizations (or use an existing branch).
3. **Quick customization** (minimal changes):
   - Update `APP_NAME`, `VOLUME_LABEL`, and `REPO_OPTIONS` in `src/config.rs`
   - Replace `assets/Icons/icon.png` and `icon.ico` with your branding
   - Update `Cargo.toml` and `assets/Mac/Info.plist` with your project info
4. **Full theme customization** (optional):
   - Edit the `get_theme_config()` method in `src/app.rs` (line ~136)
   - Adjust color overrides (RGBA format) to match your brand
   - Search for `Color32::from_rgb` in `app.rs` to customize hardcoded UI colors (Install/Cancel buttons, success messages)
5. Push your branch to GitHub.

> GitHub Actions will automatically build Windows, Linux (x64 + ARM64), and macOS (ARM64 + x64) binaries — **no local build setup required**.

---

> **PLEASE:** Keep the original spruceOS authors in `Cargo.toml` and `Info.plist` for credit. Add your name alongside ours.
