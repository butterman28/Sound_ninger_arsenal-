use eframe::egui;
use std::time::Duration;
use std::sync::atomic::Ordering;

use super::{AppState, WaveformFocus, DrumTrack, NUM_STEPS};
use crate::samples::PlaybackMode;

const PAD_COLORS: &[(u8, u8, u8)] = &[
    (80, 160, 255), (80, 220, 140), (240, 160, 60), (200, 80, 200),
    (240, 80, 80),  (80, 220, 220), (240, 200, 60), (160, 120, 240),
    (255, 120, 160),(100, 220, 180),(200, 140, 60), (120, 160, 240),
];

fn pad_color(idx: usize) -> egui::Color32 {
    let (r, g, b) = PAD_COLORS[idx % PAD_COLORS.len()];
    egui::Color32::from_rgb(r, g, b)
}
fn pad_color_dim(idx: usize) -> egui::Color32 {
    let (r, g, b) = PAD_COLORS[idx % PAD_COLORS.len()];
    egui::Color32::from_rgb(r / 5, g / 5, b / 5)
}

// Drum track rows start at a color offset so they don't clash with chop colors
fn drum_color(idx: usize) -> egui::Color32 {
    pad_color(idx + 4)
}
fn drum_color_dim(idx: usize) -> egui::Color32 {
    pad_color_dim(idx + 4)
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick_sequencer();
        self.draw_piano_roll(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Audio Sampler");

            // ── Transport ─────────────────────────────────────
            ui.horizontal(|ui| {
                if ui.button("Load Sample").clicked() {
                    self.stop_playback();
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

            // ── Playback Region Controls ──────────────────────
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

            // ── Sample Info ───────────────────────────────────
            if let Some(asset) = self.current_asset.read().as_ref() {
                ui.add_space(4.0);
                ui.collapsing("Sample Info", |ui| {
                    ui.label(format!("Name: {}", asset.file_name));
                    ui.label(format!("Sample Rate: {} Hz | Channels: {} | Duration: {:.3}s",
                        asset.sample_rate, asset.channels, asset.frames as f32 / asset.sample_rate as f32));
                });
            }

            // ──────────────────────────────────────────────────
            //  WAVEFORM DISPLAY
            //  Shows whichever row/track is currently focused.
            //  Clicking a sequencer row label changes the focus.
            // ──────────────────────────────────────────────────
            ui.add_space(8.0);

            // Focus banner
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
                        .map(|t| format!("Waveform  ·  {} (drum track {})", t.asset.file_name, idx + 1))
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

                    // Waveform color based on focus
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

                    // Draw chop markers only when showing main sample
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

                    // Playhead (only on main sample)
                    if matches!(focus, WaveformFocus::MainSample) {
                        let prog = self.playback_position.load(Ordering::Relaxed);
                        let px = rect.left() + prog * w;
                        painter.vline(px, rect.y_range(), egui::Stroke::new(2.5, egui::Color32::from_rgb(255, 80, 80)));
                        let ts = 8.0;
                        painter.add(egui::Shape::convex_polygon(
                            vec![egui::pos2(px, rect.top() + ts), egui::pos2(px - ts, rect.top()), egui::pos2(px + ts, rect.top())],
                            egui::Color32::from_rgb(255, 80, 80), egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
                        ));
                    }

                    // Marker dragging (main sample only)
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
                    }

                    // Click to seek (main sample only)
                    if matches!(focus, WaveformFocus::MainSample) && self.dragged_mark_index.read().is_none() {
                        if response.dragged() || response.clicked() {
                            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                if rect.contains(pos) {
                                    self.seek_to(((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
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

            // ── Seek slider (main sample only) ────────────────
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

            // ── Sample Pads ───────────────────────────────────
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

            // ── Step Sequencer ────────────────────────────────
            let asset_opt = self.current_asset.read().clone();
            if let Some(asset) = asset_opt {
                ui.add_space(8.0);
                self.draw_step_sequencer(ui, &asset);
            } else if !self.drum_tracks.read().is_empty() {
                // Still show sequencer even if no main sample, for drum tracks
                ui.add_space(8.0);
                self.draw_step_sequencer_drums_only(ui);
            }
        }); // CentralPanel

        // ── Global shortcuts ──────────────────────────────────
        if self.current_asset.read().is_none() && self.is_playing.load(Ordering::Relaxed) {
            self.stop_playback();
        }
        if self.is_playing.load(Ordering::Relaxed) {
            if ctx.input(|i| i.key_pressed(egui::Key::M)) {
                if let Some(asset) = self.current_asset.read().as_ref() {
                    let pos = self.playback_position.load(Ordering::Relaxed);
                    self.samples_manager.mark_current_position(&asset.file_name, &asset.file_name, pos);
                    let dur = asset.frames as f32 / asset.sample_rate as f32;
                    *self.status.write() = format!("✓ Marked at {:.2}s", pos * dur);
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
    }
}

// ─────────────────────────────────────────────────────────────
//  Step Sequencer — chop rows + drum track rows
// ─────────────────────────────────────────────────────────────
impl AppState {
    fn seq_header_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("STEP SEQUENCER").small().strong().color(egui::Color32::from_gray(100)));
            ui.separator();
            let mut bpm = self.seq_bpm.load(Ordering::Relaxed);
            ui.label(egui::RichText::new("BPM").small().color(egui::Color32::from_gray(120)));
            if ui.add(egui::DragValue::new(&mut bpm).speed(0.5).clamp_range(40.0..=300.0).fixed_decimals(0)).changed() {
                self.seq_bpm.store(bpm, Ordering::Relaxed);
            }
            ui.separator();
            let playing = self.seq_playing.load(Ordering::Relaxed);
            let (lbl, col) = if playing { ("⏹ Stop", egui::Color32::from_rgb(220,80,60)) } else { ("▶ Play", egui::Color32::from_rgb(60,200,100)) };
            if ui.add(egui::Button::new(egui::RichText::new(lbl).color(col).small())).clicked() {
                if playing { self.stop_sequencer(); } else { self.start_sequencer(); }
            }
            if ui.add(egui::Button::new(egui::RichText::new("🗑 Clear").small().color(egui::Color32::from_gray(120)))).clicked() {
                let mut g = self.seq_grid.write();
                for s in g.iter_mut() { s.clear(); }
                let mut tracks = self.drum_tracks.write();
                for t in tracks.iter_mut() { t.steps = [false; NUM_STEPS]; }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("🎹 Piano Roll").small().color(egui::Color32::from_rgb(140,180,255)))).clicked() {
                    *self.piano_roll_open.write() = true;
                }
                // Add drum track button
                if ui.add(egui::Button::new(egui::RichText::new("＋ Add Track").small().color(egui::Color32::from_rgb(80,220,140)))).clicked() {
                    self.load_drum_track();
                }
            });
        });
    }

    fn draw_beat_header(ui: &mut egui::Ui, label_w: f32, step_w: f32) {
        ui.horizontal(|ui| {
            ui.add_space(label_w + 8.0);
            for step in 0..NUM_STEPS {
                let sz = egui::vec2(step_w - 2.0, 13.0);
                let (r, _) = ui.allocate_exact_size(sz, egui::Sense::hover());
                if step % 4 == 0 {
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER,
                        format!("{}", step / 4 + 1), egui::FontId::proportional(9.0), egui::Color32::from_gray(75));
                }
                let tc = if step % 4 == 0 { egui::Color32::from_gray(65) } else { egui::Color32::from_gray(38) };
                ui.painter().vline(r.left(), r.y_range(), egui::Stroke::new(0.5, tc));
            }
        });
    }

    fn draw_step_buttons(
        ui: &mut egui::Ui,
        step_w: f32, row_h: f32,
        color: egui::Color32, color_dim: egui::Color32,
        is_ons: &[bool; NUM_STEPS],
        current_step: usize, seq_playing: bool,
        on_toggle: &mut dyn FnMut(usize),
    ) {
        for step in 0..NUM_STEPS {
            let is_on = is_ons[step];
            let is_cur = seq_playing && current_step == step;
            let sz = egui::vec2(step_w - 2.0, row_h);
            let (sr, sresp) = ui.allocate_exact_size(sz, egui::Sense::click());
            let grp = step / 4;
            let bg = if grp % 2 == 0 { egui::Color32::from_rgb(25,25,33) } else { egui::Color32::from_rgb(21,21,29) };
            ui.painter().rect_filled(sr, 2.0, bg);
            ui.painter().rect_filled(sr.shrink(2.0), 2.0, if is_on { color } else { color_dim });
            if is_on {
                ui.painter().hline(sr.shrink(2.0).x_range(), sr.shrink(2.0).top() + 1.5,
                    egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(255,255,255,70)));
            }
            if is_cur {
                ui.painter().rect_filled(sr, 2.0, egui::Color32::from_rgba_unmultiplied(255,220,80,45));
                ui.painter().rect_stroke(sr, 2.0, egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(255,220,80,180)));
            } else {
                ui.painter().rect_stroke(sr, 2.0, egui::Stroke::new(0.5, egui::Color32::from_gray(36)));
            }
            if sresp.hovered() {
                ui.painter().rect_stroke(sr, 2.0, egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255,255,255,50)));
            }
            if sresp.clicked() { on_toggle(step); }
        }
    }

    fn load_drum_track(&self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3","wav","flac","ogg","m4a","aac"])
            .pick_file()
        {
            let audio_manager = self.audio_manager.clone();
            let drum_tracks = self.drum_tracks.clone();
            let drum_loading = self.drum_loading.clone();
            let status = self.status.clone();
            drum_loading.store(true, Ordering::Relaxed);

            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    audio_manager.load_audio(path.to_str().unwrap_or(""))
                }));
                match result {
                    Ok(Ok(asset)) => {
                        let waveform = audio_manager.analyze_waveform(&asset, 400);
                        let track = DrumTrack::new(asset.clone(), Some(waveform));
                        drum_tracks.write().push(track);
                        *status.write() = format!("✓ Track added: {}", asset.file_name);
                    }
                    Ok(Err(e)) => { *status.write() = format!("✗ Track load error: {}", e); }
                    Err(_) => { *status.write() = "✗ Track load crashed".to_string(); }
                }
                drum_loading.store(false, Ordering::Relaxed);
            });
        }
    }

    fn draw_step_sequencer(&mut self, ui: &mut egui::Ui, asset: &crate::audio::AudioAsset) {
    let frame = egui::Frame::none()
        .fill(egui::Color32::from_rgb(15, 15, 21))
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(38)));
    frame.show(ui, |ui| {
        self.seq_header_ui(ui);
        ui.add_space(5.0);
        
        let label_w = 158.0;
        let step_w = 42.0; // Fixed step width for scrolling
        let row_h = 26.0;
        let current_step = *self.seq_current_step.read();
        let seq_playing = self.seq_playing.load(Ordering::Relaxed);
        
        // ── Scrollable area (steps only, labels stay fixed) ────────────────
        egui::ScrollArea::horizontal()
            .auto_shrink(false)
            .show(ui, |ui| {
                // Beat header
                ui.horizontal(|ui| {
                    ui.add_space(label_w + 8.0);
                    for step in 0..NUM_STEPS {
                        let sz = egui::vec2(step_w - 2.0, 13.0);
                        let (r, _) = ui.allocate_exact_size(sz, egui::Sense::hover());
                        if step % 4 == 0 {
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER,
                                format!("{}", step / 4 + 1), egui::FontId::proportional(9.0), egui::Color32::from_gray(75));
                        }
                        let tc = if step % 4 == 0 { egui::Color32::from_gray(65) } else { egui::Color32::from_gray(38) };
                        ui.painter().vline(r.left(), r.y_range(), egui::Stroke::new(0.5, tc));
                    }
                });
                
                // ── Chop rows ─────────────────────────────────────
                let marks = self.samples_manager.get_marks_for_sample(&asset.file_name);
                let has_chops = !marks.is_empty();
                if has_chops {
                    ui.label(egui::RichText::new("  Chops").small().color(egui::Color32::from_gray(70)));
                }
                for (pad_idx, mark) in marks.iter().enumerate() {
                    let dur = asset.frames as f32 / asset.sample_rate as f32;
                    let time_at = mark.position * dur;
                    let color = pad_color(pad_idx);
                    let color_dim = pad_color_dim(pad_idx);
                    let is_focused = matches!(self.waveform_focus.read().clone(), WaveformFocus::MainSample);
                    
                    ui.horizontal(|ui| {
                        // Label (fixed, outside scroll)
                        let (lr, lresp) = ui.allocate_exact_size(egui::vec2(label_w, row_h), egui::Sense::click());
                        let label_bg = if is_focused { egui::Color32::from_rgb(24, 28, 38) } else { egui::Color32::from_rgb(20,20,28) };
                        ui.painter().rect_filled(lr, 3.0, label_bg);
                        ui.painter().rect_stroke(lr, 3.0, egui::Stroke::new(1.0, egui::Color32::from_gray(38)));
                        ui.painter().rect_filled(egui::Rect::from_min_size(lr.min + egui::vec2(5.0, 7.0), egui::vec2(4.0, row_h - 14.0)), 2.0, color);
                        ui.painter().text(egui::pos2(lr.min.x + 15.0, lr.center().y - 4.0), egui::Align2::LEFT_CENTER,
                            format!("Chop #{}", mark.id), egui::FontId::proportional(11.0), color);
                        ui.painter().text(egui::pos2(lr.min.x + 15.0, lr.center().y + 6.0), egui::Align2::LEFT_CENTER,
                            format!("{:.2}s", time_at), egui::FontId::proportional(8.5), egui::Color32::from_gray(95));
                        
                        if lresp.clicked() {
                            *self.waveform_focus.write() = WaveformFocus::MainSample;
                        }
                        
                        lresp.context_menu(|ui| {
                            ui.set_min_width(155.0);
                            ui.label(egui::RichText::new(format!("Chop #{} @ {:.2}s", mark.id, time_at)).small().color(egui::Color32::from_gray(140)));
                            ui.separator();
                            if ui.button("🎹  Open Piano Roll").clicked() { *self.piano_roll_open.write() = true; ui.close_menu(); }
                            ui.separator();
                            if seq_playing {
                                if ui.button("⏹  Stop Pattern").clicked() { self.stop_sequencer(); ui.close_menu(); }
                            } else {
                                if ui.button("▶  Play Pattern").clicked() { self.start_sequencer(); ui.close_menu(); }
                            }
                            if ui.button("🗑  Clear Chop Steps").clicked() {
                                let mut g = self.seq_grid.write();
                                for s in g.iter_mut() { s.retain(|&p| p != pad_idx); }
                                ui.close_menu();
                            }
                        });
                        
                        ui.add_space(8.0);
                        
                        // Step buttons (scrollable)
                        let grid_snap = self.seq_grid.read().clone();
                        let is_ons: [bool; NUM_STEPS] = std::array::from_fn(|s| grid_snap[s].contains(&pad_idx));
                        Self::draw_step_buttons(ui, step_w, row_h, color, color_dim, &is_ons, current_step, seq_playing,
                            &mut |step| {
                                let mut grid = self.seq_grid.write();
                                let sp = &mut grid[step];
                                if let Some(i) = sp.iter().position(|&p| p == pad_idx) { sp.remove(i); } else { sp.push(pad_idx); }
                            }
                        );
                    });
                }
                
                // ── Drum track rows ───────────────────────────────
                let n_drums = self.drum_tracks.read().len();
                if n_drums > 0 {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("  Drum Tracks").small().color(egui::Color32::from_gray(70)));
                    for drum_idx in 0..n_drums {
                        let color = drum_color(drum_idx);
                        let color_dim = drum_color_dim(drum_idx);
                        let (file_name, time_str, muted, steps) = {
                            let tracks = self.drum_tracks.read();
                            let t = &tracks[drum_idx];
                            (
                                t.asset.file_name.clone(),
                                format!("{:.2}s", t.asset.frames as f32 / t.asset.sample_rate as f32),
                                t.muted,
                                t.steps,
                            )
                        };
                        let is_focused = matches!(self.waveform_focus.read().clone(), WaveformFocus::DrumTrack(i) if i == drum_idx);
                        
                        ui.horizontal(|ui| {
                            // Label (fixed)
                            let (lr, lresp) = ui.allocate_exact_size(egui::vec2(label_w, row_h), egui::Sense::click());
                            let label_bg = if is_focused { egui::Color32::from_rgb(20, 30, 25) } else { egui::Color32::from_rgb(20,20,28) };
                            ui.painter().rect_filled(lr, 3.0, if muted { egui::Color32::from_rgb(18,18,22) } else { label_bg });
                            ui.painter().rect_stroke(lr, 3.0, egui::Stroke::new(
                                if is_focused { 1.5 } else { 1.0 },
                                if is_focused { color } else { egui::Color32::from_gray(38) },
                            ));
                            let swatch_col = if muted { egui::Color32::from_gray(50) } else { color };
                            ui.painter().rect_filled(egui::Rect::from_min_size(lr.min + egui::vec2(5.0, 7.0), egui::vec2(4.0, row_h - 14.0)), 2.0, swatch_col);
                            
                            let display_name = if file_name.len() > 16 { format!("{}…", &file_name[..14]) } else { file_name.clone() };
                            let text_col = if muted { egui::Color32::from_gray(80) } else { color };
                            ui.painter().text(egui::pos2(lr.min.x + 15.0, lr.center().y - 4.0), egui::Align2::LEFT_CENTER,
                                display_name, egui::FontId::proportional(11.0), text_col);
                            ui.painter().text(egui::pos2(lr.min.x + 15.0, lr.center().y + 6.0), egui::Align2::LEFT_CENTER,
                                &time_str, egui::FontId::proportional(8.5), egui::Color32::from_gray(90));
                            
                            if lresp.clicked() {
                                *self.waveform_focus.write() = WaveformFocus::DrumTrack(drum_idx);
                                *self.status.write() = format!("Showing waveform: {}", file_name);
                            }
                            
                            let drum_tracks_ref = self.drum_tracks.clone();
                            lresp.context_menu(|ui| {
                                ui.set_min_width(160.0);
                                ui.label(egui::RichText::new(&file_name).small().color(egui::Color32::from_gray(140)));
                                ui.separator();
                                if ui.button(if muted { "🔊  Unmute" } else { "🔇  Mute" }).clicked() {
                                    if let Some(t) = drum_tracks_ref.write().get_mut(drum_idx) { t.muted = !t.muted; }
                                    ui.close_menu();
                                }
                                if ui.button("🗑  Clear Steps").clicked() {
                                    if let Some(t) = drum_tracks_ref.write().get_mut(drum_idx) { t.steps = [false; NUM_STEPS]; }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button(egui::RichText::new("✕  Remove Track").color(egui::Color32::from_rgb(220,80,60))).clicked() {
                                    drum_tracks_ref.write().remove(drum_idx);
                                    ui.close_menu();
                                }
                            });
                            
                            ui.add_space(8.0);
                            
                            // Step buttons (scrollable)
                            Self::draw_step_buttons(ui, step_w, row_h, color, color_dim, &steps, current_step, seq_playing,
                                &mut |step| {
                                    if let Some(t) = self.drum_tracks.write().get_mut(drum_idx) {
                                        t.steps[step] = !t.steps[step];
                                    }
                                }
                            );
                        });
                    }
                }
                
                if !has_chops && n_drums == 0 {
                    ui.label(egui::RichText::new("No chops yet — press M while playing to create chop points, or click ＋ Add Track to load a drum sample")
                        .small().color(egui::Color32::from_gray(80)).italics());
                }
                ui.add_space(3.0);
                ui.label(egui::RichText::new("Click steps to toggle  ·  Click row label to preview waveform  ·  Right-click label for options")
                    .small().color(egui::Color32::from_gray(58)));
            });
    });
}

    fn draw_step_sequencer_drums_only(&mut self, ui: &mut egui::Ui) {
    let frame = egui::Frame::none()
        .fill(egui::Color32::from_rgb(15,15,21))
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(38)));
    frame.show(ui, |ui| {
        self.seq_header_ui(ui);
        
        let label_w = 158.0;
        let step_w = 42.0; // Fixed step width for scrolling
        let row_h = 26.0;
        let current_step = *self.seq_current_step.read();
        let seq_playing = self.seq_playing.load(Ordering::Relaxed);
        
        ui.add_space(5.0);
        
        // ── Scrollable area ─────────────────────────────────────
        egui::ScrollArea::horizontal()
            .auto_shrink(false)
            .show(ui, |ui| {
                // Beat header
                ui.horizontal(|ui| {
                    ui.add_space(label_w + 8.0);
                    for step in 0..NUM_STEPS {
                        let sz = egui::vec2(step_w - 2.0, 13.0);
                        let (r, _) = ui.allocate_exact_size(sz, egui::Sense::hover());
                        if step % 4 == 0 {
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER,
                                format!("{}", step / 4 + 1), egui::FontId::proportional(9.0), egui::Color32::from_gray(75));
                        }
                        let tc = if step % 4 == 0 { egui::Color32::from_gray(65) } else { egui::Color32::from_gray(38) };
                        ui.painter().vline(r.left(), r.y_range(), egui::Stroke::new(0.5, tc));
                    }
                });
                
                let n_drums = self.drum_tracks.read().len();
                for drum_idx in 0..n_drums {
                    let color = drum_color(drum_idx);
                    let color_dim = drum_color_dim(drum_idx);
                    let (file_name, steps, muted) = {
                        let tracks = self.drum_tracks.read();
                        let t = &tracks[drum_idx];
                        (t.asset.file_name.clone(), t.steps, t.muted)
                    };
                    let is_focused = matches!(self.waveform_focus.read().clone(), WaveformFocus::DrumTrack(i) if i == drum_idx);
                    
                    ui.horizontal(|ui| {
                        let (lr, lresp) = ui.allocate_exact_size(egui::vec2(label_w, row_h), egui::Sense::click());
                        ui.painter().rect_filled(lr, 3.0, if is_focused { egui::Color32::from_rgb(20,30,25) } else { egui::Color32::from_rgb(20,20,28) });
                        ui.painter().rect_stroke(lr, 3.0, egui::Stroke::new(if is_focused{1.5}else{1.0}, if is_focused{color}else{egui::Color32::from_gray(38)}));
                        ui.painter().rect_filled(egui::Rect::from_min_size(lr.min + egui::vec2(5.0,7.0), egui::vec2(4.0, row_h-14.0)), 2.0, if muted{egui::Color32::from_gray(50)}else{color});
                        
                        let dn = if file_name.len() > 16 { format!("{}…", &file_name[..14]) } else { file_name.clone() };
                        ui.painter().text(egui::pos2(lr.min.x+15.0, lr.center().y-4.0), egui::Align2::LEFT_CENTER, dn, egui::FontId::proportional(11.0), if muted{egui::Color32::from_gray(80)}else{color});
                        
                        if lresp.clicked() { *self.waveform_focus.write() = WaveformFocus::DrumTrack(drum_idx); }
                        
                        let drum_tracks_ref = self.drum_tracks.clone();
                        lresp.context_menu(|ui| {
                            ui.set_min_width(155.0);
                            if ui.button(if muted{"🔊 Unmute"}else{"🔇 Mute"}).clicked() {
                                if let Some(t) = drum_tracks_ref.write().get_mut(drum_idx) { t.muted = !t.muted; }
                                ui.close_menu();
                            }
                            if ui.button("🗑 Clear Steps").clicked() {
                                if let Some(t) = drum_tracks_ref.write().get_mut(drum_idx) { t.steps = [false; NUM_STEPS]; }
                                ui.close_menu();
                            }
                            if ui.button(egui::RichText::new("✕ Remove").color(egui::Color32::from_rgb(220,80,60))).clicked() {
                                drum_tracks_ref.write().remove(drum_idx); ui.close_menu();
                            }
                        });
                        
                        ui.add_space(8.0);
                        
                        Self::draw_step_buttons(ui, step_w, row_h, color, color_dim, &steps, current_step, seq_playing,
                            &mut |step| { if let Some(t) = self.drum_tracks.write().get_mut(drum_idx) { t.steps[step] = !t.steps[step]; } });
                    });
                }
            });
    });
}
}

// ─────────────────────────────────────────────────────────────
//  Piano Roll
// ─────────────────────────────────────────────────────────────
impl AppState {
    fn draw_piano_roll(&mut self, ctx: &egui::Context) {
        if !*self.piano_roll_open.read() { return; }
        let asset_opt = self.current_asset.read().clone();
        let asset = match &asset_opt { Some(a) => a.clone(), None => { *self.piano_roll_open.write() = false; return; } };
        let marks = self.samples_manager.get_marks_for_sample(&asset.file_name);
        let dur = asset.frames as f32 / asset.sample_rate as f32;
        let mut window_open = true;

        egui::Window::new(format!("🎹 Piano Roll — {}", asset.file_name))
            .id(egui::Id::new("piano_roll_window"))
            .default_size([820.0, 400.0])
            .min_size([500.0, 260.0])
            .resizable(true)
            .collapsible(false)
            .open(&mut window_open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let playing = self.seq_playing.load(Ordering::Relaxed);
                    let (lbl, col) = if playing { ("⏹ Stop", egui::Color32::from_rgb(220,80,60)) } else { ("▶ Play Pattern", egui::Color32::from_rgb(60,200,100)) };
                    if ui.add(egui::Button::new(egui::RichText::new(lbl).color(col))).clicked() {
                        if playing { self.stop_sequencer(); } else { self.start_sequencer(); }
                    }
                    let mut bpm = self.seq_bpm.load(Ordering::Relaxed);
                    ui.label("BPM");
                    if ui.add(egui::DragValue::new(&mut bpm).speed(0.5).clamp_range(40.0..=300.0).fixed_decimals(0)).changed() { self.seq_bpm.store(bpm, Ordering::Relaxed); }
                    ui.separator();
                    if ui.button(egui::RichText::new("Clear All").color(egui::Color32::from_rgb(200,80,80))).clicked() {
                        let mut g = self.seq_grid.write();
                        for s in g.iter_mut() { s.clear(); }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("Click cell to toggle  ·  Rows = chops").small().color(egui::Color32::from_gray(95)));
                    });
                });
                ui.separator();
                ui.add_space(4.0);

                if marks.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new("No chops yet!\nPlay your sample and press M to drop markers.").size(13.0).color(egui::Color32::from_gray(140)));
                    });
                    return;
                }

                let pad_label_w = 165.0;
                let avail = ui.available_size();
                let grid_w = (avail.x - pad_label_w - 14.0).max(200.0);
                let cell_w = grid_w / NUM_STEPS as f32;
                let cell_h = 34.0;
                let n_rows = marks.len();
                let header_h = 18.0;
                let grid_h = n_rows as f32 * cell_h + header_h;
                let current_step = *self.seq_current_step.read();
                let grid_snap = self.seq_grid.read().clone();

                egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
                    let (outer_rect, _) = ui.allocate_exact_size(egui::vec2(pad_label_w + grid_w + 8.0, grid_h + 4.0), egui::Sense::hover());
                    let painter = ui.painter_at(outer_rect);
                    painter.rect_filled(outer_rect, 0.0, egui::Color32::from_rgb(13, 13, 19));
                    let grid_origin = egui::pos2(outer_rect.min.x + pad_label_w, outer_rect.min.y + header_h);

                    // Step header
                    for step in 0..NUM_STEPS {
                        let x = grid_origin.x + step as f32 * cell_w;
                        let hr = egui::Rect::from_min_size(egui::pos2(x, outer_rect.min.y), egui::vec2(cell_w-1.0, header_h-1.0));
                        let grp = step / 4;
                        painter.rect_filled(hr, 0.0, if grp%2==0{egui::Color32::from_rgb(21,21,31)}else{egui::Color32::from_rgb(17,17,27)});
                        if step%4==0 { painter.text(hr.center(), egui::Align2::CENTER_CENTER, format!("{}", step/4+1), egui::FontId::proportional(10.0), egui::Color32::from_gray(110)); }
                        else { painter.circle_filled(hr.center(), 1.5, egui::Color32::from_gray(50)); }
                        if self.seq_playing.load(Ordering::Relaxed) && current_step == step {
                            painter.rect_filled(hr, 0.0, egui::Color32::from_rgba_unmultiplied(255,220,80,38));
                        }
                    }

                    // Pad rows
                    for (pad_idx, mark) in marks.iter().enumerate() {
                        let time_at = mark.position * dur;
                        let color = pad_color(pad_idx);
                        let color_dim = pad_color_dim(pad_idx);
                        let y = grid_origin.y + pad_idx as f32 * cell_h;

                        let lr = egui::Rect::from_min_size(egui::pos2(outer_rect.min.x, y), egui::vec2(pad_label_w - 3.0, cell_h - 1.0));
                        painter.rect_filled(lr, 0.0, if pad_idx%2==0{egui::Color32::from_rgb(19,19,27)}else{egui::Color32::from_rgb(16,16,24)});
                        painter.rect_filled(egui::Rect::from_min_size(lr.min+egui::vec2(5.0,9.0), egui::vec2(4.0, cell_h-18.0)), 2.0, color);
                        painter.text(egui::pos2(lr.min.x+15.0, lr.center().y-6.0), egui::Align2::LEFT_CENTER, format!("Chop #{}", mark.id), egui::FontId::proportional(12.0), color);
                        painter.text(egui::pos2(lr.min.x+15.0, lr.center().y+7.0), egui::Align2::LEFT_CENTER, format!("{:.3}s", time_at), egui::FontId::proportional(9.0), egui::Color32::from_gray(105));
                        painter.hline(outer_rect.x_range(), y + cell_h - 0.5, egui::Stroke::new(0.5, egui::Color32::from_gray(26)));

                        for step in 0..NUM_STEPS {
                            let x = grid_origin.x + step as f32 * cell_w;
                            let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w-1.0, cell_h-1.0));
                            let grp = step / 4;
                            painter.rect_filled(cell, 0.0, if grp%2==0{egui::Color32::from_rgb(19,19,27)}else{egui::Color32::from_rgb(16,16,24)});
                            let is_on = grid_snap[step].contains(&pad_idx);
                            if is_on {
                                painter.rect_filled(cell.shrink(2.0), 3.0, color);
                                painter.hline(cell.shrink(2.0).x_range(), cell.shrink(2.0).top()+1.5, egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(255,255,255,70)));
                            } else {
                                painter.rect_filled(cell.shrink(3.0), 2.0, color_dim);
                            }
                            if self.seq_playing.load(Ordering::Relaxed) && current_step == step {
                                painter.rect_filled(cell, 0.0, egui::Color32::from_rgba_unmultiplied(255,220,80,30));
                            }
                            let lc = if step%4==0{egui::Color32::from_gray(48)}else{egui::Color32::from_gray(26)};
                            painter.vline(x, egui::Rangef::new(y, y+cell_h), egui::Stroke::new(0.5, lc));
                        }
                    }

                    // Click handling
                    let grid_rect = egui::Rect::from_min_size(grid_origin, egui::vec2(grid_w, n_rows as f32 * cell_h));
                    let gresp = ui.interact(grid_rect, egui::Id::new("pr_grid"), egui::Sense::click_and_drag());
                    if gresp.clicked() || gresp.dragged() {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            if grid_rect.contains(pos) {
                                let step = (((pos.x - grid_origin.x) / cell_w) as usize).min(NUM_STEPS - 1);
                                let row  = (((pos.y - grid_origin.y) / cell_h) as usize).min(n_rows - 1);
                                let mut grid = self.seq_grid.write();
                                let sp = &mut grid[step];
                                if let Some(i) = sp.iter().position(|&p| p == row) {
                                    if gresp.clicked() { sp.remove(i); }
                                } else { sp.push(row); }
                            }
                        }
                    }
                });
            });

        if !window_open { *self.piano_roll_open.write() = false; }
    }
}