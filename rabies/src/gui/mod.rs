use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use parking_lot::RwLock;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SizedSample, FromSample};
use atomic_float::AtomicF32;
use crate::audio::{AudioAsset, AudioManager, WaveformAnalysis};
use crate::samples::{SamplesManager, PlaybackMode};

pub const NUM_STEPS: usize = 16;

#[derive(Clone)]
pub struct VoiceEvent {
    pub pcm: Arc<Vec<f32>>,
    pub channels: usize,
    pub start_frame: usize,
    pub speed: f32,
}

/// One independently-loaded sample as a sequencer row.
pub struct DrumTrack {
    pub asset: Arc<AudioAsset>,
    pub waveform: Option<WaveformAnalysis>,
    pub steps: [bool; NUM_STEPS],
    pub muted: bool,
}

impl DrumTrack {
    pub fn new(asset: Arc<AudioAsset>, waveform: Option<WaveformAnalysis>) -> Self {
        Self { asset, waveform, steps: [false; NUM_STEPS], muted: false }
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

    // Chop sequencer grid (pads on main sample)
    pub seq_grid: Arc<RwLock<Vec<Vec<usize>>>>,

    // Multi-sample drum tracks
    pub drum_tracks: Arc<RwLock<Vec<DrumTrack>>>,
    pub drum_loading: Arc<AtomicBool>,

    // Sequencer engine
    pub seq_bpm: Arc<AtomicF32>,
    pub seq_playing: Arc<AtomicBool>,
    pub seq_current_step: Arc<RwLock<usize>>,
    pub seq_last_step_time: Arc<RwLock<Option<Instant>>>,
    pub(crate) seq_stream_handle: Arc<RwLock<Option<cpal::Stream>>>,
    pub(crate) seq_voice_queue: Arc<std::sync::Mutex<Vec<VoiceEvent>>>,

    // Which asset the waveform display shows
    pub waveform_focus: Arc<RwLock<WaveformFocus>>,

    pub piano_roll_open: Arc<RwLock<bool>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio_manager: Arc::new(AudioManager::new()),
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
                    self.samples_manager.get_mark_by_id(region.to).map(|m| m.position).unwrap_or(-1.0)
                } else { -1.0 }
            }
        };
        let stop_target = if stop_target >= 0.0 && start_pos >= stop_target { -1.0 } else { stop_target };
        self.playback_stop_target.store(stop_target, Ordering::Relaxed);
        self.is_playing.store(true, Ordering::Relaxed);

        let host = cpal::default_host();
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
        if let Some(asset) = self.current_asset.read().clone() {
            if self.is_playing.load(Ordering::Relaxed) {
                self.is_playing.store(false, Ordering::Relaxed);
                *self.status.write() = format!("Paused: {}", asset.file_name);
            } else {
                if let PlaybackMode::CustomRegion { region_id } = self.samples_manager.get_playback_mode() {
                    if let Some(region) = self.samples_manager.get_region_by_id(region_id) {
                        if let Some(mark) = self.samples_manager.get_mark_by_id(region.from) {
                            self.playback_position.store(mark.position, Ordering::Relaxed);
                            let sp = (mark.position as f64 * asset.pcm.len() as f64) as u64;
                            self.playback_sample_index.store(sp, Ordering::Relaxed);
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

    // ─────────────────────────────────────────────────────────
    //  Sequencer tick
    // ─────────────────────────────────────────────────────────
    pub fn tick_sequencer(&self) {
        if !self.seq_playing.load(Ordering::Relaxed) { return; }

        let bpm = self.seq_bpm.load(Ordering::Relaxed);
        let step_dur = std::time::Duration::from_secs_f64(60.0 / bpm as f64 / 4.0);
        let now = Instant::now();
        let should_advance = { let last = self.seq_last_step_time.read(); last.map_or(true, |t| now.duration_since(t) >= step_dur) };
        if !should_advance { return; }

        *self.seq_last_step_time.write() = Some(now);
        let step = { let mut s = self.seq_current_step.write(); let cur = *s; *s = (cur + 1) % NUM_STEPS; cur };

        let mut events: Vec<VoiceEvent> = Vec::new();

        // Chop pad events
        if let Some(asset) = self.current_asset.read().clone() {
            let active_pads = self.seq_grid.read()[step].clone();
            if !active_pads.is_empty() {
                let marks = self.samples_manager.get_marks();
                let channels = asset.channels as usize;
                let total_frames = asset.pcm.len() / channels.max(1);
                let pcm = Arc::new(asset.pcm.clone());
                for pad_idx in active_pads {
                    if let Some(mark) = marks.get(pad_idx) {
                        if mark.sample_name != asset.file_name { continue; }
                        let start_frame = (mark.position as f64 * total_frames as f64) as usize;
                        events.push(VoiceEvent { pcm: pcm.clone(), channels, start_frame, speed: 1.0 });
                    }
                }
            }
        }

        // Drum track events
        {
            let tracks = self.drum_tracks.read();
            for track in tracks.iter() {
                if !track.muted && track.steps[step] {
                    let channels = track.asset.channels as usize;
                    events.push(VoiceEvent {
                        pcm: Arc::new(track.asset.pcm.clone()),
                        channels,
                        start_frame: 0,
                        speed: 1.0,
                    });
                }
            }
        }

        if events.is_empty() { return; }
        self.ensure_seq_stream();
        self.seq_voice_queue.lock().unwrap().extend(events);
    }

    fn ensure_seq_stream(&self) {
        if self.seq_stream_handle.read().is_some() { return; }
        let host = cpal::default_host();
        let device = match host.default_output_device() { Some(d) => d, None => return };
        let config = match device.default_output_config() { Ok(c) => c, Err(_) => return };
        let cfg: cpal::StreamConfig = config.into();
        let out_channels = cfg.channels as usize;
        let seq_playing = self.seq_playing.clone();
        let voice_queue = self.seq_voice_queue.clone();

        let stream = device.build_output_stream(
            &cfg,
            {
                let mut voices: Vec<VoiceState> = Vec::with_capacity(24);
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    {
                        let mut q = voice_queue.lock().unwrap();
                        for ev in q.drain(..) {
                            if voices.len() >= 16 { voices.remove(0); }
                            voices.push(VoiceState { frame_pos: ev.start_frame as f64, speed: ev.speed, src_channels: ev.channels.max(1), pcm: ev.pcm });
                        }
                    }
                    for s in data.iter_mut() { *s = 0.0; }
                    if !seq_playing.load(Ordering::Relaxed) { voices.clear(); return; }
                    let out_frames = data.len() / out_channels.max(1);
                    for voice in voices.iter_mut() {
                        let src_ch = voice.src_channels;
                        let pcm_frames = voice.pcm.len() / src_ch;
                        for f in 0..out_frames {
                            let i0 = voice.frame_pos as usize;
                            if i0 >= pcm_frames.saturating_sub(1) { break; }
                            let i1 = (i0 + 1).min(pcm_frames - 1);
                            let t = (voice.frame_pos - i0 as f64) as f32;
                            for oc in 0..out_channels {
                                let sc = oc.min(src_ch - 1);
                                let s0 = voice.pcm.get(i0 * src_ch + sc).copied().unwrap_or(0.0);
                                let s1 = voice.pcm.get(i1 * src_ch + sc).copied().unwrap_or(0.0);
                                let smp = s0 + t * (s1 - s0);
                                let oi = f * out_channels + oc;
                                if oi < data.len() { data[oi] = (data[oi] + smp).clamp(-1.0, 1.0); }
                            }
                            voice.frame_pos += voice.speed as f64;
                        }
                    }
                    voices.retain(|v| (v.frame_pos as usize) < (v.pcm.len() / v.src_channels).saturating_sub(1));
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
        *self.seq_current_step.write() = 0;
        *self.seq_last_step_time.write() = None;
        self.seq_playing.store(true, Ordering::Relaxed);
        *self.status.write() = format!("Sequencer ▶ {:.0} BPM", self.seq_bpm.load(Ordering::Relaxed));
    }

    pub fn stop_sequencer(&self) {
        self.seq_playing.store(false, Ordering::Relaxed);
        *self.seq_stream_handle.write() = None;
        self.seq_voice_queue.lock().unwrap().clear();
        *self.seq_current_step.write() = 0;
        *self.status.write() = "Sequencer stopped".to_string();
    }

    /// Returns (asset, waveform) for whichever row is focused in the waveform display.
    pub fn focused_display(&self) -> (Option<Arc<AudioAsset>>, Option<WaveformAnalysis>) {
        match self.waveform_focus.read().clone() {
            WaveformFocus::MainSample => (self.current_asset.read().clone(), self.waveform_analysis.read().clone()),
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

struct VoiceState { frame_pos: f64, speed: f32, src_channels: usize, pcm: Arc<Vec<f32>> }

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
    let err_fn = move |err| { eprintln!("Audio error: {}", err); *err_status.write() = format!("Playback error: {}", err); err_playing.store(false, Ordering::Relaxed); };
    let d_status = args.status; let d_playing = args.is_playing; let d_pos = args.position;
    let d_idx = args.sample_index; let d_stop = args.stop_target;
    let init = d_idx.load(Ordering::Relaxed) as f64 / ch.max(1) as f64;
    let stream = device.build_output_stream(config, {
        let mut fp = init;
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            if !d_playing.load(Ordering::Relaxed) { for d in data.iter_mut() { *d = T::from_sample(0.0f32); } return; }
            let frames = data.len() / ch.max(1); let pcm_frames = pcm.len() / ch.max(1);
            let stop_pos = d_stop.load(Ordering::Relaxed);
            let target = if stop_pos >= 0.0 { Some((stop_pos * pcm_frames as f32) as usize) } else { None };
            let mut out = 0usize;
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
        }
    }, err_fn, None)?;
    Ok(stream)
}

pub mod view;