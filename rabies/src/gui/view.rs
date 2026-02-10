use eframe::egui;
use std::time::Duration;
use std::sync::atomic::Ordering;

use super::AppState;
use crate::samples::PlaybackMode;


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

            // ✅ MPC-STYLE SAMPLE PADS GRID
            ui.add_space(12.0);
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Sample Pads").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Clear All").clicked() {
                            self.samples_manager.clear_marks();
                        }
                    });
                });
                
                ui.add_space(4.0);
                
                let marks = self.samples_manager.get_marks();
                
                if marks.is_empty() {
                    ui.label(egui::RichText::new("No pads yet").italics().color(egui::Color32::GRAY));
                    ui.label("Press 'M' while playing to create pads");
                } else {
                    // ✅ GRID LAYOUT: 4 columns
                    let cols = 4;
                    let pad_size = egui::vec2(
                        (ui.available_width() - (cols as f32 - 1.0) * 8.0) / cols as f32,
                        60.0
                    );
                    
                    egui::Grid::new("sample_pads_grid")
                        .spacing([8.0, 8.0])
                        .min_col_width(pad_size.x)
                        .show(ui, |ui| {
                            for (idx, mark) in marks.iter().enumerate() {
                                let duration = if let Some(asset) = self.current_asset.read().as_ref() {
                                    asset.frames as f32 / asset.sample_rate as f32
                                } else {
                                    0.0
                                };
                                let time_at_mark = mark.position * duration;
                                
                                let is_active = self.current_asset.read()
                                    .as_ref()
                                    .map(|a| a.file_name == mark.sample_name)
                                    .unwrap_or(false);
                                
                                let is_currently_playing = self.is_playing.load(Ordering::Relaxed) 
                                    && is_active
                                    && {
                                        let current_pos = self.playback_position.load(Ordering::Relaxed);
                                        (current_pos - mark.position).abs() < 0.05 // Within 5% of position
                                    };
                                
                                // ✅ MPC-STYLE PAD
                                let (rect, response) = ui.allocate_exact_size(
                                    pad_size,
                                    egui::Sense::click()
                                );
                                
                                // Determine pad color
                                let base_color = if is_currently_playing {
                                    egui::Color32::from_rgb(255, 140, 0) // Orange when playing from this pad
                                } else if is_active {
                                    egui::Color32::from_rgb(60, 140, 220) // Blue if sample loaded
                                } else {
                                    egui::Color32::from_rgb(40, 40, 45) // Dark gray if sample not loaded
                                };
                                
                                let color = if response.hovered() {
                                    egui::Color32::from_rgb(
                                        (base_color.r() as f32 * 1.3).min(255.0) as u8,
                                        (base_color.g() as f32 * 1.3).min(255.0) as u8,
                                        (base_color.b() as f32 * 1.3).min(255.0) as u8,
                                    )
                                } else {
                                    base_color
                                };
                                
                                // Draw pad background
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    color,
                                );
                                
                                // Draw border
                                ui.painter().rect_stroke(
                                    rect,
                                    4.0,
                                    egui::Stroke::new(
                                        if is_currently_playing { 3.0 } else { 1.0 },
                                        if is_currently_playing {
                                            egui::Color32::from_rgb(255, 200, 100)
                                        } else {
                                            egui::Color32::from_rgb(80, 80, 85)
                                        }
                                    ),
                                );
                                
                                // Draw pad ID (big number)
                                ui.painter().text(
                                    rect.center() - egui::vec2(0.0, 8.0),
                                    egui::Align2::CENTER_CENTER,
                                    format!("{}", mark.id),
                                    egui::FontId::proportional(24.0),
                                    egui::Color32::WHITE,
                                );
                                
                                // Draw time info (small text)
                                ui.painter().text(
                                    rect.center() + egui::vec2(0.0, 12.0),
                                    egui::Align2::CENTER_CENTER,
                                    format!("{:.2}s", time_at_mark),
                                    egui::FontId::proportional(10.0),
                                    egui::Color32::from_gray(200),
                                );
                                
                                // ✅ TRIGGER PLAYBACK ON CLICK
                                if response.clicked() {
                                    if let Some(asset) = self.current_asset.read().clone() {
                                        // Seek to marker position
                                        self.playback_position.store(mark.position, Ordering::Relaxed);
                                        let total_samples = asset.pcm.len();
                                        let new_sample_pos = (mark.position as f64 * total_samples as f64) as usize;
                                        self.playback_sample_index.store(new_sample_pos as u64, Ordering::Relaxed);
                                        
                                        // Start playback immediately
                                        self.start_playback(asset);
                                        
                                        *self.status.write() = format!("Playing from pad #{} ({:.2}s)", mark.id, time_at_mark);
                                    }
                                }
                                
                                // Right-click to delete
                                if response.secondary_clicked() {
                                    self.samples_manager.delete_mark(idx);
                                }
                                
                                // Show tooltip on hover
                                if response.hovered() {
                                    egui::show_tooltip_at_pointer(ui.ctx(), egui::Id::new(format!("pad_tooltip_{}", idx)), |ui| {
                                        ui.label(format!("Pad #{}", mark.id));
                                        ui.label(format!("Sample: {}", mark.sample_name));
                                        ui.label(format!("Time: {:.2}s", time_at_mark));
                                        ui.label(egui::RichText::new("Left-click: Play").small());
                                        ui.label(egui::RichText::new("Right-click: Delete").small());
                                    });
                                }
                                
                                // Start new row every 4 pads
                                if (idx + 1) % cols == 0 {
                                    ui.end_row();
                                }
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