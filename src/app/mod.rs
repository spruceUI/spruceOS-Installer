// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

// Module structure for the installer application
// This breaks down the large app.rs file into focused modules:
//
// - state.rs: Core types (AppState, ProgressInfo, InstallerApp struct) and initialization
// - theme.rs: Theme configuration
// - logic.rs: Installation logic and orchestration
// - ui.rs: UI rendering (eframe::App implementation)

mod state;
mod theme;
mod logic;
mod ui;

// Re-export public types so they can be used by other modules via super::
pub use state::{InstallerApp, AppState, ProgressInfo, get_available_disk_space};
