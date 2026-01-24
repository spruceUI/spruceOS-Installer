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

// Re-export the main InstallerApp struct
pub use state::InstallerApp;
