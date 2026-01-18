# spruceOS Installer

## Overview

**spruceOS Installer** is an all-in-one **downloader, extractor, formatter, and installer** made for **spruceOS**.

It can be easily edited and adapted to work with **any custom firmware (CFW)** that requires files to be copied onto a **FAT32 SD card**, with little to no hassle.

GitHub Actions are set up to automatically **build and create releases per branch**.  
If you’d like to use this program for your own project, let us know—we can create a branch for you or add you directly to the repository.

> **Please do not remove the Spruce team from the authors section.**  
> Instead, add your name alongside the existing credits.

---

## To-Do

- ~~List supported devices in the version description and widen the dropdown to accommodate longer text~~  
  *(Never mind — this will need to be handled another way)*

- Clean up colors to better match the **SPRUCE** theme

---

# spruceOS Installer — Developer Guide

## Overview

**spruceOS Installer** is an all-in-one Rust installer for flashing SD cards with SpruceOS (or other custom firmware).  
This guide is intended for **developers** who want to **rebrand or customize the installer** for their own OS project.

> **Note:** All builds are handled automatically via **GitHub Actions**.  
> Developers only need to create their **own branch** with the desired customizations — no local build setup is required.

---

## Quick Start — Rebranding the Installer

To adapt this installer for your project, update the following in your branch:

### 1. `src/config.rs` — Core Customization

Edit these constants:

| Field | Purpose | Example |
|-------|---------|---------|
| `APP_NAME` | Display name of your OS (window title, UI) | `"SpruceOS"` |
| `VOLUME_LABEL` | FAT32 SD card label (max 11 chars, uppercase) | `"SPRUCE"` |
| `REPO_OPTIONS` | Array of repositories to fetch | `[("spruceOS Stable", "user/spruceOS")]` |
| `DEFAULT_REPO_INDEX` | Index of the default repo selection | `0` |

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

> **Tip:** Keep the original SpruceOS authors in `Cargo.toml` and `Info.plist` for credit. Add your name alongside theirs.










## Rebranding the Installer

To rebrand the installer for your own project, make the following changes:

### 1. `src/config.rs`

- Update `APP_NAME`  
  _(e.g., `"SPRUCE"`)_

- Update `VOLUME_LABEL`  
  _(e.g., `"SPRUCE"`)_

- Update `REPO_OPTIONS` to point to your GitHub repositories

- Adjust color values to match your project’s theme

---

### 2. `Cargo.toml`

- Change `name`  
  _(e.g., `"yourname-installer"`)_

- Update `description`

---

### 3. `assets/Mac/Info.plist`

- Update:
  - `CFBundleName`
  - `CFBundleDisplayName`
  - `CFBundleIdentifier`

---

### 4. Replace Icons

- `assets/Icons/icon.png`
- `assets/Icons/icon.ico`
