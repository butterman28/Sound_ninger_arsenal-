use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use eframe::egui;
use parking_lot::RwLock;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SizedSample, FromSample};
use atomic_float::AtomicF32;
use crate::audio::{AudioAsset, AudioManager, WaveformAnalysis};
use crate::samples::{SamplesManager, PlaybackMode};

pub struct AppState {
    pub audio_manager: Arc<AudioManager>,
    pub samples_manager: Arc<SamplesManager>,
    pub current_asset: Arc<RwLock<Option<Arc<AudioAsset>>>>,
    pub waveform_analysis: Arc<RwLock<Option<WaveformAnalysis>>>,
    pub status: Arc<RwLock<String>>,
    
    // Playback state
    playback_position: Arc<AtomicF32>,
    is_playing: Arc<AtomicBool>,
    stream_handle: Arc<RwLock<Option<cpal::Stream>>>,
    playback_asset: Arc<RwLock<Option<Arc<AudioAsset>>>>,
    playback_sample_index: Arc<AtomicU64>,
    playback_stop_target: Arc<AtomicF32>,
    loading: Arc<AtomicBool>,
    dragged_mark_index: Arc<RwLock<Option<usize>>>,
    
    // UI state for region selection
    selected_from_marker: Arc<RwLock<Option<usize>>>,
    selected_to_marker: Arc<RwLock<Option<usize>>>,
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
        }
    }
}

impl AppState {
    fn start_playback(&self, asset: Arc<AudioAsset>) {
        // Stop existing playback
        self.stop_playback();

        // Store asset for playback
        *self.playback_asset.write() = Some(asset.clone());

        // ================================
        // ✅ COMPUTE STOP TARGET ONCE
        // ================================
        let start_pos = self.playback_position.load(Ordering::Relaxed);

        let stop_target = match self.samples_manager.get_playback_mode() {
            PlaybackMode::PlayToEnd => -1.0,

            PlaybackMode::PlayToNextMarker => {
                self.samples_manager
                    .get_playback_target(start_pos, &asset.file_name)
                    .unwrap_or(-1.0)
            }

            PlaybackMode::CustomRegion { from: _, to } => {
                self.samples_manager
                    .get_mark_by_id(to)
                    .map(|m| m.position)
                    .unwrap_or(-1.0)
            }
        };

        self.playback_stop_target
            .store(stop_target, Ordering::Relaxed);
        // ================================

        self.is_playing.store(true, Ordering::Relaxed);

        // Setup CPAL audio stream
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                *self.status.write() = "Error: No audio output device found".to_string();
                self.is_playing.store(false, Ordering::Relaxed);
                return;
            }
        };

        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                *self.status.write() = format!("Error getting audio config: {}", e);
                self.is_playing.store(false, Ordering::Relaxed);
                return;
            }
        };

        let sample_rate = asset.sample_rate;
        let channels = asset.channels;
        let pcm = asset.pcm.clone();
        let position = self.playback_position.clone();
        let sample_index = self.playback_sample_index.clone();
        let is_playing = self.is_playing.clone();
        let status = self.status.clone();
        let total_samples = pcm.len() as u64;

        // ✅ Pass fixed stop target to audio thread
        let stop_target_atomic = self.playback_stop_target.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => self.build_stream::<f32>(
                &device,
                &config.into(),
                sample_rate,
                channels,
                pcm,
                position,
                sample_index,
                is_playing,
                total_samples,
                status,
                stop_target_atomic,
            ),
            cpal::SampleFormat::I16 => self.build_stream::<i16>(
                &device,
                &config.into(),
                sample_rate,
                channels,
                pcm,
                position,
                sample_index,
                is_playing,
                total_samples,
                status,
                stop_target_atomic,
            ),
            cpal::SampleFormat::U16 => self.build_stream::<u16>(
                &device,
                &config.into(),
                sample_rate,
                channels,
                pcm,
                position,
                sample_index,
                is_playing,
                total_samples,
                status,
                stop_target_atomic,
            ),
            _ => {
                *self.status.write() =
                    format!("Unsupported sample format: {:?}", config.sample_format());
                self.is_playing.store(false, Ordering::Relaxed);
                return;
            }
        };

        match stream {
            Ok(s) => {
                if let Err(e) = s.play() {
                    *self.status.write() = format!("Error starting playback: {}", e);
                    self.is_playing.store(false, Ordering::Relaxed);
                } else {
                    *self.stream_handle.write() = Some(s);

                    let mode_text = match self.samples_manager.get_playback_mode() {
                        PlaybackMode::PlayToEnd => "to end".to_string(),
                        PlaybackMode::PlayToNextMarker => "to next marker".to_string(),
                        PlaybackMode::CustomRegion { from, to } => format!("region {} → {}", from, to),
                    };

                    *self.status.write() =
                        format!("Playing: {} ({})", asset.file_name, mode_text);
                }
            }
            Err(e) => {
                *self.status.write() = format!("Error creating audio stream: {}", e);
                self.is_playing.store(false, Ordering::Relaxed);
            }
        }
    }

    
    fn build_stream<T: cpal::Sample + SizedSample + FromSample<f32>>(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        _sample_rate: u32,
        channels: u16,
        pcm: Vec<f32>,
        position: Arc<AtomicF32>,
        sample_index: Arc<AtomicU64>,
        is_playing: Arc<AtomicBool>,
        total_samples: u64,
        status: Arc<RwLock<String>>,
        stop_target: Arc<AtomicF32>,  // ✅ FIXED: Use pre-computed stop target
    ) -> Result<cpal::Stream, cpal::BuildStreamError> 
    {
        let channels_count = channels as usize;
        
        let err_status = status.clone();
        let err_is_playing = is_playing.clone();
        
        let err_fn = move |err| {
            eprintln!("Audio error: {}", err);
            *err_status.write() = format!("Playback error: {}", err);
            err_is_playing.store(false, Ordering::Relaxed);
        };
        
        let data_status = status.clone();
        let data_is_playing = is_playing.clone();
        let data_position = position.clone();
        let data_sample_index = sample_index.clone();
        let data_pcm = pcm.clone();
        let data_stop_target = stop_target.clone();  // ✅ Clone for audio callback
        
        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                if !data_is_playing.load(Ordering::Relaxed) {
                    for d in data.iter_mut() {
                        *d = T::from_sample(0.0f32);
                    }
                    return;
                }
                
                let mut current_sample = data_sample_index.load(Ordering::Relaxed) as usize;
                let frames_needed = data.len() / channels_count;
                let mut sample_index_pos = 0;
                
                // ✅ FIXED: Get the pre-computed stop target
                let stop_position = data_stop_target.load(Ordering::Relaxed);
                
                // Calculate target sample index if we have a valid stop position
                let target_sample = if stop_position >= 0.0 {
                    Some((stop_position * total_samples as f32) as usize)
                } else {
                    None
                };
                
                for _ in 0..frames_needed {
                    // ✅ FIXED: Check if we've reached the target marker
                    if let Some(target) = target_sample {
                        if current_sample >= target {
                            data_is_playing.store(false, Ordering::Relaxed);
                            *data_status.write() = "Stopped at marker".to_string();
                            break;
                        }
                    }
                    
                    for ch in 0..channels_count {
                        let sample_value = if current_sample + ch < data_pcm.len() {
                            data_pcm[current_sample + ch]
                        } else {
                            0.0
                        };
                        
                        data[sample_index_pos] = T::from_sample(sample_value);
                        sample_index_pos += 1;
                    }
                    
                    current_sample = current_sample.saturating_add(channels_count);
                    
                    if current_sample >= data_pcm.len() {
                        data_is_playing.store(false, Ordering::Relaxed);
                        *data_status.write() = "Playback finished".to_string();
                        break;
                    }
                }
                
                if total_samples > 0 {
                    let progress = current_sample as f32 / total_samples as f32;
                    data_position.store(progress.min(1.0), Ordering::Relaxed);
                }
                
                data_sample_index.store(current_sample as u64, Ordering::Relaxed);
                
                // Zero-fill remaining buffer
                for d in data.iter_mut().skip(sample_index_pos) {
                    *d = T::from_sample(0.0f32);
                }
            },
            err_fn,
            None,
        )?;
        
        Ok(stream)
    }
    
    fn stop_playback(&self) {
        self.is_playing.store(false, Ordering::Relaxed);
        *self.stream_handle.write() = None;
        *self.playback_asset.write() = None;
    }
    
    fn toggle_playback(&self) {
        if let Some(asset) = self.current_asset.read().clone() {
            if self.is_playing.load(Ordering::Relaxed) {
                self.is_playing.store(false, Ordering::Relaxed);
                *self.status.write() = format!("Paused: {}", asset.file_name);
            } else {
                // For custom regions, start from the "from" marker
                if let PlaybackMode::CustomRegion { from, .. } = self.samples_manager.get_playback_mode() {
                    if let Some(mark) = self.samples_manager.get_mark_by_id(from) {
                        self.playback_position.store(mark.position, Ordering::Relaxed);
                        let total_samples = asset.pcm.len();
                        let new_sample_pos = (mark.position as f64 * total_samples as f64) as usize;
                        self.playback_sample_index.store(new_sample_pos as u64, Ordering::Relaxed);
                    }
                } else if self.playback_position.load(Ordering::Relaxed) >= 0.999 {
                    // If we're at the end, restart from beginning
                    self.playback_position.store(0.0, Ordering::Relaxed);
                    self.playback_sample_index.store(0, Ordering::Relaxed);
                }
                self.start_playback(asset);
            }
        }
    }
    
    fn seek_to(&self, normalized_pos: f32) {
        if let Some(asset) = self.current_asset.read().as_ref() {
            let was_playing = self.is_playing.load(Ordering::Relaxed);
            self.is_playing.store(false, Ordering::Relaxed);
            
            let total_samples = asset.pcm.len();
            let new_sample_pos = (normalized_pos as f64 * total_samples as f64) as usize;
            let clamped_pos = new_sample_pos.min(total_samples);
            
            self.playback_position.store(normalized_pos, Ordering::Relaxed);
            self.playback_sample_index.store(clamped_pos as u64, Ordering::Relaxed);
            
            let duration = asset.frames as f32 / asset.sample_rate as f32;
            let current_time = normalized_pos * duration;
            *self.status.write() = format!(
                "Seeked to {:.2}s / {:.2}s",
                current_time,
                duration
            );
            
            if was_playing {
                self.start_playback(asset.clone());
            }
        }
    }
}

pub mod view;