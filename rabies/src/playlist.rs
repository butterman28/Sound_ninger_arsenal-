// src/playlist.rs
//! Song editor – arranges patterns on a linear timeline,
//! identical to FL Studio's "Playlist / Song Editor".

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use parking_lot::RwLock;

use crate::pattern::Pattern;
use crate::gui::NUM_STEPS;

// ── legacy compat ─────────────────────────────────────────────────────────────
/// Kept so call-sites in gui/mod.rs that aren't migrated yet still compile.
#[derive(Clone, Debug)]
pub struct PlaylistEntry {
    pub pattern_id: usize,
    pub repeats: u32,
}
impl PlaylistEntry {
    pub fn new(pattern_id: usize) -> Self { Self { pattern_id, repeats: 1 } }
}

// ── Song Editor ───────────────────────────────────────────────────────────────

/// The song editor owns all patterns and their timeline arrangement.
///
/// Layout:
///   `arrangement[row][bar]` — row = index into `patterns`, bar = bar number.
///   A `true` means "this pattern plays during this bar".
pub struct SongEditor {
    pub patterns: RwLock<Vec<Pattern>>,
    /// `arrangement[row][bar]`
    pub arrangement: RwLock<Vec<Vec<bool>>>,
    pub total_bars:  RwLock<usize>,

    pub is_playing:          AtomicBool,
    pub current_bar:         AtomicUsize,
    pub current_step_in_bar: AtomicUsize,

    /// Which pattern (row index) the step-sequencer is currently *editing*
    pub active_edit_idx: AtomicUsize,

    next_id: AtomicUsize,
}

impl SongEditor {
    const DEFAULT_BARS: usize = 32;

    pub fn new() -> Self {
        let mut patterns = Vec::new();
        patterns.push(Pattern::empty(0));
        let arrangement = vec![vec![false; Self::DEFAULT_BARS]];
        Self {
            patterns:             RwLock::new(patterns),
            arrangement:          RwLock::new(arrangement),
            total_bars:           RwLock::new(Self::DEFAULT_BARS),
            is_playing:           AtomicBool::new(false),
            current_bar:          AtomicUsize::new(0),
            current_step_in_bar:  AtomicUsize::new(0),
            active_edit_idx:      AtomicUsize::new(0),
            next_id:              AtomicUsize::new(1),
        }
    }

    // ── pattern management ─────────────────────────────────────────────────

    /// Create a new empty pattern and add a row in the arrangement.
    /// Returns the new pattern's *row index* (not its id).
    pub fn create_pattern(&self) -> usize {
        let id    = self.next_id.fetch_add(1, Ordering::Relaxed);
        let total = *self.total_bars.read();

        self.patterns.write().push(Pattern::empty(id));
        self.arrangement.write().push(vec![false; total]);

        self.patterns.read().len() - 1
    }

    pub fn pattern_count(&self) -> usize {
        self.patterns.read().len()
    }

    pub fn get_pattern_by_idx(&self, idx: usize) -> Option<Pattern> {
        self.patterns.read().get(idx).cloned()
    }

    pub fn update_pattern_by_idx(&self, idx: usize, p: Pattern) {
        if let Some(slot) = self.patterns.write().get_mut(idx) { *slot = p; }
    }

    pub fn remove_pattern(&self, idx: usize) {
        {
            let mut ps = self.patterns.write();
            if ps.len() <= 1 { return; }
            if idx < ps.len() { ps.remove(idx); }
        }
        {
            let mut arr = self.arrangement.write();
            if idx < arr.len() { arr.remove(idx); }
        }
        let n = self.patterns.read().len();
        let active = self.active_edit_idx.load(Ordering::Relaxed);
        if active >= n { self.active_edit_idx.store(n.saturating_sub(1), Ordering::Relaxed); }
    }

    pub fn rename_pattern(&self, idx: usize, name: String) {
        if let Some(p) = self.patterns.write().get_mut(idx) { p.name = name; }
    }

    pub fn duplicate_pattern(&self, idx: usize) -> usize {
        let maybe = self.patterns.read().get(idx).cloned();
        if let Some(mut p) = maybe {
            let new_id = self.next_id.fetch_add(1, Ordering::Relaxed);
            p.id   = new_id;
            p.name = format!("{} (copy)", p.name);
            let total = *self.total_bars.read();
            let src_arr = self.arrangement.read().get(idx).cloned().unwrap_or_default();
            self.patterns.write().push(p);
            self.arrangement.write().push(src_arr);
            self.patterns.read().len() - 1
        } else {
            self.create_pattern()
        }
    }

    // ── active-edit index ──────────────────────────────────────────────────

    pub fn active_edit_idx(&self) -> usize {
        self.active_edit_idx.load(Ordering::Relaxed)
    }

    pub fn set_active_edit_idx(&self, idx: usize) {
        self.active_edit_idx.store(idx, Ordering::Relaxed);
    }

    // ── arrangement ────────────────────────────────────────────────────────

    pub fn toggle_block(&self, row: usize, bar: usize) {
        self.ensure_bar_count(bar + 1);
        let mut arr = self.arrangement.write();
        self.ensure_row(&mut arr, row);
        if let Some(r) = arr.get_mut(row) {
            if let Some(cell) = r.get_mut(bar) { *cell = !*cell; }
        }
    }

    pub fn set_block(&self, row: usize, bar: usize, val: bool) {
        self.ensure_bar_count(bar + 1);
        let mut arr = self.arrangement.write();
        self.ensure_row(&mut arr, row);
        if let Some(r) = arr.get_mut(row) {
            if let Some(cell) = r.get_mut(bar) { *cell = val; }
        }
    }

    pub fn get_block(&self, row: usize, bar: usize) -> bool {
        self.arrangement.read()
            .get(row)
            .and_then(|r| r.get(bar))
            .copied()
            .unwrap_or(false)
    }

    pub fn clear_arrangement(&self) {
        for row in self.arrangement.write().iter_mut() {
            for cell in row.iter_mut() { *cell = false; }
        }
    }

    pub fn get_arrangement_snapshot(&self) -> Vec<Vec<bool>> {
        self.arrangement.read().clone()
    }

    pub fn ensure_bar_count(&self, min: usize) {
        let cur = *self.total_bars.read();
        if min > cur {
            *self.total_bars.write() = min;
            for row in self.arrangement.write().iter_mut() {
                while row.len() < min { row.push(false); }
            }
        }
    }

    fn ensure_row(&self, arr: &mut Vec<Vec<bool>>, row: usize) {
        let total = *self.total_bars.read();
        while arr.len() <= row {
            arr.push(vec![false; total]);
        }
    }

    // ── transport ─────────────────────────────────────────────────────────

    pub fn start(&self) {
        self.current_bar.store(0, Ordering::Relaxed);
        self.current_step_in_bar.store(0, Ordering::Relaxed);
        self.is_playing.store(true, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.is_playing.store(false, Ordering::Relaxed);
        self.current_bar.store(0, Ordering::Relaxed);
        self.current_step_in_bar.store(0, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool { self.is_playing.load(Ordering::Relaxed) }

    /// Called once per sequencer tick.
    /// Returns the row-indices whose patterns should play this tick,
    /// plus the current bar index and step-within-bar.
    pub fn advance_song(&self) -> (Vec<usize>, usize, usize) {
        if !self.is_playing() {
            let bar  = self.current_bar.load(Ordering::Relaxed);
            let step = self.current_step_in_bar.load(Ordering::Relaxed);
            return (vec![], bar, step);
        }

        let bar      = self.current_bar.load(Ordering::Relaxed);
        let step     = self.current_step_in_bar.load(Ordering::Relaxed);
        let next_step = (step + 1) % NUM_STEPS;
        self.current_step_in_bar.store(next_step, Ordering::Relaxed);

        if next_step == 0 {
            let total    = *self.total_bars.read();
            let next_bar = (bar + 1) % total.max(1);
            self.current_bar.store(next_bar, Ordering::Relaxed);
        }

        let arr     = self.arrangement.read();
        let active  = arr.iter().enumerate()
            .filter(|(_, row)| row.get(bar).copied().unwrap_or(false))
            .map(|(i, _)| i)
            .collect();

        (active, bar, step)
    }

    // ── legacy compat shims ────────────────────────────────────────────────
    //  Kept so existing call-sites in gui/mod.rs still compile while being
    //  migrated incrementally.

    pub fn get_playlist_status(&self) -> (usize, usize, u32, u32) {
        let bar   = self.current_bar.load(Ordering::Relaxed);
        let total = *self.total_bars.read();
        (bar, total, 0, 1)
    }

    pub fn get_active_pattern_id(&self) -> Option<usize> {
        let idx = self.active_edit_idx();
        self.get_pattern_by_idx(idx).map(|p| p.id)
    }

    /// No-op shim – song mode no longer uses separate advance()
    pub fn advance(&self) -> Option<usize> { None }

    pub fn load_pattern_into_sequencer(
        &self,
        _pattern_id: usize,
        _seq_grid: &Arc<RwLock<Vec<Vec<usize>>>>,
        _drum_tracks: &Arc<RwLock<Vec<crate::gui::DrumTrack>>>,
    ) {
        // Handled by AppState::load_pattern_state() instead
    }

    // Legacy PlaylistManager surface used by a handful of call-sites
    pub fn get_playlist(&self) -> Vec<PlaylistEntry> { vec![] }
    pub fn add_to_playlist(&self, _id: usize) {}
    pub fn clear_playlist(&self) {}
    pub fn create_pattern_legacy(&self, _name: Option<String>) -> usize { self.create_pattern() }
    pub fn get_pattern(&self, id: usize) -> Option<Pattern> {
        self.patterns.read().iter().find(|p| p.id == id).cloned()
    }
    pub fn update_pattern(&self, p: Pattern) {
        let idx_opt = self.patterns.read().iter().position(|x| x.id == p.id);
        if let Some(idx) = idx_opt { self.update_pattern_by_idx(idx, p); }
    }
    pub fn remove_from_playlist(&self, _idx: usize) {}
    pub fn get_all_patterns(&self) -> Vec<Pattern> { self.patterns.read().clone() }
}

/// Re-export so `playlist::PlaylistManager` still resolves in old imports.
pub type PlaylistManager = SongEditor;