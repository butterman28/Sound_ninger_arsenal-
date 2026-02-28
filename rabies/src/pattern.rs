// src/pattern.rs
use eframe::egui;
use crate::gui::NUM_STEPS;
use crate::adsr::ADSREnvelope;
use crate::gui::ChopPlayMode;
use crate::piano_roll::PianoRollNote;

/// Colour palette – one per pattern, cycles
pub const PATTERN_COLORS: &[(u8, u8, u8)] = &[
    (100, 149, 237), // steel-blue
    (80,  200, 120), // green
    (237, 164,  80), // amber
    (180,  80, 200), // purple
    (220,  80,  80), // red
    ( 80, 200, 200), // cyan
    (220, 200,  80), // yellow
    (160, 100, 220), // violet
    (100, 220, 180), // teal
    (240, 120, 160), // pink
];

/// Saved position of a single chop marker
#[derive(Debug, Clone)]
pub struct MarkSnapshot {
    pub position: f32,
}

/// Full state of one drum track, serialisable per pattern
#[derive(Debug, Clone)]
pub struct TrackSnapshot {
    pub file_path: String,
    pub file_name: String,
    pub steps: [bool; NUM_STEPS],
    pub chop_steps: Vec<[bool; NUM_STEPS]>,
    pub adsr: ADSREnvelope,
    pub adsr_enabled: bool,
    pub chop_adsr: Vec<ADSREnvelope>,
    pub chop_adsr_enabled: Vec<bool>,
    pub chop_play_modes: Vec<ChopPlayMode>,
    pub chop_piano_notes: Vec<Vec<PianoRollNote>>,
    pub marks: Vec<MarkSnapshot>,   // chop marker positions (normalised 0-1)
    pub muted: bool,
}

/// A single pattern – the equivalent of one FL Studio "pattern" in the channel rack
#[derive(Debug, Clone)]
pub struct Pattern {
    pub id: usize,
    pub name: String,
    pub color: (u8, u8, u8),
    /// Main-sample chop grid  [step] → [chop_indices]
    pub main_grid: Vec<Vec<usize>>,
    /// Drum-track snapshots (one per track in this pattern)
    pub tracks: Vec<TrackSnapshot>,
    /// Visual length in the song editor (bars)
    pub length_bars: usize,
}

impl Pattern {
    pub fn new(id: usize, name: String) -> Self {
        let color = PATTERN_COLORS[id % PATTERN_COLORS.len()];
        Self {
            id,
            name,
            color,
            main_grid: vec![Vec::new(); NUM_STEPS],
            tracks: Vec::new(),
            length_bars: 1,
        }
    }

    pub fn empty(id: usize) -> Self {
        Self::new(id, format!("Pat {}", id + 1))
    }

    /// Full-brightness egui colour
    pub fn egui_color(&self) -> egui::Color32 {
        egui::Color32::from_rgb(self.color.0, self.color.1, self.color.2)
    }

    /// Dimmed egui colour for inactive blocks
    pub fn egui_color_dim(&self) -> egui::Color32 {
        egui::Color32::from_rgb(
            self.color.0 / 5,
            self.color.1 / 5,
            self.color.2 / 5,
        )
    }

    /// (legacy compat) no-op – we use TrackSnapshot Vec directly
    pub fn ensure_track_count(&mut self, _count: usize) {}
}