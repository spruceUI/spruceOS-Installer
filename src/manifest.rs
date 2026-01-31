// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

use serde::{Deserialize, Serialize};

/// External asset manifest structure for releases hosted outside GitHub
///
/// OS teams can include a manifest.json file in their GitHub release to provide
/// download information for assets hosted on external servers (bypassing GitHub's 2GB limit)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    /// Manifest format version (e.g., "1.0")
    pub version: String,

    /// Optional repository-level display name (e.g., "spruceOS Hotfix 3.3.3")
    /// This overrides the display_name from config.rs for this specific release
    /// Falls back to config.rs display_name if not provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// List of external assets available for download
    pub assets: Vec<ManifestAsset>,
}

/// Individual asset entry in the manifest
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestAsset {
    /// Filename (e.g., "SpruceOS-RK3326.img.gz")
    pub name: String,

    /// Direct download URL for the asset
    pub url: String,

    /// File size in bytes
    pub size: u64,

    /// Optional user-friendly display name (e.g., "RK3326 Chipset")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Optional compatible devices description (e.g., "Anbernic RG351P/V/M, Odroid Go Advance")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<String>,
}
