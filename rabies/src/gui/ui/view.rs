// src/gui/ui/view.rs
use eframe::egui;
use std::time::Duration;
use std::sync::atomic::Ordering;
use crate::gui::{AppState, WaveformFocus};
use crate::samples::PlaybackMode;
use super::widgets::*;

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick_sequencer();
        self.draw_piano_roll(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Audio Sampler");
                
                // ── Transport ──────────────────────────────────────────
                ui.horizontal(|ui| {
                    if ui.button("Load Sample").clicked() {
                        self.stop_playback();
                        self.stop_sequencer();
                        self.load_sample_as_track();
                    }
                    
                    // Track switching buttons
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Tracks").strong());
                            let tracks = self.drum_tracks.read();
                            for (idx, track) in tracks.iter().enumerate() {
                                let focus = self.waveform_focus.read().clone();
                                let is_focused = matches!(focus, WaveformFocus::DrumTrack(i) if i == idx);
                                let btn_label = if track.asset.file_name.len() > 10 {
                                    format!("{}…", &track.asset.file_name[..8])
                                } else {
                                    track.asset.file_name.clone()
                                };
                                if ui.selectable_label(is_focused, btn_label).clicked() {
                                    self.switch_to_track(idx);
                                }
                            }
                        });
                    });
                    
                    // Play/Pause for focused track
                    let focus = self.waveform_focus.read().clone();
                    let WaveformFocus::DrumTrack(idx) = focus else { return; };
                    let has_tracks = !self.drum_tracks.read().is_empty();
                    if has_tracks && idx < self.drum_tracks.read().len() {
                        let is_playing = self.is_playing.load(Ordering::Relaxed);
                        if ui.button(if is_playing { "⏸ Pause" } else { "▶ Play" }).clicked() {
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
                });
                
                ui.add_space(6.0);
                
                // ── Playback Mode Controls ─────────────────────────────────
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Playback Mode").strong().small());
                        
                        let current_mode = self.samples_manager.get_playback_mode();
                        
                        // Play to End button
                        let is_end = matches!(current_mode, PlaybackMode::PlayToEnd);
                        if ui.selectable_label(is_end, "Play to End").clicked() {
                            self.samples_manager.set_playback_mode(PlaybackMode::PlayToEnd);
                            *self.status.write() = "Playback: Play to End".to_string();
                        }
                        
                        // Play to Next Marker button
                        let is_next = matches!(current_mode, PlaybackMode::PlayToNextMarker);
                        if ui.selectable_label(is_next, "Play to Next").clicked() {
                            self.samples_manager.set_playback_mode(PlaybackMode::PlayToNextMarker);
                            *self.status.write() = "Playback: Stop at Next Marker".to_string();
                        }
                        
                        ui.separator();
                        
                        // Region playback (if regions exist)
                        let regions = self.samples_manager.get_regions();
                        if !regions.is_empty() {
                            ui.label(egui::RichText::new("Region:").small().color(egui::Color32::from_gray(100)));
                            for region in &regions {
                                let is_region = matches!(current_mode, PlaybackMode::CustomRegion { region_id } if region_id == region.id);
                                if ui.selectable_label(is_region, &region.name).clicked() {
                                    self.samples_manager.set_playback_mode(PlaybackMode::CustomRegion { region_id: region.id });
                                    *self.status.write() = format!("Playback: Region {}", region.name);
                                }
                            }
                        }
                    });
                });
                
                ui.add_space(4.0);
                ui.label(self.status.read().as_str());
                
                // ── Waveform Display ─────────────────────────────────────
                ui.add_space(8.0);
                let focus = self.waveform_focus.read().clone();
                let focus_label = {
                    let tracks = self.drum_tracks.read();
                    if let WaveformFocus::DrumTrack(idx) = &focus {
                        tracks.get(*idx)
                            .map(|t| format!("Waveform  ·  {} (Track {})  —  press M while previewing to chop", t.asset.file_name, idx + 1))
                            .unwrap_or("Waveform".to_string())
                    } else {
                        "Waveform".to_string()
                    }
                };
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&focus_label).small().color(egui::Color32::from_gray(170)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Preview button for focused track
                            if let WaveformFocus::DrumTrack(idx) = &focus {
                                let asset_opt = {
                                    let tracks = self.drum_tracks.read();
                                    tracks.get(*idx).map(|t| t.asset.clone())
                                };
                                if let Some(drum_asset) = asset_opt {
                                    let is_playing = self.is_playing.load(Ordering::Relaxed);
                                    let btn_label = if is_playing { "⏹ Stop" } else { "▶ Preview" };
                                    let btn_color = if is_playing {
                                        egui::Color32::from_rgb(220, 80, 60)
                                    } else {
                                        egui::Color32::from_rgb(60, 200, 100)
                                    };
                                    if ui.add(egui::Button::new(
                                        egui::RichText::new(btn_label).small().color(btn_color)
                                    )).clicked() {
                                        if is_playing {
                                            self.stop_playback();
                                        } else {
                                            self.playback_position.store(0.0, Ordering::Relaxed);
                                            self.playback_sample_index.store(0, Ordering::Relaxed);
                                            self.start_playback(drum_asset);
                                        }
                                    }
                                }
                            }
                        });
                    });
                    ui.add_space(2.0);
                    let size = egui::Vec2::new(ui.available_width(), 150.0);
                    let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
                    let rect = response.rect;
                    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(22));
                    let (focused_asset, focused_waveform) = self.focused_display();
                    if let Some(analysis) = focused_waveform.as_ref() {
                        let cy = rect.center().y;
                        let hs = rect.height() * 0.45;
                        let w = rect.width();
                        let bc = analysis.min_max_buckets.len();
                        let bw = (w / bc as f32).max(1.0);
                        let wave_color = if let WaveformFocus::DrumTrack(idx) = &focus {
                            drum_color(*idx)
                        } else {
                            egui::Color32::from_rgb(80, 160, 255)
                        };
                        for (i, (min, max)) in analysis.min_max_buckets.iter().enumerate() {
                            let x = rect.left() + i as f32 * bw;
                            let peak = max.abs().max(min.abs());
                            let bh = (peak * hs * 2.0).min(rect.height() * 0.9);
                            let bt = cy - bh / 2.0;
                            painter.rect_filled(
                                egui::Rect::from_min_max(egui::pos2(x, bt), egui::pos2(x + bw - 0.5, bt + bh)),
                                0.0, wave_color,
                            );
                        }
                        painter.hline(rect.x_range(), cy, egui::Stroke::new(0.5, egui::Color32::from_gray(55)));
                        
                        // Chop markers for focused track
                        if let WaveformFocus::DrumTrack(drum_idx) = &focus {
                            let file_name_opt = {
                                let tracks = self.drum_tracks.read();
                                tracks.get(*drum_idx).map(|t| t.asset.file_name.clone())
                            };
                            if let Some(file_name) = file_name_opt {
                                let marks = self.samples_manager.get_marks_for_sample(&file_name);
                                for (chop_idx, mark) in marks.iter().enumerate() {
                                    let mx = rect.left() + mark.position * w;
                                    let color = pad_color(chop_idx);
                                    painter.vline(mx, rect.y_range(), egui::Stroke::new(2.0, color));
                                    let ts = 5.0;
                                    painter.add(egui::Shape::convex_polygon(
                                        vec![egui::pos2(mx, rect.top()), egui::pos2(mx - ts, rect.top() + ts), egui::pos2(mx + ts, rect.top() + ts)],
                                        color, egui::Stroke::new(1.0, color),
                                    ));
                                    painter.text(egui::pos2(mx, rect.top() + ts + 12.0), egui::Align2::CENTER_TOP,
                                        format!("{}", chop_idx + 1), egui::FontId::proportional(11.0), color);
                                }
                                
                                // ── Region Visual Indicators ───────────────────────────
                                let regions = self.samples_manager.get_regions();
                                let current_mode = self.samples_manager.get_playback_mode();
                                for region in &regions {
                                    if let (Some(from_mark), Some(to_mark)) = (
                                        self.samples_manager.get_mark_by_id(region.from),
                                        self.samples_manager.get_mark_by_id(region.to),
                                    ) {
                                        if from_mark.sample_name == file_name && to_mark.sample_name == file_name {
                                            let x1 = rect.left() + from_mark.position * w;
                                            let x2 = rect.left() + to_mark.position * w;
                                            let is_active = matches!(current_mode, PlaybackMode::CustomRegion { region_id } if region_id == region.id);
                                            
                                            // Highlight region span
                                            let region_rect = egui::Rect::from_min_max(
                                                egui::pos2(x1, rect.top()),
                                                egui::pos2(x2, rect.bottom()),
                                            );
                                            let fill_color = if is_active {
                                                egui::Color32::from_rgba_unmultiplied(100, 200, 100, 40)
                                            } else {
                                                egui::Color32::from_rgba_unmultiplied(100, 150, 200, 25)
                                            };
                                            painter.rect_filled(region_rect, 0.0, fill_color);
                                            
                                            // Region label
                                            painter.text(
                                                egui::pos2(x1 + 5.0, rect.top() + 10.0),
                                                egui::Align2::LEFT_TOP,
                                                &region.name,
                                                egui::FontId::proportional(9.0),
                                                if is_active { egui::Color32::from_rgb(150, 255, 150) } else { egui::Color32::from_gray(100) },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Playback cursor
                        {
                            let prog = self.playback_position.load(Ordering::Relaxed);
                            let px = rect.left() + prog * w;
                            painter.vline(px, rect.y_range(), egui::Stroke::new(2.5, egui::Color32::from_rgb(255, 80, 80)));
                            let ts = 8.0;
                            painter.add(egui::Shape::convex_polygon(
                                vec![egui::pos2(px, rect.top() + ts), egui::pos2(px - ts, rect.top()), egui::pos2(px + ts, rect.top())],
                                egui::Color32::from_rgb(255, 80, 80), egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                            ));
                        }
                        
                        // Seek interaction
                        if response.dragged() || response.clicked() {
                            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                if rect.contains(pos) {
                                    let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                    self.playback_position.store(normalized, Ordering::Relaxed);
                                    let sp = {
                                        let tracks = self.drum_tracks.read();
                                        if let WaveformFocus::DrumTrack(drum_idx) = &focus {
                                            tracks.get(*drum_idx).map(|t| (normalized as f64 * t.asset.pcm.len() as f64) as u64)
                                        } else {
                                            None
                                        }
                                    };
                                    if let Some(sp) = sp {
                                        self.playback_sample_index.store(sp, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                    } else {
                        let text = if focused_asset.is_none() { "No sample loaded – click Load Sample" } else { "Analyzing waveform..." };
                        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text,
                            egui::FontId::monospace(13.0), egui::Color32::from_gray(160));
                    }
                });
                
                // ── Region Creation UI ────────────────────────────────────
                ui.add_space(4.0);
                if let WaveformFocus::DrumTrack(drum_idx) = &focus {
                    let file_name_opt = {
                        let tracks = self.drum_tracks.read();
                        tracks.get(*drum_idx).map(|t| t.asset.file_name.clone())
                    };
                    if let Some(file_name) = file_name_opt {
                        let marks = self.samples_manager.get_marks_for_sample(&file_name);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Regions").small().color(egui::Color32::from_gray(100)));
                            
                            if marks.len() >= 2 {
                                if ui.add(egui::Button::new(
                                    egui::RichText::new("＋ Create Region (First→Last Marker)").small()
                                )).clicked() {
                                    if let (Some(first), Some(last)) = (marks.first(), marks.last()) {
                                        let region_id = self.samples_manager.create_region(first.id, last.id);
                                        let region = self.samples_manager.get_region_by_id(region_id).unwrap();
                                        *self.status.write() = format!("✓ Created region: {} ({}→{})", 
                                            region.name, first.position, last.position);
                                    }
                                }
                                
                                // Show existing regions
                                let regions = self.samples_manager.get_regions();
                                for region in &regions {
                                    if ui.add(egui::Button::new(
                                        egui::RichText::new(format!("🗑 {}", region.name)).small()
                                            .color(egui::Color32::from_rgb(200, 80, 80))
                                    )).clicked() {
                                        self.samples_manager.delete_region(region.id);
                                        *self.status.write() = format!("✓ Deleted region: {}", region.name);
                                    }
                                }
                            } else {
                                ui.label(egui::RichText::new("Add 2+ markers to create regions").small()
                                    .color(egui::Color32::from_gray(60)));
                            }
                        });
                    }
                }
                
                // ── Step Sequencer ────────────────────────────────────
                ui.add_space(8.0);
                self.draw_step_sequencer(ui);
                
                // ── M key — mark chop point ──
                if self.is_playing.load(Ordering::Relaxed) {
                    if ctx.input(|i| i.key_pressed(egui::Key::M)) {
                        let pos = self.playback_position.load(Ordering::Relaxed);
                        let focus = self.waveform_focus.read().clone();
                        if let WaveformFocus::DrumTrack(idx) = focus {
                            let info = {
                                let tracks = self.drum_tracks.read();
                                tracks.get(idx).map(|t| (
                                    t.asset.file_name.clone(),
                                    t.asset.frames as f32 / t.asset.sample_rate as f32,
                                ))
                            };
                            if let Some((file_name, dur)) = info {
                                self.samples_manager.mark_current_position(&file_name, &file_name, pos);
                                *self.status.write() = format!("✓ Chopped {} at {:.2}s", file_name, pos * dur);
                                ctx.request_repaint();
                            }
                        }
                    }
                }
                
                if self.loading.load(Ordering::Relaxed) || self.drum_loading.load(Ordering::Relaxed) {
                    let sr = ctx.screen_rect();
                    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("loading")));
                    painter.rect_filled(sr, 0.0, egui::Color32::from_black_alpha(180));
                    let c = sr.center();
                    painter.rect_filled(egui::Rect::from_center_size(c, egui::vec2(240.0, 100.0)), 12.0, egui::Color32::from_gray(28));
                    let time = ctx.input(|i| i.time) as f32;
                    for i in 0u32..8 {
                        let angle = time * 3.0 + i as f32 * std::f32::consts::TAU / 8.0;
                        let off = egui::vec2(angle.cos(), angle.sin()) * 20.0;
                        let alpha = (100.0 + (i as f32 / 8.0) * 155.0) as u8;
                        painter.circle_filled(egui::pos2(c.x+off.x, c.y+off.y-10.0), 6.0, egui::Color32::from_rgba_unmultiplied(80,160,255,alpha));
                    }
                    painter.text(egui::pos2(c.x, c.y+25.0), egui::Align2::CENTER_TOP, "Loading...", egui::FontId::proportional(16.0), egui::Color32::WHITE);
                }
                ctx.request_repaint_after(Duration::from_millis(16));
            });
        });
    }
}