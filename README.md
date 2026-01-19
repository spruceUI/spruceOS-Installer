---

## To-Do

- Clean up colors to better match the **SPRUCE** theme
- ~~Reverse the download/format order of operations.~~
- Add the same output for all
- Show % for copying and extracting (this might already be a thing)  if possible
- ~~x64 Linux hangs for a sec before ejecting safely, possible bug, it does eject though.~~ idk if this is really fixed it only happens sometimes for me?

---


# spruceOS Installer

## Overview

**spruceOS Installer** is an all-in-one **downloader, extractor, formatter, and installer** made for **spruceOS**.

It can be easily edited and adapted to work with **any custom firmware (CFW)** that requires files to be copied onto a **FAT32 SD card**, with little to no hassle.

GitHub Actions are set up to automatically **build and create releases per branch**.  
If you’d like to use this program for your own project, let us know—we can create a branch for you or add you directly to the repository.

> **Please do not remove the Spruce team from the authors section.**  
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





# spruceOS Installer — Developer Guide

## Overview

**spruceOS Installer** is an all-in-one Rust installer for flashing SD cards with SpruceOS (or other custom firmware).  
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
| `VOLUME_LABEL` | FAT32 SD card label (max 11 chars, uppercase) | `"SPRUCE"` |
| `REPO_OPTIONS` | Array of repositories to fetch | `[("spruceOS Stable", "user/spruceOS")]` |
| `DEFAULT_REPO_INDEX` | Index of the default repo selection | `0` |
| `ASSET_EXTENSION` | File extension to download from releases | `".7z"` or `".zip"` |

> **Notes:**  
> - `WINDOW_TITLE`, `USER_AGENT`, and `TEMP_PREFIX` are auto-generated from `APP_NAME`. You usually **do not need to change these**.

---

### 2. Theme Colors

Customize the installer’s look and feel via RGB colors in `config.rs`:

| Constant | Usage |
|----------|-------|
| `COLOR_BG_DARK` | Main window background |
| `COLOR_BG_MEDIUM` | Panels, input fields |
| `COLOR_BG_LIGHT` | Buttons and interactive elements |
| `COLOR_ACCENT` | Primary accents (headings, highlights, progress bars) |
| `COLOR_ACCENT_DIM` | Hover states, selections |
| `COLOR_TEXT` | Primary text |
| `COLOR_TEXT_DIM` | Secondary labels/text |
| `COLOR_SUCCESS` | Success messages |
| `COLOR_ERROR` | Error messages |
| `COLOR_WARNING` | Warning/destructive alerts |

The theme is applied automatically via `setup_theme(ctx)` in the internal theme setup.

---

### 3. Window Settings

| Constant | Purpose |
|----------|---------|
| `WINDOW_SIZE` | Default window size `(width, height)` |
| `WINDOW_MIN_SIZE` | Minimum window size `(width, height)` |

---

### 4. Icons

Customize the application icon:

| Icon | Path | Usage |
|------|------|-------|
| PNG | `assets/Icons/icon.png` | Window, title bar (all platforms) |
| ICO | `assets/Icons/icon.ico` | Windows Explorer, taskbar |

> Notes:  
> - PNG: Recommended 64x64 or 128x128 with transparency  
> - ICO: Multi-resolution preferred (16x16, 32x32, 48x48, 256x256)  
> - Update `APP_ICON_PNG` path if needed  

> **Important:** Once your branch is pushed, GitHub Actions will automatically build your branch — no manual compilation is required.

---

### 5. External Files to Update

To fully rebrand the installer, also update:

- `Cargo.toml` — `name`, `description`, `authors`  
- `assets/Mac/Info.plist` — `CFBundleName`, `CFBundleDisplayName`, `CFBundleIdentifier`  
- `.github/workflows/*.yml` — Artifact names (optional cosmetic change)

---

## Advanced Notes

- **Internal Identifiers** (`WINDOW_TITLE`, `USER_AGENT`, `TEMP_PREFIX`) are auto-generated; modifying them is optional.  
- `setup_theme(ctx)` configures `egui` visuals for all widgets and windows. Editing it is **only recommended for advanced developers**.  
- `REPO_OPTIONS` can include multiple repos for stable, nightlies, or forks.  

---

## Recommended Workflow for Developers

1. Fork or clone the repository.  
2. Create a **new branch** for your customizations.  
3. Update `APP_NAME`, `VOLUME_LABEL`, and `REPO_OPTIONS`.  
4. Adjust theme colors if desired.  
5. Replace icons and update `Info.plist` / `Cargo.toml`.  
6. Push your branch to GitHub.  

> GitHub Actions will automatically build your branch and generate artifacts — **no local build setup required**.

---

> **PLEASE:** Keep the original spruceOS authors in `Cargo.toml` and `Info.plist` for credit. Add your name alongside ours.
