// src/gui/ui/view.rs

use eframe::egui;
use std::time::Duration;
use std::sync::atomic::Ordering;
// Import from parent crate, not super
use crate::gui::{AppState,WaveformFocus, NUM_STEPS};
use crate::samples::PlaybackMode;
use crate::adsr::ADSREnvelope;

// Import widgets from sibling module
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
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Audio", &["mp3","wav","flac","ogg","m4a","aac"])
                            .pick_file()
                        {
                            let pb = path.clone();
                            let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let status = self.status.clone();
                            let audio_manager = self.audio_manager.clone();
                            let current_asset = self.current_asset.clone();
                            let waveform_analysis = self.waveform_analysis.clone();
                            let loading = self.loading.clone();
                            let waveform_focus = self.waveform_focus.clone();
                            *self.status.write() = format!("Loading: {}...", fname);
                            loading.store(true, Ordering::Relaxed);
                            std::thread::spawn(move || {
                                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    audio_manager.load_audio(pb.to_str().unwrap_or(""))
                                }));
                                match result {
                                    Ok(Ok(asset)) => {
                                        *current_asset.write() = Some(asset.clone());
                                        let analysis = audio_manager.analyze_waveform(&asset, 400);
                                        *waveform_analysis.write() = Some(analysis);
                                        *waveform_focus.write() = WaveformFocus::MainSample;
                                        let dur = asset.frames as f32 / asset.sample_rate as f32;
                                        *status.write() = format!("✓ Ready: {} ({:.2}s)", asset.file_name, dur);
                                    }
                                    Ok(Err(e)) => { *status.write() = format!("✗ Load error: {}", e); }
                                    Err(p) => {
                                        let msg = p.downcast_ref::<&str>().map(|s| s.to_string())
                                            .or_else(|| p.downcast_ref::<String>().map(|s| s.clone()))
                                            .unwrap_or("Unknown panic".to_string());
                                        *status.write() = format!("✗ CRASH: {}", msg);
                                    }
                                }
                                loading.store(false, Ordering::Relaxed);
                            });
                        }
                    }
                    if self.current_asset.read().is_some() {
                        let is_playing = self.is_playing.load(Ordering::Relaxed);
                        if ui.button(if is_playing { "⏸ Pause" } else { "▶ Play" }).clicked() { self.toggle_playback(); }
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
                        *self.waveform_focus.write() = WaveformFocus::MainSample;
                        *self.status.write() = "Ready. Load an audio sample to begin".to_string();
                    }
                });
                ui.add_space(6.0);
                ui.label(self.status.read().as_str());
                // ── Playback Region Controls ───────────────────────────
                if let Some(asset) = self.current_asset.read().as_ref() {
                    let marks = self.samples_manager.get_marks_for_sample(&asset.file_name);
                    if !marks.is_empty() {
                        ui.add_space(6.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("Playback Mode").strong());
                            let cur_mode = self.samples_manager.get_playback_mode();
                            ui.horizontal(|ui| {
                                if ui.selectable_label(matches!(cur_mode, PlaybackMode::PlayToEnd), "Play to End").clicked() {
                                    self.samples_manager.set_playback_mode(PlaybackMode::PlayToEnd);
                                }
                                if ui.selectable_label(matches!(cur_mode, PlaybackMode::PlayToNextMarker), "Play to Next Marker").clicked() {
                                    self.samples_manager.set_playback_mode(PlaybackMode::PlayToNextMarker);
                                }
                            });
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Custom Regions").strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let sf = self.selected_from_marker.read();
                                    let st = self.selected_to_marker.read();
                                    if ui.add_enabled(sf.is_some() && st.is_some(), egui::Button::new("➕ Create Region")).clicked() {
                                        if let (Some(from), Some(to)) = (*sf, *st) {
                                            let rid = self.samples_manager.create_region(from, to);
                                            self.samples_manager.set_playback_mode(PlaybackMode::CustomRegion { region_id: rid });
                                        }
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label("From:");
                                let mut sf = self.selected_from_marker.write();
                                egui::ComboBox::from_id_source("from_marker")
                                    .selected_text(sf.map(|id| format!("Marker {}", id)).unwrap_or("Select...".into()))
                                    .show_ui(ui, |ui| { for m in &marks { ui.selectable_value(&mut *sf, Some(m.id), format!("Marker {}", m.id)); } });
                                ui.label("To:");
                                let mut st = self.selected_to_marker.write();
                                egui::ComboBox::from_id_source("to_marker")
                                    .selected_text(st.map(|id| format!("Marker {}", id)).unwrap_or("Select...".into()))
                                    .show_ui(ui, |ui| { for m in &marks { ui.selectable_value(&mut *st, Some(m.id), format!("Marker {}", m.id)); } });
                            });
                            let regions = self.samples_manager.get_regions();
                            if !regions.is_empty() {
                                ui.separator();
                                egui::ScrollArea::vertical().max_height(80.0).show(ui, |ui| {
                                    for region in &regions {
                                        ui.horizontal(|ui| {
                                            let is_active = matches!(cur_mode, PlaybackMode::CustomRegion { region_id } if region_id == region.id);
                                            if ui.selectable_label(is_active, &region.name).clicked() {
                                                self.samples_manager.set_playback_mode(PlaybackMode::CustomRegion { region_id: region.id });
                                            }
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.small_button("🗑").clicked() { self.samples_manager.delete_region(region.id); }
                                            });
                                        });
                                    }
                                });
                            }
                        });
                    }
                }
                // ── Sample Info ────────────────────────────────────────
                if let Some(asset) = self.current_asset.read().as_ref() {
                    ui.add_space(4.0);
                    ui.collapsing("Sample Info", |ui| {
                        ui.label(format!("Name: {}", asset.file_name));
                        ui.label(format!("Sample Rate: {} Hz | Channels: {} | Duration: {:.3}s",
                            asset.sample_rate, asset.channels, asset.frames as f32 / asset.sample_rate as f32));
                    });
                }
                ui.add_space(8.0);
                let focus = self.waveform_focus.read().clone();
                let focus_label = match &focus {
                    WaveformFocus::MainSample => {
                        self.current_asset.read().as_ref()
                            .map(|a| format!("Waveform  ·  {}", a.file_name))
                            .unwrap_or("Waveform".to_string())
                    }
                    WaveformFocus::DrumTrack(idx) => {
                        let tracks = self.drum_tracks.read();
                        tracks.get(*idx)
                            .map(|t| format!("Waveform  ·  {} (drum track {})  —  press M while previewing to chop", t.asset.file_name, idx + 1))
                            .unwrap_or("Waveform".to_string())
                    }
                };
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&focus_label).small().color(egui::Color32::from_gray(170)));
                        if !matches!(focus, WaveformFocus::MainSample) {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("↩ Main sample").clicked() {
                                    *self.waveform_focus.write() = WaveformFocus::MainSample;
                                }
                                // ── Preview button for drum tracks ──
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
                        }
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
                        let wave_color = match &focus {
                            WaveformFocus::MainSample => egui::Color32::from_rgb(80, 160, 255),
                            WaveformFocus::DrumTrack(idx) => drum_color(*idx),
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
                        // ── Chop markers for main sample ──────────────
                        if matches!(focus, WaveformFocus::MainSample) {
                            if let Some(asset) = self.current_asset.read().as_ref() {
                                let marks = self.samples_manager.get_marks();
                                let dragged = *self.dragged_mark_index.read();
                                for (idx, mark) in marks.iter().enumerate() {
                                    if mark.sample_name != asset.file_name { continue; }
                                    let mx = rect.left() + mark.position * w;
                                    let color = if dragged == Some(idx) { egui::Color32::WHITE } else { pad_color(idx) };
                                    let sw = if dragged == Some(idx) { 3.0 } else { 2.0 };
                                    painter.vline(mx, rect.y_range(), egui::Stroke::new(sw, color));
                                    let ts = 5.0;
                                    painter.add(egui::Shape::convex_polygon(
                                        vec![egui::pos2(mx, rect.top()), egui::pos2(mx - ts, rect.top() + ts), egui::pos2(mx + ts, rect.top() + ts)],
                                        color, egui::Stroke::new(1.0, color),
                                    ));
                                    painter.text(egui::pos2(mx, rect.top() + ts + 12.0), egui::Align2::CENTER_TOP,
                                        format!("{}", mark.id), egui::FontId::proportional(11.0), color);
                                }
                            }
                        }
                        // ── Chop markers for focused drum track ───────
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
                            }
                        }
                        // ── Playback cursor (shown for any focused waveform while playing) ──
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
                        // ── Drag / seek interaction for main sample ───
                        if matches!(focus, WaveformFocus::MainSample) {
                            if let Some(asset) = self.current_asset.read().as_ref() {
                                let mut dragged_index = self.dragged_mark_index.write();
                                let marks = self.samples_manager.get_marks();
                                if let Some(idx) = *dragged_index {
                                    if idx < marks.len() && marks[idx].sample_name == asset.file_name {
                                        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                            if rect.contains(pos) { self.samples_manager.update_mark_position(idx, (pos.x - rect.left()) / w); }
                                        }
                                        if ui.input(|i| i.pointer.any_released()) { *dragged_index = None; }
                                    } else { *dragged_index = None; }
                                } else if response.drag_started_by(egui::PointerButton::Primary) {
                                    if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                        if rect.contains(pos) {
                                            let nx = (pos.x - rect.left()) / w;
                                            if let Some(idx) = self.samples_manager.find_mark_near(&asset.file_name, nx, 12.0 / w) {
                                                *dragged_index = Some(idx);
                                                self.samples_manager.update_mark_position(idx, nx);
                                            }
                                        }
                                    }
                                }
                            }
                            if self.dragged_mark_index.read().is_none() {
                                if response.dragged() || response.clicked() {
                                    if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                        if rect.contains(pos) {
                                            self.seek_to(((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
                                        }
                                    }
                                }
                            }
                        }
                        // ── Seek interaction for drum track preview ────
                        if let WaveformFocus::DrumTrack(drum_idx) = &focus {
                            if response.dragged() || response.clicked() {
                                if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                    if rect.contains(pos) {
                                        let normalized = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                        self.playback_position.store(normalized, Ordering::Relaxed);
                                        let sp = {
                                            let tracks = self.drum_tracks.read();
                                            tracks.get(*drum_idx).map(|t| (normalized as f64 * t.asset.pcm.len() as f64) as u64)
                                        };
                                        if let Some(sp) = sp {
                                            self.playback_sample_index.store(sp, Ordering::Relaxed);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        let text = match &focus {
                            WaveformFocus::MainSample => if focused_asset.is_none() { "No sample loaded – click Load Sample" } else { "Analyzing waveform..." },
                            WaveformFocus::DrumTrack(_) => "Waveform unavailable",
                        };
                        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text,
                            egui::FontId::monospace(13.0), egui::Color32::from_gray(160));
                    }
                });
                // ── Seek slider (main sample only) ────────────────────
                if matches!(focus, WaveformFocus::MainSample) {
                    if let Some(asset) = self.current_asset.read().as_ref() {
                        ui.add_space(4.0);
                        let dur = asset.frames as f32 / asset.sample_rate as f32;
                        let mut prog = self.playback_position.load(Ordering::Relaxed);
                        ui.horizontal(|ui| {
                            ui.label(format!("{:.2}s", prog * dur));
                            if ui.add(egui::Slider::new(&mut prog, 0.0..=1.0).show_value(false)).changed() { self.seek_to(prog); }
                            ui.label(format!("{:.2}s", dur));
                        });
                    }
                }
                // ── Sample Pads ───────────────────────────────────────
                ui.add_space(8.0);
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Sample Pads").strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Clear All").clicked() { self.samples_manager.clear_marks(); }
                        });
                    });
                    ui.add_space(4.0);
                    let marks = self.samples_manager.get_marks();
                    if marks.is_empty() {
                        ui.label(egui::RichText::new("No pads yet — press M while playing to create chop points").italics().color(egui::Color32::GRAY));
                    } else {
                        let cols = 4;
                        let pw = (ui.available_width() - (cols as f32 - 1.0) * 8.0) / cols as f32;
                        egui::Grid::new("pads").spacing([8.0, 8.0]).min_col_width(pw).show(ui, |ui| {
                            for (idx, mark) in marks.iter().enumerate() {
                                let dur = self.current_asset.read().as_ref().map(|a| a.frames as f32 / a.sample_rate as f32).unwrap_or(0.0);
                                let t = mark.position * dur;
                                let is_active = self.current_asset.read().as_ref().map(|a| a.file_name == mark.sample_name).unwrap_or(false);
                                let is_now = self.is_playing.load(Ordering::Relaxed) && is_active
                                    && (self.playback_position.load(Ordering::Relaxed) - mark.position).abs() < 0.05;
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(pw, 52.0), egui::Sense::click());
                                let base = if is_now { egui::Color32::from_rgb(255,140,0) } else if is_active { pad_color(idx) } else { egui::Color32::from_rgb(35,35,40) };
                                let col = if resp.hovered() { egui::Color32::from_rgb((base.r() as f32*1.3).min(255.0) as u8,(base.g() as f32*1.3).min(255.0) as u8,(base.b() as f32*1.3).min(255.0) as u8) } else { base };
                                ui.painter().rect_filled(rect, 4.0, col);
                                ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(if is_now{3.0}else{1.0}, if is_now{egui::Color32::from_rgb(255,220,100)}else{egui::Color32::from_gray(55)}));
                                let key = ["1","2","3","4","Q","W","E","R","A","S","D","F","Z","X","C","V"].get(idx).copied().unwrap_or("");
                                if !key.is_empty() { ui.painter().text(rect.min+egui::vec2(5.0,3.0), egui::Align2::LEFT_TOP, key, egui::FontId::proportional(10.0), egui::Color32::from_gray(140)); }
                                ui.painter().text(rect.center()-egui::vec2(0.0,7.0), egui::Align2::CENTER_CENTER, format!("{}", mark.id), egui::FontId::proportional(20.0), egui::Color32::WHITE);
                                ui.painter().text(rect.center()+egui::vec2(0.0,10.0), egui::Align2::CENTER_CENTER, format!("{:.2}s", t), egui::FontId::proportional(10.0), egui::Color32::from_gray(200));
                                if resp.clicked() {
                                    if let Some(asset) = self.current_asset.read().clone() {
                                        self.playback_position.store(mark.position, Ordering::Relaxed);
                                        let sp = (mark.position as f64 * asset.pcm.len() as f64) as u64;
                                        self.playback_sample_index.store(sp, Ordering::Relaxed);
                                        self.start_playback(asset);
                                    }
                                }
                                if resp.secondary_clicked() { self.samples_manager.delete_mark(idx); }
                                if (idx + 1) % cols == 0 { ui.end_row(); }
                            }
                        });
                    }
                });
                // ── Step Sequencer ────────────────────────────────────
                let asset_opt = self.current_asset.read().clone();
                if let Some(asset) = asset_opt {
                    ui.add_space(8.0);
                    self.draw_step_sequencer(ui, &asset);
                } else if !self.drum_tracks.read().is_empty() {
                    ui.add_space(8.0);
                    self.draw_step_sequencer_drums_only(ui);
                }
                // ── Global shortcuts ───────────────────────────────────
                if self.current_asset.read().is_none() && self.is_playing.load(Ordering::Relaxed) {
                    // Only stop if we're not previewing a drum track
                    let is_drum_preview = matches!(self.waveform_focus.read().clone(), WaveformFocus::DrumTrack(_));
                    if !is_drum_preview {
                        self.stop_playback();
                    }
                }
                // ── M key — mark chop point on whatever is focused/playing ──
                if self.is_playing.load(Ordering::Relaxed) {
                    if ctx.input(|i| i.key_pressed(egui::Key::M)) {
                        let pos = self.playback_position.load(Ordering::Relaxed);
                        let focus = self.waveform_focus.read().clone();
                        match focus {
                            WaveformFocus::MainSample => {
                                if let Some(asset) = self.current_asset.read().as_ref() {
                                    self.samples_manager.mark_current_position(&asset.file_name, &asset.file_name, pos);
                                    let dur = asset.frames as f32 / asset.sample_rate as f32;
                                    *self.status.write() = format!("✓ Marked at {:.2}s", pos * dur);
                                }
                            }
                            WaveformFocus::DrumTrack(idx) => {
                                // Drop a chop point on the focused drum track
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
                                }
                            }
                        }
                    }
                }
                let key_pad: Vec<(egui::Key, usize)> = vec![
                    (egui::Key::Num1,0),(egui::Key::Num2,1),(egui::Key::Num3,2),(egui::Key::Num4,3),
                    (egui::Key::Q,4),(egui::Key::W,5),(egui::Key::E,6),(egui::Key::R,7),
                    (egui::Key::A,8),(egui::Key::S,9),(egui::Key::D,10),(egui::Key::F,11),
                    (egui::Key::Z,12),(egui::Key::X,13),(egui::Key::C,14),(egui::Key::V,15),
                ];
                let marks = self.samples_manager.get_marks();
                for (key, pidx) in key_pad {
                    if ctx.input(|i| i.key_pressed(key)) && pidx < marks.len() {
                        let mark = &marks[pidx];
                        if let Some(asset) = self.current_asset.read().clone() {
                            self.playback_position.store(mark.position, Ordering::Relaxed);
                            let sp = (mark.position as f64 * asset.pcm.len() as f64) as u64;
                            self.playback_sample_index.store(sp, Ordering::Relaxed);
                            self.start_playback(asset);
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
            }); // end ScrollArea
        });
    }
}