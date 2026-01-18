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
