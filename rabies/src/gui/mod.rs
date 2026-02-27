// src/gui/mod.rs
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use parking_lot::RwLock;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SizedSample, FromSample};
use atomic_float::AtomicF32;
use crate::audio::{AudioAsset, AudioManager, WaveformAnalysis};
use crate::samples::{SamplesManager, PlaybackMode};
use crate::adsr::{ADSREnvelope, Voice};
use crate::piano_roll::PianoRollNote; 
use crate::recording::{RecordingManager, RecordingTrack, RecordState};

pub const NUM_STEPS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChopPlayMode {
    ToEnd,
    ToNextChop,
    ToNextStep,
    ToMarker(usize),
}

pub struct DrumTrack {
    pub asset: Arc<AudioAsset>,
    pub waveform: Option<WaveformAnalysis>,
    pub steps: [bool; NUM_STEPS],
    pub chop_steps: Vec<[bool; NUM_STEPS]>,
    pub chop_adsr: Vec<ADSREnvelope>,
    pub chop_adsr_enabled: Vec<bool>,
    pub chop_play_modes: Vec<ChopPlayMode>,
    pub chop_piano_notes: Vec<Vec<PianoRollNote>>,
    pub muted: bool,
    pub adsr: ADSREnvelope,
    pub adsr_enabled: bool,
}

impl DrumTrack {
    pub fn new(asset: Arc<AudioAsset>, waveform: Option<WaveformAnalysis>) -> Self {
        Self {
            asset,
            waveform,
            steps: [false; NUM_STEPS],
            chop_steps: Vec::new(),
            chop_adsr: Vec::new(),
            chop_adsr_enabled: Vec::new(),
            chop_play_modes: Vec::new(),
            chop_piano_notes: Vec::new(),  // ← NEW
            muted: false,
            adsr: ADSREnvelope::default(),
            adsr_enabled: false,
        }
    }

    pub fn ensure_chop_steps(&mut self, needed: usize) {
        while self.chop_steps.len() < needed {
            self.chop_steps.push([false; NUM_STEPS]);
        }
        while self.chop_adsr.len() < needed {
            self.chop_adsr.push(self.adsr);
        }
        while self.chop_adsr_enabled.len() < needed {
            self.chop_adsr_enabled.push(false);
        }
        while self.chop_play_modes.len() < needed {
            self.chop_play_modes.push(ChopPlayMode::ToNextChop);
        }
        while self.chop_piano_notes.len() < needed {  // ← NEW
            self.chop_piano_notes.push(Vec::new());
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WaveformFocus {
    MainSample,
    DrumTrack(usize),
}

pub struct AppState {
    pub audio_manager: Arc<AudioManager>,
    pub samples_manager: Arc<SamplesManager>,
    pub current_asset: Arc<RwLock<Option<Arc<AudioAsset>>>>,
    pub waveform_analysis: Arc<RwLock<Option<WaveformAnalysis>>>,
    pub status: Arc<RwLock<String>>,
    pub(crate) playback_position: Arc<AtomicF32>,
    pub(crate) is_playing: Arc<AtomicBool>,
    pub(crate) stream_handle: Arc<RwLock<Option<cpal::Stream>>>,
    pub(crate) playback_asset: Arc<RwLock<Option<Arc<AudioAsset>>>>,
    pub(crate) playback_sample_index: Arc<AtomicU64>,
    pub(crate) playback_stop_target: Arc<AtomicF32>,
    pub(crate) loading: Arc<AtomicBool>,
    pub(crate) dragged_mark_index: Arc<RwLock<Option<usize>>>,
    pub(crate) selected_from_marker: Arc<RwLock<Option<usize>>>,
    pub(crate) selected_to_marker: Arc<RwLock<Option<usize>>>,
    pub seq_grid: Arc<RwLock<Vec<Vec<usize>>>>,
    pub chop_adsr: Arc<RwLock<Vec<ADSREnvelope>>>,
    pub drum_tracks: Arc<RwLock<Vec<DrumTrack>>>,
    pub(crate) active_voices: Arc<std::sync::Mutex<Vec<Voice>>>,
    pub drum_loading: Arc<AtomicBool>,
    pub seq_bpm: Arc<AtomicF32>,
    pub seq_playing: Arc<AtomicBool>,
    pub seq_current_step: Arc<RwLock<usize>>,
    pub seq_last_step_time: Arc<RwLock<Option<Instant>>>,
    pub(crate) seq_stream_handle: Arc<RwLock<Option<cpal::Stream>>>,
    pub(crate) seq_voice_queue: Arc<std::sync::Mutex<Vec<Voice>>>,
    pub waveform_focus: Arc<RwLock<WaveformFocus>>,
    pub piano_roll_open: Arc<RwLock<bool>>,
    pub piano_roll_chop: Arc<RwLock<Option<(usize, usize)>>>,  // ← NEW: (track_idx, chop_idx)
    pub main_track_index: Arc<RwLock<Option<usize>>>,
    pub rec_manager:        Arc<RecordingManager>,
    pub rec_tracks:         Arc<RwLock<Vec<RecordingTrack>>>,
    /// Which rec_track index is currently recording (if any)
    pub rec_active_track:   Arc<RwLock<Option<usize>>>,
    /// Cached list of input devices (refreshed on demand)
    pub input_devices:      Arc<RwLock<Vec<crate::recording::InputDevice>>>,

}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio_manager: Arc::new(AudioManager::new()),
            active_voices: Arc::new(std::sync::Mutex::new(Vec::new())),
            samples_manager: Arc::new(SamplesManager::new()),
            current_asset: Arc::new(RwLock::new(None)),
            waveform_analysis: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new("Click Load Sample to begin".to_string())),
            playback_stop_target: Arc::new(AtomicF32::new(-1.0)),
            playback_position: Arc::new(AtomicF32::new(0.0)),
            is_playing: Arc::new(AtomicBool::new(false)),
            stream_handle: Arc::new(RwLock::new(None)),
            playback_asset: Arc::new(RwLock::new(None)),
            playback_sample_index: Arc::new(AtomicU64::new(0)),
            loading: Arc::new(AtomicBool::new(false)),
            dragged_mark_index: Arc::new(RwLock::new(None)),
            selected_from_marker: Arc::new(RwLock::new(None)),
            selected_to_marker: Arc::new(RwLock::new(None)),
            seq_grid: Arc::new(RwLock::new(vec![Vec::new(); NUM_STEPS])),
            chop_adsr: Arc::new(RwLock::new(Vec::new())),
            drum_tracks: Arc::new(RwLock::new(Vec::new())),
            drum_loading: Arc::new(AtomicBool::new(false)),
            seq_bpm: Arc::new(AtomicF32::new(120.0)),
            seq_playing: Arc::new(AtomicBool::new(false)),
            seq_current_step: Arc::new(RwLock::new(0)),
            seq_last_step_time: Arc::new(RwLock::new(None)),
            seq_stream_handle: Arc::new(RwLock::new(None)),
            seq_voice_queue: Arc::new(std::sync::Mutex::new(Vec::new())),
            waveform_focus: Arc::new(RwLock::new(WaveformFocus::MainSample)),
            piano_roll_open: Arc::new(RwLock::new(false)),
            piano_roll_chop: Arc::new(RwLock::new(None)),
            main_track_index: Arc::new(RwLock::new(None)),
            rec_manager:      Arc::new(RecordingManager::new()),
            rec_tracks:       Arc::new(RwLock::new(Vec::new())),
            rec_active_track: Arc::new(RwLock::new(None)),
            input_devices:    Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl AppState {
    pub fn start_playback(&self, asset: Arc<AudioAsset>) {
        self.stop_playback();
        *self.playback_asset.write() = Some(asset.clone());
        let start_pos = self.playback_position.load(Ordering::Relaxed);
        let stop_target = match self.samples_manager.get_playback_mode() {
            PlaybackMode::PlayToEnd => -1.0,
            PlaybackMode::PlayToNextMarker => self.samples_manager.get_playback_target(start_pos, &asset.file_name).unwrap_or(-1.0),
            PlaybackMode::CustomRegion { region_id } => {
                if let Some(region) = self.samples_manager.get_region_by_id(region_id) {
                    if let Some(from_mark) = self.samples_manager.get_mark_by_id(region.from) {
                        if from_mark.sample_name == asset.file_name {
                            self.playback_position.store(from_mark.position, Ordering::Relaxed);
                            let sp = (from_mark.position as f64 * asset.pcm.len() as f64) as u64;
                            self.playback_sample_index.store(sp, Ordering::Relaxed);
                        }
                    }
                    self.samples_manager.get_mark_by_id(region.to).map(|m| m.position).unwrap_or(-1.0)
                } else { -1.0 }
            }
        };
        let stop_target = if stop_target >= 0.0 && start_pos >= stop_target { -1.0 } else { stop_target };
        self.playback_stop_target.store(stop_target, Ordering::Relaxed);
        self.is_playing.store(true, Ordering::Relaxed);

        let host   = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => { *self.status.write() = "No audio output device".to_string(); self.is_playing.store(false, Ordering::Relaxed); return; }
        };
        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => { *self.status.write() = format!("Audio config error: {}", e); self.is_playing.store(false, Ordering::Relaxed); return; }
        };

        let args = StreamArgs {
            channels: asset.channels, pcm: asset.pcm.clone(),
            position: self.playback_position.clone(), sample_index: self.playback_sample_index.clone(),
            is_playing: self.is_playing.clone(), total_samples: asset.pcm.len() as u64,
            status: self.status.clone(), stop_target: self.playback_stop_target.clone(),
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config.into(), args),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config.into(), args),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config.into(), args),
            _ => { *self.status.write() = "Unsupported sample format".to_string(); self.is_playing.store(false, Ordering::Relaxed); return; }
        };

        match stream {
            Ok(s) => {
                if let Err(e) = s.play() { *self.status.write() = format!("Playback error: {}", e); self.is_playing.store(false, Ordering::Relaxed); }
                else { *self.stream_handle.write() = Some(s); *self.status.write() = format!("Playing: {}", asset.file_name); }
            }
            Err(e) => { *self.status.write() = format!("Stream error: {}", e); self.is_playing.store(false, Ordering::Relaxed); }
        }
    }

    pub fn stop_playback(&self) {
        self.is_playing.store(false, Ordering::Relaxed);
        *self.stream_handle.write() = None;
        *self.playback_asset.write() = None;
    }

    pub fn toggle_playback(&self) {
        let asset_to_play = {
            let focus = self.waveform_focus.read();
            match &*focus {
                WaveformFocus::MainSample => self.current_asset.read().clone(),
                WaveformFocus::DrumTrack(idx) => {
                    let tracks = self.drum_tracks.read();
                    tracks.get(*idx).map(|t| t.asset.clone())
                }
            }
        };

        if let Some(asset) = asset_to_play {
            if self.is_playing.load(Ordering::Relaxed) {
                self.is_playing.store(false, Ordering::Relaxed);
                *self.status.write() = format!("Paused: {}", asset.file_name);
            } else {
                if let PlaybackMode::CustomRegion { region_id } = self.samples_manager.get_playback_mode() {
                    if let Some(region) = self.samples_manager.get_region_by_id(region_id) {
                        if let Some(mark) = self.samples_manager.get_mark_by_id(region.from) {
                            if mark.sample_name == asset.file_name {
                                self.playback_position.store(mark.position, Ordering::Relaxed);
                                let sp = (mark.position as f64 * asset.pcm.len() as f64) as u64;
                                self.playback_sample_index.store(sp, Ordering::Relaxed);
                            }
                        }
                    }
                } else if self.playback_position.load(Ordering::Relaxed) >= 0.999 {
                    self.playback_position.store(0.0, Ordering::Relaxed);
                    self.playback_sample_index.store(0, Ordering::Relaxed);
                }
                self.start_playback(asset);
            }
        }
    }

    pub fn seek_to(&self, normalized_pos: f32) {
        if let Some(asset) = self.current_asset.read().as_ref() {
            let was_playing = self.is_playing.load(Ordering::Relaxed);
            self.is_playing.store(false, Ordering::Relaxed);
            let sp = (normalized_pos as f64 * asset.pcm.len() as f64) as usize;
            self.playback_position.store(normalized_pos, Ordering::Relaxed);
            self.playback_sample_index.store(sp.min(asset.pcm.len()) as u64, Ordering::Relaxed);
            let dur = asset.frames as f32 / asset.sample_rate as f32;
            *self.status.write() = format!("Seeked to {:.2}s / {:.2}s", normalized_pos * dur, dur);
            if was_playing { self.start_playback(asset.clone()); }
        }
    }

    pub fn load_sample_as_track(&self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3","wav","flac","ogg","m4a","aac"])
            .pick_file()
        {   
            let audio_manager    = self.audio_manager.clone();
            let drum_tracks      = self.drum_tracks.clone();
            let drum_loading     = self.drum_loading.clone();
            let status           = self.status.clone();
            let waveform_focus   = self.waveform_focus.clone();
            let main_track_index = self.main_track_index.clone();
            let waveform_analysis = self.waveform_analysis.clone();

            drum_loading.store(true, Ordering::Relaxed);
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    audio_manager.load_audio(path.to_str().unwrap_or(""))
                }));
                match result {
                    Ok(Ok(asset)) => {
                        let waveform  = audio_manager.analyze_waveform(&asset, 400);
                        let track     = DrumTrack::new(asset.clone(), Some(waveform.clone()));
                        let track_idx = {
                            let mut tracks = drum_tracks.write();
                            tracks.push(track);
                            tracks.len() - 1
                        };
                        *waveform_focus.write()    = WaveformFocus::DrumTrack(track_idx);
                        *waveform_analysis.write() = Some(waveform);
                        *main_track_index.write()  = Some(track_idx);
                        *status.write() = format!("✓ Track loaded: {}", asset.file_name);
                    }
                    Ok(Err(e)) => { *status.write() = format!("✗ Track load error: {}", e); }
                    Err(_)     => { *status.write() = "✗ Track load crashed".to_string(); }
                }
                drum_loading.store(false, Ordering::Relaxed);
            });
        }
    }

    pub fn load_drum_track(&self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3","wav","flac","ogg","m4a","aac"])
            .pick_file()
        {
            let audio_manager = self.audio_manager.clone();
            let drum_tracks   = self.drum_tracks.clone();
            let drum_loading  = self.drum_loading.clone();
            let status        = self.status.clone();
            drum_loading.store(true, Ordering::Relaxed);
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    audio_manager.load_audio(path.to_str().unwrap_or(""))
                }));
                match result {
                    Ok(Ok(asset)) => {
                        let waveform = audio_manager.analyze_waveform(&asset, 400);
                        let track    = DrumTrack::new(asset.clone(), Some(waveform));
                        drum_tracks.write().push(track);
                        *status.write() = format!("✓ Track added: {}", asset.file_name);
                    }
                    Ok(Err(e)) => { *status.write() = format!("✗ Track load error: {}", e); }
                    Err(_)     => { *status.write() = "✗ Track load crashed".to_string(); }
                }
                drum_loading.store(false, Ordering::Relaxed);
            });
        }
    }

    pub fn switch_to_track(&self, track_idx: usize) {
        let tracks = self.drum_tracks.read();
        if let Some(track) = tracks.get(track_idx) {
            *self.waveform_focus.write()    = WaveformFocus::DrumTrack(track_idx);
            *self.waveform_analysis.write() = track.waveform.clone();
            *self.status.write()            = format!("Viewing: {}", track.asset.file_name);
        }
    }

     /// Refresh the cached list of input devices (spawns no thread; call from UI).
    pub fn refresh_input_devices(&self) {
        *self.input_devices.write() = RecordingManager::list_input_devices();
    }

    /// Add a new, empty recording track row.
    pub fn add_rec_track(&self) {
        if self.input_devices.read().is_empty() {
            self.refresh_input_devices();
        }
        self.rec_tracks.write().push(RecordingTrack::new());
    }

    /// Begin capturing from the device selected for `track_idx`.
    pub fn start_recording(&self, track_idx: usize) {
        // Only one recording at a time
        if self.rec_manager.is_recording() {
            *self.status.write() = "Already recording — stop current recording first".to_string();
            return;
        }

        let dev_label = {
            let tracks = self.rec_tracks.read();
            tracks.get(track_idx).and_then(|t| t.device_label.clone())
        };
        let dev_label = match dev_label {
            Some(l) => l,
            None => { *self.status.write() = "Select an input device first".to_string(); return; }
        };

        let dev = {
            let devices = self.input_devices.read();
            devices.iter().find(|d| d.label == dev_label).cloned()
        };
        let dev = match dev {
            Some(d) => d,
            None => {
                *self.status.write() = format!("Device '{}' not found — try Refresh", dev_label);
                return;
            }
        };

        match self.rec_manager.start(&dev) {
            Ok(()) => {
                *self.rec_active_track.write() = Some(track_idx);
                if let Some(t) = self.rec_tracks.write().get_mut(track_idx) {
                    t.state = RecordState::Recording;
                }
                *self.status.write() = format!("🔴 Recording from {}", dev.device_name);
            }
            Err(e) => {
                *self.status.write() = format!("Record error: {}", e);
            }
        }
    }

    /// Stop capturing and store the PCM in the track's asset.
    pub fn stop_recording(&self, track_idx: usize) {
        self.rec_manager.stop();
        *self.rec_active_track.write() = None;

        let (dev_label, take_num) = {
            let tracks = self.rec_tracks.read();
            let t = tracks.get(track_idx);
            (
                t.and_then(|t| t.device_label.clone()).unwrap_or_else(|| "rec".into()),
                t.map(|t| t.take_number).unwrap_or(1),
            )
        };

        // Sanitise device label into a filename-safe string
        let safe: String = dev_label
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .take(20)
            .collect();
        let file_name = format!("rec{}_take{}.wav", track_idx + 1, take_num);

        match self.rec_manager.take_asset(file_name.clone()) {
            Some(asset) => {
                let dur = asset.frames as f32 / asset.sample_rate as f32;
                let mut tracks = self.rec_tracks.write();
                if let Some(t) = tracks.get_mut(track_idx) {
                    t.asset = Some(asset);
                    t.state = RecordState::Recorded;
                    t.take_number += 1;
                }
                *self.status.write() = format!("✓ Recorded {:.2}s → {}", dur, file_name);
            }
            None => {
                if let Some(t) = self.rec_tracks.write().get_mut(track_idx) {
                    t.state = RecordState::Idle;
                }
                *self.status.write() = "Recording was empty".to_string();
            }
        }
    }

    /// Convert a recording track into a full DrumTrack (removes the rec row).
    pub fn promote_rec_to_drum(&self, rec_idx: usize) {
        let (asset_opt, steps) = {
            let tracks = self.rec_tracks.read();
            if let Some(t) = tracks.get(rec_idx) {
                (t.asset.clone(), t.steps)
            } else {
                return;
            }
        };
        if let Some(asset) = asset_opt {
            let waveform = self.audio_manager.analyze_waveform(&asset, 400);
            let mut drum = DrumTrack::new(asset.clone(), Some(waveform));
            drum.steps = steps; // carry over step pattern
            self.drum_tracks.write().push(drum);
            self.rec_tracks.write().remove(rec_idx);
            *self.status.write() = format!("✓ Promoted '{}' to drum track", asset.file_name);
        }
    }

    pub fn tick_sequencer(&self) {
        if !self.seq_playing.load(Ordering::Relaxed) { return; }
        let bpm      = self.seq_bpm.load(Ordering::Relaxed);
        let step_dur = std::time::Duration::from_secs_f64(60.0 / bpm as f64 / 4.0);
        let now      = Instant::now();
        let should_advance = {
            let last = self.seq_last_step_time.read();
            last.map_or(true, |t| now.duration_since(t) >= step_dur)
        };
        if !should_advance { return; }
        *self.seq_last_step_time.write() = Some(now);
        let step = {
            let mut s = self.seq_current_step.write();
            let cur = *s;
            *s = (cur + 1) % NUM_STEPS;
            cur
        };

        let mut voices: Vec<Voice> = Vec::new();

        // ── Main sample chops ─────────────────────────────────────────────
        if let Some(asset) = self.current_asset.read().clone() {
            let active_pads = self.seq_grid.read()[step].clone();
            if !active_pads.is_empty() {
                let marks       = self.samples_manager.get_marks();
                let channels    = asset.channels as usize;
                let total_frames = asset.pcm.len() / channels.max(1);
                let pcm         = Arc::new(asset.pcm.clone());
                let chop_adsr   = self.chop_adsr.read();
                for pad_idx in active_pads {
                    if let Some(mark) = marks.get(pad_idx) {
                        if mark.sample_name != asset.file_name { continue; }
                        let start_frame = (mark.position as f64 * total_frames as f64) as usize;
                        let adsr        = chop_adsr.get(pad_idx).copied().unwrap_or_default();
                        voices.push(Voice::new(pcm.clone(), channels, start_frame, 1.0, adsr, false));
                    }
                }
            }
        }

        // ── Drum track chops ──────────────────────────────────────────────
        {
            let tracks   = self.drum_tracks.read();
            let main_idx = *self.main_track_index.read();

            for (track_idx, track) in tracks.iter().enumerate() {
                if track.muted { continue; }
                let chop_marks = self.samples_manager.get_marks_for_sample(&track.asset.file_name);

                if !chop_marks.is_empty() {
                    let channels    = track.asset.channels as usize;
                    let total_frames = track.asset.pcm.len() / channels.max(1);
                    let pcm         = Arc::new(track.asset.pcm.clone());

                    for (chop_idx, mark) in chop_marks.iter().enumerate() {
                        let start_frame = (mark.position as f64 * total_frames as f64) as usize;
                        let adsr        = track.chop_adsr.get(chop_idx).copied().unwrap_or(track.adsr);
                        let chop_adsr_on = track.chop_adsr_enabled
                            .get(chop_idx).copied().unwrap_or(track.adsr_enabled);
                        let play_mode = track.chop_play_modes
                            .get(chop_idx).copied().unwrap_or(ChopPlayMode::ToNextChop);

                        let end_frame = match play_mode {
                            ChopPlayMode::ToEnd => None,
                            ChopPlayMode::ToNextChop => {
                                chop_marks.get(chop_idx + 1)
                                    .map(|n| (n.position as f64 * total_frames as f64) as usize)
                            }
                            ChopPlayMode::ToNextStep => {
                                let step_frames = (60.0 / bpm as f64 / 4.0
                                    * track.asset.sample_rate as f64) as usize;
                                Some(start_frame + step_frames)
                            }
                            ChopPlayMode::ToMarker(tid) => {
                                chop_marks.iter()
                                    .find(|m| m.id == tid)
                                    .map(|m| (m.position as f64 * total_frames as f64) as usize)
                            }
                        };

                        // ── PIANO ROLL: fired if the chop has any notes ───
                        let has_piano_notes = track.chop_piano_notes
                            .get(chop_idx)
                            .map(|n| !n.is_empty())
                            .unwrap_or(false);

                        if has_piano_notes {
                            // Fire every note whose step matches the current step
                            let piano_notes_now: Vec<PianoRollNote> = track.chop_piano_notes
                                .get(chop_idx)
                                .map(|notes| {
                                    notes.iter()
                                        .filter(|n| n.step == step)
                                        .cloned()
                                        .collect()
                                })
                                .unwrap_or_default();

                            for note in &piano_notes_now {
                                let mut voice = Voice::new(
                                    pcm.clone(), channels, start_frame,
                                    note.speed(),  // ← pitch shift!
                                    adsr, chop_adsr_on,
                                );
                                voice.end_frame = end_frame;
                                voices.push(voice);
                            }

                        } else {
                            // ── Fallback: regular step-sequencer on/off ───
                            let fires = if Some(track_idx) == main_idx {
                                self.seq_grid.read()[step].contains(&chop_idx)
                            } else {
                                track.chop_steps.get(chop_idx).map(|s| s[step]).unwrap_or(false)
                            };

                            if fires {
                                let mut voice = Voice::new(
                                    pcm.clone(), channels, start_frame,
                                    1.0, adsr, chop_adsr_on,
                                );
                                voice.end_frame = end_frame;
                                voices.push(voice);
                            }
                        }
                    }

                } else if track.steps[step] {
                    // Whole-sample trigger (no chops)
                    let channels = track.asset.channels as usize;
                    voices.push(Voice::new(
                        Arc::new(track.asset.pcm.clone()),
                        channels, 0, 1.0, track.adsr, track.adsr_enabled,
                    ));
                }
            }
        }
        {
            let rec_tracks = self.rec_tracks.read();
            for track in rec_tracks.iter() {
                if track.muted || track.state != RecordState::Recorded { continue; }
                if !track.steps[step] { continue; }
                if let Some(asset) = &track.asset {
                    let channels = asset.channels as usize;
                    voices.push(crate::adsr::Voice::new(
                        Arc::new(asset.pcm.clone()),
                        channels,
                        0, // play from beginning each step
                        1.0,
                        track.adsr,
                        track.adsr_enabled,
                    ));
                }
            }
        }

        if !voices.is_empty() {
            self.ensure_seq_stream();
            if let Ok(mut active) = self.active_voices.lock() {
                active.extend(voices);
            }
        }

    }

    fn ensure_seq_stream(&self) {
        if self.seq_stream_handle.read().is_some() { return; }
        let host   = cpal::default_host();
        let device = match host.default_output_device() { Some(d) => d, None => return };
        let config = match device.default_output_config() { Ok(c) => c, Err(_) => return };

        let mut cfg: cpal::StreamConfig = config.clone().into();
        cfg.buffer_size = cpal::BufferSize::Fixed(1024);
        cfg.sample_rate = cpal::SampleRate(48000);

        let out_channels = cfg.channels as usize;
        let sample_rate  = cfg.sample_rate.0 as f32;

        let stream = device.build_output_stream(
            &cfg,
            {
                let active_voices = self.active_voices.clone();
                let seq_playing   = self.seq_playing.clone();
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for s in data.iter_mut() { *s = 0.0; }
                    if !seq_playing.load(Ordering::Relaxed) { return; }
                    let mut voices = match active_voices.lock() { Ok(v) => v, Err(_) => return };
                    let out_frames = data.len() / out_channels.max(1);
                    voices.retain_mut(|voice| {
                        let mut alive = false;
                        for f in 0..out_frames {
                            if let Some(samples) = voice.render(sample_rate, out_channels) {
                                alive = true;
                                for (oc, smp) in samples.iter().enumerate() {
                                    let oi = f * out_channels + oc;
                                    if oi < data.len() {
                                        data[oi] = (data[oi] + smp).clamp(-1.0, 1.0);
                                    }
                                }
                            }
                        }
                        alive
                    });
                }
            },
            |err| eprintln!("Seq stream error: {}", err),
            None,
        );
        if let Ok(s) = stream { let _ = s.play(); *self.seq_stream_handle.write() = Some(s); }
    }

    pub fn start_sequencer(&self) {
        self.seq_voice_queue.lock().unwrap().clear();
        *self.seq_stream_handle.write() = None;
        *self.seq_current_step.write()  = 0;
        *self.seq_last_step_time.write() = None;
        self.seq_playing.store(true, Ordering::Relaxed);
        *self.status.write() = format!("Sequencer ▶ {:.0} BPM", self.seq_bpm.load(Ordering::Relaxed));
    }

    pub fn stop_sequencer(&self) {
        self.seq_playing.store(false, Ordering::Relaxed);
        *self.seq_stream_handle.write() = None;
        self.seq_voice_queue.lock().unwrap().clear();
        if let Ok(mut v) = self.active_voices.lock() { v.clear(); }
        *self.seq_current_step.write() = 0;
        *self.status.write() = "Sequencer stopped".to_string();
    }

    pub fn focused_display(&self) -> (Option<Arc<AudioAsset>>, Option<WaveformAnalysis>) {
        match self.waveform_focus.read().clone() {
            WaveformFocus::MainSample => (
                self.current_asset.read().clone(),
                self.waveform_analysis.read().clone(),
            ),
            WaveformFocus::DrumTrack(idx) => {
                let tracks = self.drum_tracks.read();
                if let Some(t) = tracks.get(idx) {
                    (Some(t.asset.clone()), t.waveform.clone())
                } else {
                    (self.current_asset.read().clone(), self.waveform_analysis.read().clone())
                }
            }
        }
    }
}

// ── Stream infrastructure (unchanged) ────────────────────────────────────────

struct StreamArgs {
    channels: u16, pcm: Vec<f32>,
    position: Arc<AtomicF32>, sample_index: Arc<AtomicU64>,
    is_playing: Arc<AtomicBool>, total_samples: u64,
    status: Arc<RwLock<String>>, stop_target: Arc<AtomicF32>,
}

fn build_stream<T: cpal::Sample + SizedSample + FromSample<f32> + 'static>(
    device: &cpal::Device, config: &cpal::StreamConfig, args: StreamArgs,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let ch = args.channels as usize; let total = args.total_samples; let pcm = args.pcm;
    let err_status = args.status.clone(); let err_playing = args.is_playing.clone();
    let err_fn = move |err| {
        eprintln!("Audio error: {}", err);
        *err_status.write() = format!("Playback error: {}", err);
        err_playing.store(false, Ordering::Relaxed);
    };
    let d_status = args.status; let d_playing = args.is_playing; let d_pos = args.position;
    let d_idx = args.sample_index; let d_stop = args.stop_target;
    let stream = device.build_output_stream(config, move |data: &mut [T], _| {
        let mut fp = d_idx.load(Ordering::Relaxed) as f64 / ch.max(1) as f64;
        if !d_playing.load(Ordering::Relaxed) {
            for d in data.iter_mut() { *d = T::from_sample(0.0f32); }
            return;
        }
        let frames     = data.len() / ch.max(1);
        let pcm_frames = pcm.len() / ch.max(1);
        let stop_pos   = d_stop.load(Ordering::Relaxed);
        let target     = if stop_pos >= 0.0 { Some((stop_pos * pcm_frames as f32) as usize) } else { None };
        let mut out    = 0usize;
        'outer: for _ in 0..frames {
            let i0 = fp as usize;
            if let Some(t) = target { if i0 >= t { d_playing.store(false, Ordering::Relaxed); *d_status.write() = "Stopped at marker".to_string(); break 'outer; } }
            if i0 >= pcm_frames.saturating_sub(1) { d_playing.store(false, Ordering::Relaxed); *d_status.write() = "Playback finished".to_string(); break 'outer; }
            let i1 = (i0 + 1).min(pcm_frames - 1); let t = (fp - i0 as f64) as f32;
            for c in 0..ch {
                let s0 = pcm.get(i0 * ch + c).copied().unwrap_or(0.0);
                let s1 = pcm.get(i1 * ch + c).copied().unwrap_or(0.0);
                if out < data.len() { data[out] = T::from_sample(s0 + t * (s1 - s0)); }
                out += 1;
            }
            fp += 1.0;
        }
        for d in data.iter_mut().skip(out) { *d = T::from_sample(0.0f32); }
        if total > 0 { d_pos.store((fp * ch as f64 / total as f64).min(1.0) as f32, Ordering::Relaxed); }
        d_idx.store((fp * ch as f64) as u64, Ordering::Relaxed);
    }, err_fn, None)?;
    Ok(stream)
}

pub mod ui;