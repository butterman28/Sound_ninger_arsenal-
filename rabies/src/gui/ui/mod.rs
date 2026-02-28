// src/gui/ui/mod.rs

// Declare the submodules
pub mod widgets;
pub mod panels;
pub mod view;
pub mod pattern_playlist;
// ✅ DON'T re-export AppState - it's in crate::gui
// Remove: pub use view::AppState;

// ✅ Re-export widgets helpers for convenience
