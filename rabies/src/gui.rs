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
    loading: Arc<AtomicBool>,
    dragged_mark_index: Arc<RwLock<Option<usize>>>,
    
    // ✅ NEW: UI state for region selection
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
        
        // ✅ NEW: Clone samples_manager for playback stopping logic
        let samples_manager = self.samples_manager.clone();
        let sample_name = asset.file_name.clone();
        
        // Create stream based on sample format
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => self.build_stream::<f32>(
                &device, &config.into(), sample_rate, channels, pcm, position, sample_index, 
                is_playing, total_samples, status, samples_manager, sample_name
            ),
            cpal::SampleFormat::I16 => self.build_stream::<i16>(
                &device, &config.into(), sample_rate, channels, pcm, position, sample_index, 
                is_playing, total_samples, status, samples_manager, sample_name
            ),
            cpal::SampleFormat::U16 => self.build_stream::<u16>(
                &device, &config.into(), sample_rate, channels, pcm, position, sample_index, 
                is_playing, total_samples, status, samples_manager, sample_name
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
                    
                    // ✅ Update status based on playback mode
                    let mode_text = match self.samples_manager.get_playback_mode() {
                        PlaybackMode::PlayToEnd => "to end".to_string(),
                        PlaybackMode::PlayToNextMarker => "to next marker".to_string(),
                        PlaybackMode::CustomRegion { from, to } => format!("region {} → {}", from, to),
                    };
                    *self.status.write() = format!("Playing: {} ({})", asset.file_name, mode_text);
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
        samples_manager: Arc<SamplesManager>,  // ✅ NEW
        sample_name: String,  // ✅ NEW
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
                
                // ✅ NEW: Get target stop position if playback mode requires it
                let current_pos = if total_samples > 0 {
                    current_sample as f32 / total_samples as f32
                } else {
                    0.0
                };
                
                // ✅ NEW: Check if we should stop at a marker
                if samples_manager.should_stop_at(current_pos, &sample_name) {
                    data_is_playing.store(false, Ordering::Relaxed);
                    *data_status.write() = "Stopped at marker".to_string();
                    for d in data.iter_mut() {
                        *d = T::from_sample(0.0f32);
                    }
                    return;
                }
                
                for _ in 0..frames_needed {
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
                // ✅ NEW: For custom regions, start from the "from" marker
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

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Audio Sampler");

            // Transport controls
            ui.horizontal(|ui| {
                if ui.button("Load Sample").clicked() {
                    self.stop_playback();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "aac"])
                        .pick_file()
                    {
                        let path_buf = path.clone();
                        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let status = self.status.clone();
                        let audio_manager = self.audio_manager.clone();
                        let current_asset = self.current_asset.clone();
                        let waveform_analysis = self.waveform_analysis.clone();
                        let loading = self.loading.clone();

                        *self.status.write() = format!("Loading: {}...", file_name);
                        loading.store(true, Ordering::Relaxed);

                        std::thread::spawn(move || {
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                audio_manager.load_audio(path_buf.to_str().unwrap_or(""))
                            }));

                            match result {
                                Ok(Ok(asset)) => {
                                    *current_asset.write() = Some(asset.clone());
                                    let analysis = audio_manager.analyze_waveform(&asset, 400);
                                    *waveform_analysis.write() = Some(analysis);
                                    let duration = asset.frames as f32 / asset.sample_rate as f32;
                                    *status.write() = format!(
                                        "✓ Ready: {} ({:.2}s)",
                                        asset.file_name, duration
                                    );
                                }
                                Ok(Err(e)) => {
                                    *status.write() = format!("✗ Load error: {}", e);
                                    eprintln!("Audio load error: {}", e);
                                }
                                Err(panic) => {
                                    let msg = panic.downcast_ref::<&str>()
                                        .map(|s| s.to_string())
                                        .or_else(|| panic.downcast_ref::<String>().map(|s| s.clone()))
                                        .unwrap_or_else(|| "Unknown panic".to_string());
                                    
                                    *status.write() = format!("✗ CRASH: {}", msg);
                                    eprintln!("PANIC during audio load: {}", msg);
                                }
                            }
                            loading.store(false, Ordering::Relaxed);
                        });
                    }
                }

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

            // ✅ NEW: Playback Region Controls
            if let Some(asset) = self.current_asset.read().as_ref() {
                let marks = self.samples_manager.get_marks_for_sample(&asset.file_name);
                
                if !marks.is_empty() {
                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Playback Mode").strong());
                        
                        let current_mode = self.samples_manager.get_playback_mode();
                        
                        ui.horizontal(|ui| {
                            if ui.selectable_label(
                                matches!(current_mode, PlaybackMode::PlayToEnd),
                                "Play to End"
                            ).clicked() {
                                self.samples_manager.set_playback_mode(PlaybackMode::PlayToEnd);
                            }
                            
                            if ui.selectable_label(
                                matches!(current_mode, PlaybackMode::PlayToNextMarker),
                                "Play to Next Marker"
                            ).clicked() {
                                self.samples_manager.set_playback_mode(PlaybackMode::PlayToNextMarker);
                            }
                        });
                        
                        ui.add_space(4.0);
                        ui.label("Custom Region:");
                        ui.horizontal(|ui| {
                            ui.label("From:");
                            
                            let mut selected_from = self.selected_from_marker.write();
                            egui::ComboBox::from_id_source("from_marker")
                                .selected_text(
                                    selected_from
                                        .map(|id| format!("Marker {}", id))
                                        .unwrap_or_else(|| "Select...".to_string())
                                )
                                .show_ui(ui, |ui| {
                                    for mark in &marks {
                                        ui.selectable_value(&mut *selected_from, Some(mark.id), format!("Marker {}", mark.id));
                                    }
                                });
                            
                            ui.label("To:");
                            
                            let mut selected_to = self.selected_to_marker.write();
                            egui::ComboBox::from_id_source("to_marker")
                                .selected_text(
                                    selected_to
                                        .map(|id| format!("Marker {}", id))
                                        .unwrap_or_else(|| "Select...".to_string())
                                )
                                .show_ui(ui, |ui| {
                                    for mark in &marks {
                                        ui.selectable_value(&mut *selected_to, Some(mark.id), format!("Marker {}", mark.id));
                                    }
                                });
                            
                            let can_set_region = selected_from.is_some() && selected_to.is_some();
                            if ui.add_enabled(can_set_region, egui::Button::new("Set Region")).clicked() {
                                if let (Some(from), Some(to)) = (*selected_from, *selected_to) {
                                    self.samples_manager.set_playback_mode(PlaybackMode::CustomRegion { from, to });
                                    *self.status.write() = format!("Region set: {} → {}", from, to);
                                }
                            }
                        });
                    });
                }
            }

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

            // Waveform display
            ui.add_space(12.0);
            ui.group(|ui| {
                ui.label("Waveform");
                ui.add_space(4.0);

                let size = egui::Vec2::new(ui.available_width(), 200.0);
                let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
                let rect = response.rect;

                painter.rect_filled(rect, 0.0, egui::Color32::from_gray(25));

                if let Some(analysis) = self.waveform_analysis.read().as_ref() {
                    let center_y = rect.center().y;
                    let height_scale = rect.height() * 0.45;
                    let width = rect.width();
                    let bucket_count = analysis.min_max_buckets.len();
                    let bar_width = (width / bucket_count as f32).max(1.0);
                    
                    for (i, (min, max)) in analysis.min_max_buckets.iter().enumerate() {
                        let x = rect.left() + (i as f32 * bar_width);
                        let peak = max.abs().max(min.abs());
                        let bar_height = (peak * height_scale * 2.0).min(rect.height() * 0.9);
                        let bar_top = center_y - bar_height / 2.0;
                        
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(x, bar_top),
                                egui::pos2(x + bar_width - 0.5, bar_top + bar_height),
                            ),
                            0.0,
                            egui::Color32::from_rgb(80, 160, 255),
                        );
                    }
                    
                    painter.hline(
                        rect.x_range(),
                        center_y,
                        egui::Stroke::new(0.5, egui::Color32::from_gray(60)),
                    );
                    
                    // ✅ Draw markers with IDs
                    if let Some(asset) = self.current_asset.read().as_ref() {
                        let marks = self.samples_manager.get_marks();
                        let dragged_idx = *self.dragged_mark_index.read();
                        
                        for (idx, mark) in marks.iter().enumerate() {
                            if mark.sample_name != asset.file_name {
                                continue;
                            }
                            
                            let mark_x = rect.left() + mark.position * width;
                            let is_dragging = dragged_idx == Some(idx);
                            let color = if is_dragging {
                                egui::Color32::from_rgb(255, 255, 100)
                            } else {
                                egui::Color32::from_rgb(255, 200, 0)
                            };
                            let stroke_width = if is_dragging { 3.0 } else { 2.0 };
                            
                            painter.vline(
                                mark_x,
                                rect.y_range(),
                                egui::Stroke::new(stroke_width, color),
                            );
                            
                            let triangle_size = 5.0;
                            let triangle_points = vec![
                                egui::pos2(mark_x, rect.top()),
                                egui::pos2(mark_x - triangle_size, rect.top() + triangle_size),
                                egui::pos2(mark_x + triangle_size, rect.top() + triangle_size),
                            ];
                            
                            painter.add(egui::Shape::convex_polygon(
                                triangle_points,
                                color,
                                egui::Stroke::new(1.0, color),
                            ));
                            
                            // ✅ Draw marker ID
                            painter.text(
                                egui::pos2(mark_x, rect.top() + triangle_size + 12.0),
                                egui::Align2::CENTER_TOP,
                                format!("{}", mark.id),
                                egui::FontId::proportional(11.0),
                                egui::Color32::from_rgb(255, 220, 100),
                            );
                        }
                    }
                    
                    // Draw playhead
                    let progress = self.playback_position.load(Ordering::Relaxed);
                    let playhead_x = rect.left() + progress * width;
                    
                    painter.vline(
                        playhead_x,
                        rect.y_range(),
                        egui::Stroke::new(2.5, egui::Color32::from_rgb(255, 80, 80)),
                    );
                    
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
                    
                    // Handle marker dragging
                    if let Some(asset) = self.current_asset.read().as_ref() {
                        let mut dragged_index = self.dragged_mark_index.write();
                        let marks = self.samples_manager.get_marks();
                        let width = rect.width();
                        
                        if let Some(idx) = *dragged_index {
                            if idx < marks.len() && marks[idx].sample_name == asset.file_name {
                                if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                    if rect.contains(pos) {
                                        let normalized_x = (pos.x - rect.left()) / width;
                                        self.samples_manager.update_mark_position(idx, normalized_x);
                                    }
                                }
                                
                                if ui.input(|i| i.pointer.any_released()) {
                                    *dragged_index = None;
                                }
                            } else {
                                *dragged_index = None;
                            }
                        } else if response.drag_started_by(egui::PointerButton::Primary) {
                            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                if rect.contains(pos) {
                                    let normalized_x = (pos.x - rect.left()) / width;
                                    let threshold = 12.0 / width;
                                    if let Some(idx) = self.samples_manager.find_mark_near(
                                        &asset.file_name, 
                                        normalized_x, 
                                        threshold
                                    ) {
                                        *dragged_index = Some(idx);
                                        self.samples_manager.update_mark_position(idx, normalized_x);
                                    }
                                }
                            }
                        }
                    }
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
                
                if self.dragged_mark_index.read().is_none() {
                    if response.dragged() || response.clicked() {
                        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                            if rect.contains(pos) {
                                let normalized_x = (pos.x - rect.left()) / rect.width();
                                let clamped_x = normalized_x.clamp(0.0, 1.0);
                                self.seek_to(clamped_x);
                            }
                        }
                    }
                }
            });

            // Seek slider
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

            // Sample marks browser
            ui.add_space(12.0);
            ui.collapsing("Sample Marks", |ui| {
                let marks = self.samples_manager.get_marks();
                
                if marks.is_empty() {
                    ui.label(egui::RichText::new("No marks yet").italics().color(egui::Color32::GRAY));
                    ui.label("While playing, press 'M' to mark current position");
                } else {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} marks", marks.len()));
                        if ui.button("Clear All").clicked() {
                            self.samples_manager.clear_marks();
                        }
                    });
                    
                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| {
                            for (idx, mark) in marks.iter().enumerate() {
                                let duration = if let Some(asset) = self.current_asset.read().as_ref() {
                                    asset.frames as f32 / asset.sample_rate as f32
                                } else {
                                    0.0
                                };
                                let time_at_mark = mark.position * duration;
                                
                                ui.horizontal(|ui| {
                                    let label = format!(
                                        "#{} - {}  ({:.2}s)",
                                        mark.id,
                                        mark.sample_name,
                                        time_at_mark
                                    );
                                    
                                    let is_active = self.current_asset.read()
                                        .as_ref()
                                        .map(|a| a.file_name == mark.sample_name)
                                        .unwrap_or(false);
                                    
                                    if ui.button(
                                        egui::RichText::new(label).color(if is_active { 
                                            egui::Color32::from_rgb(80, 160, 255) 
                                        } else { 
                                            egui::Color32::WHITE 
                                        })
                                    ).clicked() {
                                        self.seek_to(mark.position);
                                        *self.status.write() = format!("Jumped to mark #{}: {:.2}s", mark.id, time_at_mark);
                                    }
                                    
                                    if ui.small_button("✖").clicked() {
                                        self.samples_manager.delete_mark(idx);
                                    }
                                });
                            }
                        });
                }
            });
        });

        if self.current_asset.read().is_none() && self.is_playing.load(Ordering::Relaxed) {
            self.stop_playback();
        }

        // Keyboard marking
        if self.is_playing.load(Ordering::Relaxed) {
            if ctx.input(|i| i.key_pressed(egui::Key::M)) {
                if let Some(asset) = self.current_asset.read().as_ref() {
                    let position = self.playback_position.load(Ordering::Relaxed);
                    self.samples_manager.mark_current_position(
                        &asset.file_name,
                        &asset.file_name,
                        position,
                    );
                    let duration = asset.frames as f32 / asset.sample_rate as f32;
                    *self.status.write() = format!("✓ Marked at {:.2}s", position * duration);
                }
            }
        }

        // Loading overlay
        if self.loading.load(Ordering::Relaxed) {
            let screen_rect = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("loading_overlay")));
            painter.rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(180));
            
            let center = screen_rect.center();
            let dialog_size = egui::vec2(240.0, 100.0);
            let dialog_rect = egui::Rect::from_center_size(center, dialog_size);
            painter.rect_filled(dialog_rect, 12.0, egui::Color32::from_gray(30));
            
            let time = ctx.input(|i| i.time) as f32;
            let radius = 20.0;
            let dot_count = 8;

            for i in 0..dot_count {
                let angle = time * 3.0 + (i as f32 * std::f32::consts::TAU / dot_count as f32);
                let offset = egui::vec2(angle.cos(), angle.sin()) * radius;
                let pos = egui::pos2(center.x + offset.x, center.y + offset.y - 10.0);
                let alpha = (100.0 + (i as f32 / dot_count as f32) * 155.0).min(255.0) as u8;
                painter.circle_filled(pos, 6.0, egui::Color32::from_rgba_unmultiplied(80, 160, 255, alpha));
            }
            
            painter.text(
                egui::pos2(center.x, center.y + 25.0),
                egui::Align2::CENTER_TOP,
                "Loading sample...",
                egui::FontId::proportional(16.0),
                egui::Color32::WHITE,
            );
        }

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}