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

pub struct AppState {
    pub audio_manager: Arc<AudioManager>,
    pub current_asset: Arc<RwLock<Option<Arc<AudioAsset>>>>,
    pub waveform_analysis: Arc<RwLock<Option<WaveformAnalysis>>>,
    pub status: Arc<RwLock<String>>,
    
    // Playback state
    playback_position: Arc<AtomicF32>,   // Normalized position (0.0 to 1.0)
    is_playing: Arc<AtomicBool>,
    stream_handle: Arc<RwLock<Option<cpal::Stream>>>,
    playback_asset: Arc<RwLock<Option<Arc<AudioAsset>>>>,
    playback_sample_index: Arc<AtomicU64>, // Track actual sample position for seeking
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio_manager: Arc::new(AudioManager::new()),
            current_asset: Arc::new(RwLock::new(None)),
            waveform_analysis: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new("Click Load Sample to begin".to_string())),
            
            // Playback initialization
            playback_position: Arc::new(AtomicF32::new(0.0)),
            is_playing: Arc::new(AtomicBool::new(false)),
            stream_handle: Arc::new(RwLock::new(None)),
            playback_asset: Arc::new(RwLock::new(None)),
            playback_sample_index: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl AppState {
    fn start_playback(&self, asset: Arc<AudioAsset>) {
        // Stop existing playback
        self.stop_playback();
        
        // Store asset for playback
        *self.playback_asset.write() = Some(asset.clone());
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
        
        // Create stream based on sample format
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => self.build_stream::<f32>(
                &device, &config.into(), sample_rate, channels, pcm, position, sample_index, is_playing, total_samples, status
            ),
            cpal::SampleFormat::I16 => self.build_stream::<i16>(
                &device, &config.into(), sample_rate, channels, pcm, position, sample_index, is_playing, total_samples, status
            ),
            cpal::SampleFormat::U16 => self.build_stream::<u16>(
                &device, &config.into(), sample_rate, channels, pcm, position, sample_index, is_playing, total_samples, status
            ),
            _ => {
                *self.status.write() = format!("Unsupported sample format: {:?}", config.sample_format());
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
                    *self.status.write() = format!("Playing: {}", asset.file_name);
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
    ) -> Result<cpal::Stream, cpal::BuildStreamError> 
    {
        let channels_count = channels as usize;
        
        // ✅ FIXED: Clone Arcs BEFORE moving into closures to avoid move errors
        let err_status = status.clone();
        let err_is_playing = is_playing.clone();
        
        let err_fn = move |err| {
            eprintln!("Audio error: {}", err);
            *err_status.write() = format!("Playback error: {}", err);
            err_is_playing.store(false, Ordering::Relaxed);
        };
        
        // ✅ FIXED: Clone Arcs for data callback
        let data_status = status.clone();
        let data_is_playing = is_playing.clone();
        let data_position = position.clone();
        let data_sample_index = sample_index.clone();
        let data_pcm = pcm.clone(); // Vec<f32> is cheap to clone for audio buffers
        
        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                if !data_is_playing.load(Ordering::Relaxed) {
                    for d in data.iter_mut() {
                        *d = T::from_sample(0.0f32);
                    }
                    return;
                }
                
                // Get current sample position atomically
                let mut current_sample = data_sample_index.load(Ordering::Relaxed) as usize;
                let frames_needed = data.len() / channels_count;
                let mut sample_index_pos = 0;
                
                for _ in 0..frames_needed {
                    // Interleave channels
                    for ch in 0..channels_count {
                        let sample_value = if current_sample + ch < data_pcm.len() {
                            data_pcm[current_sample + ch]
                        } else {
                            0.0 // End of audio
                        };
                        
                        data[sample_index_pos] = T::from_sample(sample_value);
                        sample_index_pos += 1;
                    }
                    
                    current_sample = current_sample.saturating_add(channels_count);
                    
                    // Check for end of audio
                    if current_sample >= data_pcm.len() {
                        data_is_playing.store(false, Ordering::Relaxed);
                        *data_status.write() = "Playback finished".to_string();
                        break;
                    }
                }
                
                // Update playback position (normalized 0.0-1.0)
                if total_samples > 0 {
                    let progress = current_sample as f32 / total_samples as f32;
                    data_position.store(progress.min(1.0), Ordering::Relaxed);
                }
                
                // Save updated position atomically
                data_sample_index.store(current_sample as u64, Ordering::Relaxed);
                
                // Zero-fill remaining buffer if we hit end of audio
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
                // If we're at the end, restart from beginning
                if self.playback_position.load(Ordering::Relaxed) >= 0.999 {
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
            
            // Calculate new sample position
            let total_samples = asset.pcm.len();
            let new_sample_pos = (normalized_pos as f64 * total_samples as f64) as usize;
            let clamped_pos = new_sample_pos.min(total_samples);
            
            // Update position atoms
            self.playback_position.store(normalized_pos, Ordering::Relaxed);
            self.playback_sample_index.store(clamped_pos as u64, Ordering::Relaxed);
            
            // Update status
            let duration = asset.frames as f32 / asset.sample_rate as f32;
            let current_time = normalized_pos * duration;
            *self.status.write() = format!(
                "Seeked to {:.2}s / {:.2}s",
                current_time,
                duration
            );
            
            // Resume playback if it was playing
            if was_playing {
                self.start_playback(asset.clone());
            }
        }
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Rabies");

            // Transport controls
            ui.horizontal(|ui| {
                if ui.button("Load Sample").clicked() {
                    self.stop_playback();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "aac"])
                        .pick_file()
                    {
                        let path_str = path.to_string_lossy().to_string();
                        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let status = self.status.clone();
                        let audio_manager = self.audio_manager.clone();
                        let current_asset = self.current_asset.clone();
                        let waveform_analysis = self.waveform_analysis.clone();

                        *self.status.write() = format!("Loading: {}...", file_name);

                        thread::spawn(move || {
                            match audio_manager.load_audio(&path_str) {
                                Ok(asset) => {
                                    *current_asset.write() = Some(asset.clone());
                                    let analysis = audio_manager.analyze_waveform(&asset, 400);
                                    *waveform_analysis.write() = Some(analysis);
                                    let duration = asset.frames as f32 / asset.sample_rate as f32;
                                    *status.write() = format!(
                                        "✓ Ready: {} ({:.2}s)",
                                        asset.file_name, duration
                                    );
                                }
                                Err(e) => {
                                    *status.write() = format!("✗ Error: {}", e);
                                }
                            }
                        });
                    }
                }

                // Play/Pause button
                if let Some(asset) = self.current_asset.read().as_ref() {
                    let is_playing = self.is_playing.load(Ordering::Relaxed);
                    let label = if is_playing { "⏸ Pause" } else { "▶ Play" };
                    if ui.button(label).clicked() {
                        self.toggle_playback();
                    }
                } else {
                    ui.add_enabled(false, egui::Button::new("▶ Play"));
                }

                if ui.button("■ Stop").clicked() {
                    self.stop_playback();
                    // ✅ Reset position to beginning on STOP (not on pause)
                    self.playback_position.store(0.0, Ordering::Relaxed);
                    self.playback_sample_index.store(0, Ordering::Relaxed);
                    *self.status.write() = "Stopped".to_string();
                }

                if ui.button("Clear").clicked() {
                    self.stop_playback();
                    *self.current_asset.write() = None;
                    *self.waveform_analysis.write() = None;
                    *self.status.write() = "Ready. Load an audio sample to begin".to_string();
                }
            });

            ui.add_space(8.0);
            ui.label(self.status.read().as_str());

            // Sample info panel
            if let Some(asset) = self.current_asset.read().as_ref() {
                ui.add_space(8.0);
                ui.collapsing("Sample Info", |ui| {
                    ui.label(format!("Name: {}", asset.file_name));
                    ui.label(format!("Sample Rate: {} Hz", asset.sample_rate));
                    ui.label(format!("Channels: {}", asset.channels));
                    ui.label(format!("Duration: {:.3}s", asset.frames as f32 / asset.sample_rate as f32));
                });
            }

            // Waveform display with seek head
            ui.add_space(12.0);
            ui.group(|ui| {
                ui.label("Waveform");
                ui.add_space(4.0);

                let size = egui::Vec2::new(ui.available_width(), 200.0);
                let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
                let rect = response.rect;

                // Background
                painter.rect_filled(rect, 0.0, egui::Color32::from_gray(25));

 if let Some(analysis) = self.waveform_analysis.read().as_ref() {
    let center_y = rect.center().y;
    let height_scale = rect.height() * 0.45;
    let width = rect.width();
    let bucket_count = analysis.min_max_buckets.len();
    let bar_width = (width / bucket_count as f32).max(1.0);
    
    // Draw solid blue bars (amplitude visualization)
    for (i, (min, max)) in analysis.min_max_buckets.iter().enumerate() {
        let x = rect.left() + (i as f32 * bar_width);
        let peak = max.abs().max(min.abs());
        let bar_height = (peak * height_scale * 2.0).min(rect.height() * 0.9);
        let bar_top = center_y - bar_height / 2.0;
        
        // Solid blue bar (filled rectangle)
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x, bar_top),
                egui::pos2(x + bar_width - 0.5, bar_top + bar_height), // -0.5 prevents gaps
            ),
            0.0,
            egui::Color32::from_rgb(80, 160, 255),
        );
    }
    
    // Center line
    painter.hline(
        rect.x_range(),
        center_y,
        egui::Stroke::new(0.5, egui::Color32::from_gray(60)),
    );
    
    // Draw playhead
    let progress = self.playback_position.load(Ordering::Relaxed);
    let playhead_x = rect.left() + progress * width;
    
    painter.vline(
        playhead_x,
        rect.y_range(),
        egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 80, 80)),
    );
    
    // Draw playhead triangle
    let triangle_size = 8.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(playhead_x, rect.top() + triangle_size),
            egui::pos2(playhead_x - triangle_size, rect.top()),
            egui::pos2(playhead_x + triangle_size, rect.top()),
        ],
        egui::Color32::from_rgb(255, 80, 80),
        egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
    ));
} else {
    let text = if self.current_asset.read().is_none() {
        "No sample loaded – click Load Sample"
    } else {
        "Analyzing waveform..."
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::monospace(14.0),
        egui::Color32::from_gray(180),
    );
}     
                // Handle click/drag to seek
                if response.dragged() || response.clicked() {
                    let pos = ui.input(|i| i.pointer.hover_pos());
                    if let Some(pos) = pos {
                        if rect.contains(pos) {
                            let normalized_x = (pos.x - rect.left()) / rect.width();
                            let clamped_x = normalized_x.clamp(0.0, 1.0);
                            self.seek_to(clamped_x);
                        }
                    }
                }
            });

            // Seek slider with time display
            if let Some(asset) = self.current_asset.read().as_ref() {
                ui.add_space(8.0);
                let duration = asset.frames as f32 / asset.sample_rate as f32;
                let mut progress = self.playback_position.load(Ordering::Relaxed);
                let current_time = progress * duration;
                
                ui.horizontal(|ui| {
                    ui.label(format!("{:.2}s", current_time));
                    let slider = egui::Slider::new(&mut progress, 0.0..=1.0)
                        .show_value(false);
                    if ui.add(slider).changed() {
                        self.seek_to(progress);
                    }
                    ui.label(format!("{:.2}s", duration));
                });
            }
        });

        // Auto-pause when asset is cleared
        if self.current_asset.read().is_none() && self.is_playing.load(Ordering::Relaxed) {
            self.stop_playback();
        }

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}