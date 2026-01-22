## To-Do
- Merge twig flasher with rest of spruce version
- Checkboxes for various additional packages:
    - all themes
    - all A30 ports
    - all free games
    - PortMaster (are we making this a separate archive?)
- Backup and restore (update current installation instead of just fresh ones?)
- Scrape boxart for roms
- ~~Offline installer / updater mode - provide your own 7z or gz file instead of downloading~~ (Won't fix, not worth it)

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
- `REPO_OPTIONS` can include multiple repos (e.g., stable, nightlies, forks). The user can select between them via a dropdown in the UI.
- The installer uses `egui` and `egui_thematic` for the UI. The theme can be edited live using the built-in theme editor (press Ctrl+T in the app).
- All color values in `ThemeConfig` use RGBA format `[R, G, B, A]` where each value is 0-255.

---

## Recommended Workflow for Developers

### Quick Start (Minimal Customization)

1. Fork or clone the repository.
2. Create a **new branch** for your customizations (or use an existing branch).
3. Update `APP_NAME`, `VOLUME_LABEL`, and `REPO_OPTIONS` in `src/config.rs`
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
