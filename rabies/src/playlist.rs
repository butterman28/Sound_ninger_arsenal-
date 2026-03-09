// src/playlist.rs
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use parking_lot::RwLock;
use eframe::egui;

use crate::pattern::Pattern;
use crate::gui::NUM_STEPS;
use crate::audio::{AudioAsset, WaveformAnalysis};

// ── legacy compat ─────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct PlaylistEntry {
    pub pattern_id: usize,
    pub repeats: u32,
}
impl PlaylistEntry {
    pub fn new(pattern_id: usize) -> Self { Self { pattern_id, repeats: 1 } }
}

// ── Song Editor ───────────────────────────────────────────────────────────────
pub struct SongEditor {
    pub patterns: RwLock<Vec<Pattern>>,
    pub arrangement: RwLock<Vec<Vec<Option<usize>>>>,
    pub total_bars:  RwLock<usize>,

    pub is_playing:          AtomicBool,
    pub current_bar:         AtomicUsize,
    pub current_step_in_bar: AtomicUsize,
    pub active_edit_idx:     AtomicUsize,
    next_id:                 AtomicUsize,
}

impl SongEditor {
    const DEFAULT_BARS: usize = 32;

    pub fn new() -> Self {
        let mut patterns = Vec::new();
        patterns.push(Pattern::empty(0));
        let arrangement = vec![vec![None; Self::DEFAULT_BARS]];
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

    pub fn create_pattern(&self) -> usize {
        let id    = self.next_id.fetch_add(1, Ordering::Relaxed);
        let total = *self.total_bars.read();
        self.patterns.write().push(Pattern::empty(id));
        self.arrangement.write().push(vec![None; total]);
        self.patterns.read().len() - 1
    }

    pub fn pattern_count(&self) -> usize { self.patterns.read().len() }

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
            let src_arr = self.arrangement.read().get(idx).cloned().unwrap_or_default();
            self.patterns.write().push(p);
            self.arrangement.write().push(src_arr);
            self.patterns.read().len() - 1
        } else {
            self.create_pattern()
        }
    }

    pub fn active_edit_idx(&self) -> usize {
        self.active_edit_idx.load(Ordering::Relaxed)
    }

    pub fn set_active_edit_idx(&self, idx: usize) {
        self.active_edit_idx.store(idx, Ordering::Relaxed);
    }

    pub fn toggle_block(&self, row: usize, bar: usize) {
        self.ensure_bar_count(bar + 1);
        let mut arr = self.arrangement.write();
        self.ensure_row(&mut arr, row);
        if let Some(r) = arr.get_mut(row) {
            if let Some(cell) = r.get_mut(bar) {
                *cell = if cell.is_some() { None } else { Some(row) };
            }
        }
    }

    pub fn set_block(&self, row: usize, bar: usize, val: Option<usize>) {
        self.ensure_bar_count(bar + 1);
        let mut arr = self.arrangement.write();
        self.ensure_row(&mut arr, row);
        if let Some(r) = arr.get_mut(row) {
            if let Some(cell) = r.get_mut(bar) { *cell = val; }
        }
    }

    pub fn get_block(&self, row: usize, bar: usize) -> Option<usize> {
        self.arrangement.read()
            .get(row).and_then(|r| r.get(bar)).copied().flatten()
    }

    pub fn clear_arrangement(&self) {
        for row in self.arrangement.write().iter_mut() {
            for cell in row.iter_mut() { *cell = None; }
        }
    }

    pub fn get_arrangement_snapshot(&self) -> Vec<Vec<Option<usize>>> {
        self.arrangement.read().clone()
    }

    pub fn ensure_bar_count(&self, min: usize) {
        let cur = *self.total_bars.read();
        if min > cur {
            *self.total_bars.write() = min;
            for row in self.arrangement.write().iter_mut() {
                while row.len() < min { row.push(None); }
            }
        }
    }

    fn ensure_row(&self, arr: &mut Vec<Vec<Option<usize>>>, row: usize) {
        let total = *self.total_bars.read();
        while arr.len() <= row { arr.push(vec![None; total]); }
    }

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

    pub fn advance_song(&self) -> (Vec<usize>, usize, usize) {
        if !self.is_playing.load(Ordering::Relaxed) {
            let bar  = self.current_bar.load(Ordering::Relaxed);
            let step = self.current_step_in_bar.load(Ordering::Relaxed);
            return (vec![], bar, step);
        }

        let bar       = self.current_bar.load(Ordering::Relaxed);
        let step      = self.current_step_in_bar.load(Ordering::Relaxed);
        let next_step = (step + 1) % NUM_STEPS;
        self.current_step_in_bar.store(next_step, Ordering::Relaxed);

        if next_step == 0 {
            let total    = *self.total_bars.read();
            let next_bar = (bar + 1) % total.max(1);
            self.current_bar.store(next_bar, Ordering::Relaxed);
        }

        let arr    = self.arrangement.read();
        let active = arr.iter()
            .filter_map(|row| row.get(bar).copied().flatten())
            .collect();

        (active, bar, step)
    }

    pub fn get_playlist_status(&self) -> (usize, usize, u32, u32) {
        let bar   = self.current_bar.load(Ordering::Relaxed);
        let total = *self.total_bars.read();
        (bar, total, 0, 1)
    }

    pub fn get_active_pattern_id(&self) -> Option<usize> {
        let idx = self.active_edit_idx();
        self.get_pattern_by_idx(idx).map(|p| p.id)
    }

    pub fn advance(&self) -> Option<usize> { None }

    pub fn load_pattern_into_sequencer(
        &self,
        _pattern_id: usize,
        _seq_grid: &Arc<RwLock<Vec<Vec<usize>>>>,
        _drum_tracks: &Arc<RwLock<Vec<crate::gui::DrumTrack>>>,
    ) {}

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

pub type PlaylistManager = SongEditor;

// ── Playlist Audio Tracks ─────────────────────────────────────────────────────

/// A single placed audio clip on a playlist track.
#[derive(Clone)]
pub struct PlaylistAudioClip {
    pub id:        usize,
    pub start_bar: usize,
    pub asset:     Arc<AudioAsset>,
    pub waveform:  Option<WaveformAnalysis>,
}

impl PlaylistAudioClip {
    /// How many bars this clip occupies (4/4 time, given BPM).
    pub fn bar_span(&self, bpm: f32) -> usize {
        let bar_secs = 4.0 * 60.0 / bpm.max(1.0);
        let dur      = self.asset.frames as f32 / self.asset.sample_rate as f32;
        ((dur / bar_secs).ceil() as usize).max(1)
    }
}

const AUDIO_TRACK_COLORS: &[(u8, u8, u8)] = &[
    (80,  180, 255), (80,  220, 140), (240, 160, 60),
    (200, 80,  200), (240, 80,  80),  (80,  220, 220),
    (220, 200, 60),  (160, 100, 220), (100, 220, 180),
    (240, 120, 160),
];

/// One audio track row in the playlist.
pub struct PlaylistAudioTrack {
    pub id:              usize,
    pub name:            String,
    pub color:           (u8, u8, u8),
    pub muted:           bool,
    /// Loaded audio file — used as the "brush" when painting clips.
    pub source_asset:    Option<Arc<AudioAsset>>,
    pub source_waveform: Option<WaveformAnalysis>,
    pub clips:           Vec<PlaylistAudioClip>,
    next_clip_id:        usize,
}

impl PlaylistAudioTrack {
    pub fn new(id: usize) -> Self {
        let color = AUDIO_TRACK_COLORS[id % AUDIO_TRACK_COLORS.len()];
        Self {
            id,
            name:            format!("Audio {}", id + 1),
            color,
            muted:           false,
            source_asset:    None,
            source_waveform: None,
            clips:           Vec::new(),
            next_clip_id:    0,
        }
    }

    pub fn egui_color(&self) -> egui::Color32 {
        egui::Color32::from_rgb(self.color.0, self.color.1, self.color.2)
    }

    /// Place a clip from `source_asset` at `start_bar`, removing overlapping clips.
    /// Returns `false` if no source is loaded.
    pub fn place_clip(&mut self, start_bar: usize, bpm: f32) -> bool {
        let asset = match self.source_asset.clone() { Some(a) => a, None => return false };
        let bar_secs = 4.0 * 60.0 / bpm.max(1.0);
        let dur      = asset.frames as f32 / asset.sample_rate as f32;
        let span     = ((dur / bar_secs).ceil() as usize).max(1);

        self.clips.retain(|c| {
            let cend = c.start_bar + c.bar_span(bpm);
            c.start_bar >= start_bar + span || cend <= start_bar
        });

        let id = self.next_clip_id;
        self.next_clip_id += 1;
        self.clips.push(PlaylistAudioClip {
            id,
            start_bar,
            asset,
            waveform: self.source_waveform.clone(),
        });
        true
    }

    /// Remove any clip whose range covers `bar`.
    pub fn remove_clips_at(&mut self, bar: usize, bpm: f32) {
        self.clips.retain(|c| {
            let end = c.start_bar + c.bar_span(bpm);
            !(c.start_bar <= bar && bar < end)
        });
    }

    /// First clip whose range covers `bar`, or `None`.
    pub fn clip_at_bar(&self, bar: usize, bpm: f32) -> Option<&PlaylistAudioClip> {
        self.clips.iter().find(|c| {
            let end = c.start_bar + c.bar_span(bpm);
            c.start_bar <= bar && bar < end
        })
    }
}