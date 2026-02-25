// src/gui/ui/panels.rs
use eframe::egui;
use std::sync::atomic::Ordering;
use crate::gui::{AppState, WaveformFocus, NUM_STEPS};
use super::widgets::*;
use crate::adsr::ADSREnvelope;

impl AppState {
    pub fn seq_header_ui(&mut self, ui: &mut egui::Ui) {
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
                for t in tracks.iter_mut() {
                    t.steps = [false; NUM_STEPS];
                    for row in t.chop_steps.iter_mut() { *row = [false; NUM_STEPS]; }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("🎹 Piano Roll").small().color(egui::Color32::from_rgb(140,180,255)))).clicked() {
                    *self.piano_roll_open.write() = true;
                }
                if ui.add(egui::Button::new(egui::RichText::new("＋ Add Track").small().color(egui::Color32::from_rgb(80,220,140)))).clicked() {
                    self.load_drum_track();
                }
            });
        });
    }

    pub fn draw_step_sequencer(&mut self, ui: &mut egui::Ui) {
        let label_w    = 130.0;
        let step_w     = 38.0;
        let steps_total = step_w * NUM_STEPS as f32;
        let row_h      = 36.0;
        let knob_h     = 52.0;
        let frame = egui::Frame::none()
            .fill(egui::Color32::from_rgb(15, 15, 21))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(38)));
        frame.show(ui, |ui| {
            self.seq_header_ui(ui);
            ui.add_space(4.0);
            let current_step = *self.seq_current_step.read();
            let seq_playing  = self.seq_playing.load(Ordering::Relaxed);
            // Beat-number header
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
            // ── All tracks (including main) ───────────────────────────────
            let n_drums = self.drum_tracks.read().len();
            let main_idx = *self.main_track_index.read();
            if n_drums > 0 {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("  Tracks").small().color(egui::Color32::from_gray(70)));
            }
            // ── In src/gui/ui/panels.rs, find the drum track loop ─────────

for drum_idx in 0..n_drums {
    let color     = drum_color(drum_idx);
    let color_dim = drum_color_dim(drum_idx);
    let (file_name, time_str, muted) = {
        let tracks = self.drum_tracks.read();
        let t = &tracks[drum_idx];
        (t.asset.file_name.clone(),
        format!("{:.2}s", t.asset.frames as f32 / t.asset.sample_rate as f32),
        t.muted)
    };
    let is_focused = matches!(self.waveform_focus.read().clone(), WaveformFocus::DrumTrack(i) if i == drum_idx);
    
    // ── CRITICAL: Get chop marks AND ensure arrays are sized ─────────
    let chop_marks = self.samples_manager.get_marks_for_sample(&file_name);
    let has_chops = !chop_marks.is_empty();
    
    // Ensure DrumTrack has enough chop_steps and chop_adsr entries
    {
        let mut tracks = self.drum_tracks.write();
        if let Some(t) = tracks.get_mut(drum_idx) {
            t.ensure_chop_steps(chop_marks.len());
        }
    }
    
    // Track header (keep existing code...)
    ui.horizontal(|ui| {
        // ... (keep existing track header code)
    });
    
    // ── ALWAYS show the main track row ─────────────────────────────
    {
        let steps = {
            let tracks = self.drum_tracks.read();
            tracks.get(drum_idx).map(|t| t.steps).unwrap_or([false; NUM_STEPS])
        };
        
        // Main sample step row
        ui.horizontal(|ui| {
            let (lr, lresp) = ui.allocate_exact_size(egui::vec2(label_w, row_h), egui::Sense::click());
            let label_bg = if is_focused { egui::Color32::from_rgb(20,30,25) } else { egui::Color32::from_rgb(20,20,28) };
            ui.painter().rect_filled(lr, 3.0, if muted { egui::Color32::from_rgb(18,18,22) } else { label_bg });
            ui.painter().rect_stroke(lr, 3.0, egui::Stroke::new(
                if is_focused { 1.5 } else { 1.0 },
                if is_focused { color } else { egui::Color32::from_gray(38) },
            ));
            ui.painter().rect_filled(egui::Rect::from_min_size(lr.min+egui::vec2(5.0, 6.0), egui::vec2(4.0, row_h-12.0)), 2.0,
                if muted { egui::Color32::from_gray(50) } else { color });
            let dn = if file_name.len() > 14 { format!("{}…", &file_name[..12]) } else { file_name.clone() };
            ui.painter().text(egui::pos2(lr.min.x+14.0, lr.center().y-5.0), egui::Align2::LEFT_CENTER,
                dn, egui::FontId::proportional(11.0), if muted { egui::Color32::from_gray(80) } else { color });
            ui.painter().text(egui::pos2(lr.min.x+14.0, lr.center().y+6.0), egui::Align2::LEFT_CENTER,
                &time_str, egui::FontId::proportional(8.5), egui::Color32::from_gray(90));
            if lresp.clicked() {
                *self.waveform_focus.write() = WaveformFocus::DrumTrack(drum_idx);
                *self.status.write() = format!("Previewing: {}", file_name);
                if let Some(track) = self.drum_tracks.read().get(drum_idx) {
                    self.playback_position.store(0.0, Ordering::Relaxed);
                    self.playback_sample_index.store(0, Ordering::Relaxed);
                    self.start_playback(track.asset.clone());
                }
            }
            ui.add_space(8.0);
            draw_step_buttons(ui, step_w, row_h, color, color_dim, &steps, current_step, seq_playing,
                &mut |step| {
                    if let Some(t) = self.drum_tracks.write().get_mut(drum_idx) { t.steps[step] = !t.steps[step]; }
                }
            );
        });
        
        // Main track ADSR
        ui.horizontal(|ui| {
            let (label_space, _) = ui.allocate_exact_size(egui::vec2(label_w, knob_h), egui::Sense::hover());
            ui.painter().rect_filled(label_space, 0.0, egui::Color32::from_rgb(12, 12, 18));
            ui.add_space(8.0);
            let (knob_rect, _) = ui.allocate_exact_size(egui::vec2(steps_total, knob_h), egui::Sense::hover());
            ui.painter().rect_filled(knob_rect, 2.0, egui::Color32::from_rgb(16, 16, 24));
            ui.painter().rect_stroke(knob_rect, 2.0, egui::Stroke::new(0.5, egui::Color32::from_gray(30)));
            let adsr_now = self.drum_tracks.read().get(drum_idx).map(|t| t.adsr).unwrap_or_default();
            let base_id = egui::Id::new("drum_knob").with(drum_idx);
            let painter = ui.painter().clone();
            let (new_adsr, adsr_changed) = draw_adsr_knobs(ui, &painter, knob_rect, adsr_now, color, base_id);
            if adsr_changed {
                if let Some(t) = self.drum_tracks.write().get_mut(drum_idx) { t.adsr = new_adsr; }
            }
        });
    }
    
    // ── SHOW CHOP ROWS WHEN THEY EXIST ────────────────────────────
    if has_chops {
        for (chop_idx, mark) in chop_marks.iter().enumerate() {
            let chop_color = pad_color(chop_idx);
            let chop_color_dim = pad_color_dim(chop_idx);
            let dur_asset = {
                let tracks = self.drum_tracks.read();
                tracks.get(drum_idx).map(|t| t.asset.frames as f32 / t.asset.sample_rate as f32).unwrap_or(0.0)
            };
            let time_at = mark.position * dur_asset;
            
            // Chop row with step buttons
            ui.horizontal(|ui| {
                let (lr, lresp) = ui.allocate_exact_size(egui::vec2(label_w, row_h), egui::Sense::click());
                ui.painter().rect_filled(lr, 3.0, egui::Color32::from_rgb(17, 17, 25));
                ui.painter().rect_stroke(lr, 3.0, egui::Stroke::new(0.5, egui::Color32::from_gray(30)));
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(lr.min+egui::vec2(14.0,8.0), egui::vec2(3.0, row_h-16.0)),
                    1.0, chop_color,
                );
                ui.painter().text(egui::pos2(lr.min.x+22.0, lr.center().y-4.0), egui::Align2::LEFT_CENTER,
                    format!("Chop {}", chop_idx + 1), egui::FontId::proportional(10.0), chop_color);
                ui.painter().text(egui::pos2(lr.min.x+22.0, lr.center().y+5.0), egui::Align2::LEFT_CENTER,
                    format!("{:.2}s", time_at), egui::FontId::proportional(8.0), egui::Color32::from_gray(85));
                if lresp.clicked() {
                    *self.waveform_focus.write() = WaveformFocus::DrumTrack(drum_idx);
                }
                ui.add_space(8.0);
                
                // Use seq_grid for main track, chop_steps for others
                let is_ons: [bool; NUM_STEPS] = {
                    let tracks = self.drum_tracks.read();
                    if Some(drum_idx) == main_idx {
                        let grid = self.seq_grid.read();
                        std::array::from_fn(|s| grid[s].contains(&chop_idx))
                    } else {
                        tracks.get(drum_idx)
                            .and_then(|t| t.chop_steps.get(chop_idx))
                            .copied()
                            .unwrap_or([false; NUM_STEPS])
                    }
                };
                draw_step_buttons(
                    ui, step_w, row_h,
                    chop_color, chop_color_dim,
                    &is_ons, current_step, seq_playing,
                    &mut |step| {
                        let mut tracks = self.drum_tracks.write();
                        if let Some(t) = tracks.get_mut(drum_idx) {
                            if Some(drum_idx) == main_idx {
                                let mut grid = self.seq_grid.write();
                                let sp = &mut grid[step];
                                if let Some(i) = sp.iter().position(|&p| p == chop_idx) {
                                    sp.remove(i);
                                } else {
                                    sp.push(chop_idx);
                                }
                            } else if let Some(row) = t.chop_steps.get_mut(chop_idx) {
                                row[step] = !row[step];
                            }
                        }
                    },
                );
            });
            
            // ADSR Knob row for EACH chop
            ui.horizontal(|ui| {
                let (label_space, _) = ui.allocate_exact_size(egui::vec2(label_w, knob_h), egui::Sense::hover());
                ui.painter().rect_filled(label_space, 0.0, egui::Color32::from_rgb(12, 12, 18));
                ui.add_space(8.0);
                let (knob_rect, _) = ui.allocate_exact_size(egui::vec2(steps_total, knob_h), egui::Sense::hover());
                ui.painter().rect_filled(knob_rect, 2.0, egui::Color32::from_rgb(16, 16, 24));
                ui.painter().rect_stroke(knob_rect, 2.0, egui::Stroke::new(0.5, egui::Color32::from_gray(30)));
                let adsr_now = {
                    let tracks = self.drum_tracks.read();
                    tracks.get(drum_idx)
                        .map(|t| t.chop_adsr.get(chop_idx).copied().unwrap_or(t.adsr))
                        .unwrap_or_default()
                };
                let base_id = egui::Id::new("drum_knob_chop").with(drum_idx).with(chop_idx);
                let painter = ui.painter().clone();
                let (new_adsr, adsr_changed) = draw_adsr_knobs(ui, &painter, knob_rect, adsr_now, color, base_id);
                if adsr_changed {
                    let mut tracks = self.drum_tracks.write();
                    if let Some(t) = tracks.get_mut(drum_idx) {
                        if let Some(adsr) = t.chop_adsr.get_mut(chop_idx) {
                            *adsr = new_adsr;
                        }
                        if chop_idx == 0 {
                            t.adsr = new_adsr;
                        }
                    }
                }
            });
        }
    }
    
    ui.add_space(2.0);
}
            if n_drums == 0 {
                ui.label(egui::RichText::new(
                    "No tracks yet — click ＋ Add Track to load a sample")
                    .small().color(egui::Color32::from_gray(80)).italics());
            }
            ui.add_space(3.0);
            ui.label(egui::RichText::new(
                "Click steps to toggle  ·  Click label to focus/preview  ·  Right-click for options  ·  Drag knobs to shape ADSR")
                .small().color(egui::Color32::from_gray(58)));
        });
    }

    pub fn draw_piano_roll(&mut self, ctx: &egui::Context) {
        if !*self.piano_roll_open.read() { return; }
        let focus = self.waveform_focus.read().clone();
        let WaveformFocus::DrumTrack(idx) = focus else { return; };
        let tracks = self.drum_tracks.read();
        let Some(track) = tracks.get(idx) else { return; };
        let asset = &track.asset;
        let main_idx = *self.main_track_index.read();
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
                egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
                    let (outer_rect, _) = ui.allocate_exact_size(egui::vec2(pad_label_w + grid_w + 8.0, grid_h + 4.0), egui::Sense::hover());
                    let painter = ui.painter_at(outer_rect);
                    painter.rect_filled(outer_rect, 0.0, egui::Color32::from_rgb(13, 13, 19));
                    let grid_origin = egui::pos2(outer_rect.min.x + pad_label_w, outer_rect.min.y + header_h);
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
                            let is_on = if Some(idx) == main_idx {
                                self.seq_grid.read()[step].contains(&pad_idx)
                            } else {
                                track.chop_steps.get(pad_idx).map(|s| s[step]).unwrap_or(false)
                            };
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
                    let grid_rect = egui::Rect::from_min_size(grid_origin, egui::vec2(grid_w, n_rows as f32 * cell_h));
                    let gresp = ui.interact(grid_rect, egui::Id::new("pr_grid"), egui::Sense::click_and_drag());
                    if gresp.clicked() || gresp.dragged() {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            if grid_rect.contains(pos) {
                                let step = (((pos.x - grid_origin.x) / cell_w) as usize).min(NUM_STEPS - 1);
                                let row  = (((pos.y - grid_origin.y) / cell_h) as usize).min(n_rows - 1);
                                if Some(idx) == main_idx {
                                    let mut grid = self.seq_grid.write();
                                    let sp = &mut grid[step];
                                    if let Some(i) = sp.iter().position(|&p| p == row) {
                                        if gresp.clicked() { sp.remove(i); }
                                    } else { sp.push(row); }
                                } else {
                                    let mut tracks = self.drum_tracks.write();
                                    if let Some(t) = tracks.get_mut(idx) {
                                        if let Some(row) = t.chop_steps.get_mut(row) {
                                            row[step] = !row[step];
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            });
        if !window_open { *self.piano_roll_open.write() = false; }
    }
}