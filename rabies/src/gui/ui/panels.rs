// src/gui/ui/panels.rs
use eframe::egui;
use std::sync::atomic::Ordering;
use crate::gui::{AppState, WaveformFocus, NUM_STEPS};
use super::widgets::*;
use crate::adsr::ADSREnvelope;
use crate::recording::RecordState;

impl AppState {
    pub fn seq_header_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.draw_pattern_tabs(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let pl_open = self.playlist_view_open.load(std::sync::atomic::Ordering::Relaxed);
                let (pl_lbl, pl_col) = if pl_open {
                    ("🎛 Playlist ▲", egui::Color32::from_rgb(237, 164, 80))
                } else {
                    ("🎛 Playlist ▼", egui::Color32::from_gray(130))
                };
                if ui.add(egui::Button::new(egui::RichText::new(pl_lbl).size(20.0).color(pl_col))
                    .fill(if pl_open {
                        egui::Color32::from_rgba_unmultiplied(180, 120, 30, 35)
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                ).on_hover_text("Toggle FL Playlist – drag blocks to freely arrange patterns").clicked() {
                    self.playlist_view_open.store(!pl_open, std::sync::atomic::Ordering::Relaxed);
                }

                let open      = self.song_editor_open.load(std::sync::atomic::Ordering::Relaxed);
                let (lbl, col) = if open {
                    ("📋 Song ▲", egui::Color32::from_rgb(100, 149, 237))
                } else {
                    ("📋 Song ▼", egui::Color32::from_gray(130))
                };
                if ui.add(egui::Button::new(egui::RichText::new(lbl).size(20.0).color(col))
                    .fill(if open {
                        egui::Color32::from_rgba_unmultiplied(80, 120, 210, 35)
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                ).on_hover_text("Toggle Song Editor – arrange patterns on a timeline").clicked() {
                    self.song_editor_open.store(!open, std::sync::atomic::Ordering::Relaxed);
                }
            });
        });

        ui.add(egui::Separator::default().horizontal().spacing(3.0));

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("STEP SEQUENCER").size(20.0).strong().color(egui::Color32::from_gray(100)));
            ui.separator();

            let mut bpm = self.seq_bpm.load(std::sync::atomic::Ordering::Relaxed);
            ui.label(egui::RichText::new("BPM").size(20.0).color(egui::Color32::from_gray(120)));
            if ui.add(egui::DragValue::new(&mut bpm).speed(0.5).clamp_range(40.0..=300.0).fixed_decimals(0)).changed() {
                self.seq_bpm.store(bpm, std::sync::atomic::Ordering::Relaxed);
            }
            ui.separator();

            let playing = self.seq_playing.load(std::sync::atomic::Ordering::Relaxed);
            let (lbl, col) = if playing {
                ("⏹ Stop", egui::Color32::from_rgb(220, 80, 60))
            } else {
                ("▶ Play", egui::Color32::from_rgb(60, 200, 100))
            };
            if ui.add(egui::Button::new(egui::RichText::new(lbl).color(col).size(20.0))).clicked() {
                if playing { self.stop_sequencer(); } else { self.start_sequencer(); }
            }

            if ui.add(egui::Button::new(
                egui::RichText::new("🗑 Clear").size(20.0).color(egui::Color32::from_gray(120))
            )).clicked() {
                let mut g = self.seq_grid.write();
                for s in g.iter_mut() { s.clear(); }
                let mut tracks = self.drum_tracks.write();
                for t in tracks.iter_mut() {
                    t.steps = [false; crate::gui::NUM_STEPS];
                    for row in t.chop_steps.iter_mut() { *row = [false; crate::gui::NUM_STEPS]; }
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(
                    egui::RichText::new("＋ Add Track").size(20.0).color(egui::Color32::from_rgb(80,220,140))
                )).clicked() {
                    self.load_drum_track();
                }
                if ui.add(egui::Button::new(
                    egui::RichText::new("🎙 Rec Track").size(20.0).color(egui::Color32::from_rgb(220, 80, 80))
                )).on_hover_text("Add a recording track").clicked() {
                    self.add_rec_track();
                }
                if ui.add(egui::Button::new(
                    egui::RichText::new("🎹 Piano Roll").size(20.0).color(egui::Color32::from_rgb(140,180,255))
                )).clicked() {
                    *self.piano_roll_open.write() = true;
                }
            });
        });
    }


    pub fn draw_step_sequencer(&mut self, ui: &mut egui::Ui) {
        let label_w     = 130.0;
        let step_w      = 38.0;
        let steps_total = step_w * NUM_STEPS as f32;
        let row_h       = 36.0;
        let knob_h      = 52.0;

        let frame = egui::Frame::none()
            .fill(egui::Color32::from_rgb(15, 15, 21))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(38)));

        frame.show(ui, |ui| {
            self.seq_header_ui(ui);
            ui.add(egui::Separator::default().horizontal().spacing(4.0));

            let current_step = *self.seq_current_step.read();
            let seq_playing  = self.seq_playing.load(Ordering::Relaxed);

            // ── Deferred mutation targets – set inside the scroll area,
            //    applied after it closes to avoid mid-loop structural changes.
            let mut track_to_remove: Option<usize> = None;
            let mut chop_to_remove:  Option<(usize, usize)> = None;

            egui::ScrollArea::vertical()
                .id_source("seq_body_scroll")
                .auto_shrink([false, true])
                .max_height(500.0)
                .show(ui, |ui| {

                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(label_w + 8.0);
                    for step in 0..NUM_STEPS {
                        let sz = egui::vec2(step_w - 2.0, 13.0);
                        let (r, _) = ui.allocate_exact_size(sz, egui::Sense::hover());
                        if step % 4 == 0 {
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER,
                                format!("{}", step / 4 + 1), egui::FontId::proportional(9.0),
                                egui::Color32::from_gray(75));
                        }
                        let tc = if step % 4 == 0 { egui::Color32::from_gray(65) } else { egui::Color32::from_gray(38) };
                        ui.painter().vline(r.left(), r.y_range(), egui::Stroke::new(0.5, tc));
                    }
                });

                let n_drums  = self.drum_tracks.read().len();
                let main_idx = *self.main_track_index.read();
                if n_drums > 0 {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("  Tracks").size(20.0).color(egui::Color32::from_gray(70)));
                }

                for drum_idx in 0..n_drums {
                    let color     = drum_color(drum_idx);
                    let color_dim = drum_color_dim(drum_idx);

                    let (file_name, time_str, muted, sample_uuid) = {
                        let tracks = self.drum_tracks.read();
                        let t = &tracks[drum_idx];
                        (
                            t.asset.file_name.clone(),
                            format!("{:.2}s", t.asset.frames as f32 / t.asset.sample_rate as f32),
                            t.muted,
                            t.sample_uuid,
                        )
                    };
                    let is_focused = matches!(self.waveform_focus.read().clone(),
                        WaveformFocus::DrumTrack(i) if i == drum_idx);

                    let chop_marks = self.samples_manager.get_marks_for_sample(&sample_uuid);
                    let has_chops  = !chop_marks.is_empty();

                    {
                        let mut tracks = self.drum_tracks.write();
                        if let Some(t) = tracks.get_mut(drum_idx) {
                            t.ensure_chop_steps(chop_marks.len());
                        }
                    }

                    // ── Main track step row ──────────────────────────────────
                    {
                        let steps = {
                            let tracks = self.drum_tracks.read();
                            tracks.get(drum_idx).map(|t| t.steps).unwrap_or([false; NUM_STEPS])
                        };

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

                            // ── ✕ Remove track (+ all its chops) ────────────
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new("X").size(13.0)
                                        .color(egui::Color32::from_rgb(200, 70, 70))
                                )
                                .min_size(egui::vec2(26.0, row_h))
                                .fill(egui::Color32::from_rgb(38, 12, 12))
                            )
                            .on_hover_text("Remove track and all its chops")
                            .clicked()
                            {
                                track_to_remove = Some(drum_idx);
                            }
                        });

                        ui.horizontal(|ui| {
                            let (label_space, _) = ui.allocate_exact_size(egui::vec2(label_w, knob_h), egui::Sense::hover());
                            ui.painter().rect_filled(label_space, 0.0, egui::Color32::from_rgb(12, 12, 18));
                            ui.add_space(8.0);
                            let mut tracks = self.drum_tracks.write();
                            if let Some(t) = tracks.get_mut(drum_idx) {
                                if ui.checkbox(&mut t.adsr_enabled, "ADSR").changed() {
                                    *self.status.write() = if t.adsr_enabled {
                                        format!("ADSR ON for {}", file_name)
                                    } else {
                                        format!("ADSR OFF for {} (full volume)", file_name)
                                    };
                                }
                            }
                            drop(tracks);
                            let (knob_rect, _) = ui.allocate_exact_size(egui::vec2(steps_total, knob_h), egui::Sense::hover());
                            ui.painter().rect_filled(knob_rect, 2.0, egui::Color32::from_rgb(16, 16, 24));
                            ui.painter().rect_stroke(knob_rect, 2.0, egui::Stroke::new(0.5, egui::Color32::from_gray(30)));
                            let adsr_now = self.drum_tracks.read().get(drum_idx).map(|t| t.adsr).unwrap_or_default();
                            let base_id  = egui::Id::new("drum_knob").with(drum_idx);
                            let painter  = ui.painter().clone();
                            let (new_adsr, adsr_changed) = draw_adsr_knobs(ui, &painter, knob_rect, adsr_now, color, base_id);
                            if adsr_changed {
                                if let Some(t) = self.drum_tracks.write().get_mut(drum_idx) { t.adsr = new_adsr; }
                            }
                        });
                    }

                    // ── Chop rows ────────────────────────────────────────────
                    if has_chops {
                        for (chop_idx, mark) in chop_marks.iter().enumerate() {
                            let chop_color     = pad_color(chop_idx);
                            let chop_color_dim = pad_color_dim(chop_idx);
                            let dur_asset = {
                                let tracks = self.drum_tracks.read();
                                tracks.get(drum_idx).map(|t| t.asset.frames as f32 / t.asset.sample_rate as f32).unwrap_or(0.0)
                            };
                            let time_at = mark.position * dur_asset;

                            ui.horizontal(|ui| {
                                let (lr, lresp) = ui.allocate_exact_size(egui::vec2(label_w, row_h), egui::Sense::click());
                                ui.painter().rect_filled(lr, 3.0, egui::Color32::from_rgb(17, 17, 25));
                                ui.painter().rect_stroke(lr, 3.0, egui::Stroke::new(0.5, egui::Color32::from_gray(30)));
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(lr.min+egui::vec2(14.0,8.0), egui::vec2(3.0, row_h-16.0)),
                                    1.0, chop_color,
                                );
                                let has_piano_notes = {
                                    let tracks = self.drum_tracks.read();
                                    tracks.get(drum_idx)
                                        .and_then(|t| t.chop_piano_notes.get(chop_idx))
                                        .map(|n| !n.is_empty())
                                        .unwrap_or(false)
                                };
                                ui.painter().text(egui::pos2(lr.min.x+22.0, lr.center().y-4.0), egui::Align2::LEFT_CENTER,
                                    format!("Chop {}{}", chop_idx + 1, if has_piano_notes { " 🎹" } else { "" }),
                                    egui::FontId::proportional(10.0), chop_color);
                                ui.painter().text(egui::pos2(lr.min.x+22.0, lr.center().y+5.0), egui::Align2::LEFT_CENTER,
                                    format!("{:.2}s", time_at), egui::FontId::proportional(8.0), egui::Color32::from_gray(85));
                                if lresp.clicked() {
                                    *self.waveform_focus.write() = WaveformFocus::DrumTrack(drum_idx);
                                }
                                let pr_ref = self.piano_roll_chop.clone();
                                lresp.context_menu(|ui| {
                                    ui.set_min_width(175.0);
                                    ui.label(egui::RichText::new(format!("Chop {}  @{:.2}s", chop_idx + 1, time_at)).size(20.0).color(chop_color));
                                    ui.separator();
                                    if ui.button("🎹  Piano Roll").clicked() {
                                        *pr_ref.write() = Some((drum_idx, chop_idx));
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    if ui.button(egui::RichText::new("🗑  Clear Steps").color(egui::Color32::from_rgb(200,80,80))).clicked() {
                                        let mut tracks = self.drum_tracks.write();
                                        if let Some(t) = tracks.get_mut(drum_idx) {
                                            if let Some(row) = t.chop_steps.get_mut(chop_idx) { *row = [false; NUM_STEPS]; }
                                            if let Some(notes) = t.chop_piano_notes.get_mut(chop_idx) { notes.clear(); }
                                        }
                                        ui.close_menu();
                                    }
                                });
                                ui.add_space(8.0);
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
                                    ui, step_w, row_h, chop_color, chop_color_dim,
                                    &is_ons, current_step, seq_playing,
                                    &mut |step| {
                                        let mut tracks = self.drum_tracks.write();
                                        if let Some(t) = tracks.get_mut(drum_idx) {
                                            if Some(drum_idx) == main_idx {
                                                let mut grid = self.seq_grid.write();
                                                let sp = &mut grid[step];
                                                if let Some(i) = sp.iter().position(|&p| p == chop_idx) { sp.remove(i); }
                                                else { sp.push(chop_idx); }
                                            } else if let Some(row) = t.chop_steps.get_mut(chop_idx) {
                                                row[step] = !row[step];
                                            }
                                        }
                                    },
                                );

                                // ── ✕ Remove this chop ───────────────────────
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("X").size(11.0)
                                            .color(egui::Color32::from_rgb(180, 60, 60))
                                    )
                                    .min_size(egui::vec2(22.0, row_h))
                                    .fill(egui::Color32::from_rgb(32, 10, 10))
                                )
                                .on_hover_text("Remove this chop marker")
                                .clicked()
                                {
                                    chop_to_remove = Some((drum_idx, chop_idx));
                                }
                            });

                            // Per-chop ADSR row
                            ui.horizontal(|ui| {
                                let (label_space, _) = ui.allocate_exact_size(egui::vec2(label_w, knob_h), egui::Sense::hover());
                                ui.painter().rect_filled(label_space, 0.0, egui::Color32::from_rgb(12, 12, 18));
                                ui.add_space(8.0);
                                let mut tracks = self.drum_tracks.write();
                                if let Some(t) = tracks.get_mut(drum_idx) {
                                    if let Some(enabled) = t.chop_adsr_enabled.get_mut(chop_idx) {
                                        if ui.checkbox(enabled, "ADSR").changed() {
                                            *self.status.write() = if *enabled {
                                                format!("ADSR ON for Chop {}", chop_idx + 1)
                                            } else {
                                                format!("ADSR OFF for Chop {} (full volume)", chop_idx + 1)
                                            };
                                        }
                                    }
                                }
                                drop(tracks);

                                let play_mode = {
                                    let tracks = self.drum_tracks.read();
                                    tracks.get(drum_idx)
                                        .and_then(|t| t.chop_play_modes.get(chop_idx).copied())
                                        .unwrap_or(crate::gui::ChopPlayMode::ToNextChop)
                                };
                                let fixed_modes = [
                                    (crate::gui::ChopPlayMode::ToEnd,      "▶∞",  "Play to end of sample"),
                                    (crate::gui::ChopPlayMode::ToNextChop, "▶|",  "Play to next chop marker"),
                                    (crate::gui::ChopPlayMode::ToNextStep, "▶□",  "Play for one step then stop"),
                                ];
                                for (mode, label, tip) in fixed_modes {
                                    let active = play_mode == mode;
                                    let col = if active { chop_color } else { egui::Color32::from_gray(80) };
                                    let btn = egui::Button::new(egui::RichText::new(label).size(20.0).color(col))
                                        .fill(if active {
                                            egui::Color32::from_rgba_unmultiplied(chop_color.r(), chop_color.g(), chop_color.b(), 35)
                                        } else { egui::Color32::TRANSPARENT });
                                    if ui.add(btn).on_hover_text(tip).clicked() && !active {
                                        let mut tracks = self.drum_tracks.write();
                                        if let Some(t) = tracks.get_mut(drum_idx) {
                                            if let Some(m) = t.chop_play_modes.get_mut(chop_idx) { *m = mode; }
                                        }
                                    }
                                }

                                {
                                    let all_marks = self.samples_manager.get_marks_for_sample(&sample_uuid);
                                    let is_to_marker = matches!(play_mode, crate::gui::ChopPlayMode::ToMarker(_));
                                    let current_target_id: Option<usize> = if let crate::gui::ChopPlayMode::ToMarker(id) = play_mode { Some(id) } else { None };
                                    let col = if is_to_marker { chop_color } else { egui::Color32::from_gray(80) };
                                    let btn = egui::Button::new(egui::RichText::new("▶M").size(20.0).color(col))
                                        .fill(if is_to_marker {
                                            egui::Color32::from_rgba_unmultiplied(chop_color.r(), chop_color.g(), chop_color.b(), 35)
                                        } else { egui::Color32::TRANSPARENT });
                                    if ui.add(btn).on_hover_text("Play to a specific marker you choose").clicked() && !is_to_marker {
                                        let own_pos = all_marks.get(chop_idx).map(|m| m.position).unwrap_or(0.0);
                                        let first_other = all_marks.iter().find(|m| m.position > own_pos).map(|m| m.id)
                                            .or_else(|| all_marks.first().map(|m| m.id));
                                        if let Some(target_id) = first_other {
                                            let mut tracks = self.drum_tracks.write();
                                            if let Some(t) = tracks.get_mut(drum_idx) {
                                                if let Some(m) = t.chop_play_modes.get_mut(chop_idx) {
                                                    *m = crate::gui::ChopPlayMode::ToMarker(target_id);
                                                }
                                            }
                                        }
                                    }
                                    if is_to_marker && !all_marks.is_empty() {
                                        let dur_secs = {
                                            let tracks = self.drum_tracks.read();
                                            tracks.get(drum_idx).map(|t| t.asset.frames as f32 / t.asset.sample_rate as f32).unwrap_or(0.0)
                                        };
                                        let selected_label = current_target_id
                                            .and_then(|id| all_marks.iter().find(|m| m.id == id))
                                            .map(|m| format!("M{} {:.2}s", m.id, m.position * dur_secs))
                                            .unwrap_or_else(|| "Pick marker".to_string());
                                        let combo_id = egui::Id::new("to_marker_combo").with(drum_idx).with(chop_idx);
                                        egui::ComboBox::from_id_source(combo_id)
                                            .selected_text(egui::RichText::new(&selected_label).size(20.0).color(chop_color))
                                            .width(90.0)
                                            .show_ui(ui, |ui| {
                                                for mark in &all_marks {
                                                    let label = format!("M{} @ {:.2}s", mark.id, mark.position * dur_secs);
                                                    let is_selected = current_target_id == Some(mark.id);
                                                    if ui.selectable_label(is_selected, &label).clicked() {
                                                        let mut tracks = self.drum_tracks.write();
                                                        if let Some(t) = tracks.get_mut(drum_idx) {
                                                            if let Some(m) = t.chop_play_modes.get_mut(chop_idx) {
                                                                *m = crate::gui::ChopPlayMode::ToMarker(mark.id);
                                                            }
                                                        }
                                                    }
                                                }
                                            });
                                    }
                                }

                                let (knob_rect, _) = ui.allocate_exact_size(egui::vec2(steps_total, knob_h), egui::Sense::hover());
                                ui.painter().rect_filled(knob_rect, 2.0, egui::Color32::from_rgb(16, 16, 24));
                                ui.painter().rect_stroke(knob_rect, 2.0, egui::Stroke::new(0.5, egui::Color32::from_gray(30)));
                                let adsr_now = {
                                    let tracks = self.drum_tracks.read();
                                    tracks.get(drum_idx).map(|t| t.chop_adsr.get(chop_idx).copied().unwrap_or(t.adsr)).unwrap_or_default()
                                };
                                let base_id = egui::Id::new("drum_knob_chop").with(drum_idx).with(chop_idx);
                                let painter = ui.painter().clone();
                                let (new_adsr, adsr_changed) = draw_adsr_knobs(ui, &painter, knob_rect, adsr_now, color, base_id);
                                if adsr_changed {
                                    let mut tracks = self.drum_tracks.write();
                                    if let Some(t) = tracks.get_mut(drum_idx) {
                                        if let Some(adsr) = t.chop_adsr.get_mut(chop_idx) { *adsr = new_adsr; }
                                        if chop_idx == 0 { t.adsr = new_adsr; }
                                    }
                                }
                            });
                        }
                    }

                    ui.add_space(2.0);
                } // for drum_idx

                self.draw_recording_tracks(ui, current_step, seq_playing, step_w, row_h, label_w);

                if n_drums == 0 && self.rec_tracks.read().is_empty() {
                    ui.label(egui::RichText::new(
                        "No tracks yet — click ＋ Add Track to load a sample")
                        .size(20.0).color(egui::Color32::from_gray(80)).italics());
                }
                ui.add_space(3.0);
                ui.label(egui::RichText::new(
                    "Click steps to toggle  ·  Click label to focus/preview  ·  Right-click for options  ·  X to remove")
                    .size(20.0).color(egui::Color32::from_gray(58)));

                });

            // ── Apply deferred track removal ──────────────────────────────────
            if let Some(rm_idx) = track_to_remove {
                let uuid = self.drum_tracks.read().get(rm_idx).map(|t| t.sample_uuid);
                if let Some(uuid) = uuid {
                    self.samples_manager.clear_marks_for_uuid(&uuid);
                }
                self.drum_tracks.write().remove(rm_idx);
                let n = self.drum_tracks.read().len();
                if n == 0 {
                    *self.waveform_focus.write()    = WaveformFocus::MainSample;
                    *self.main_track_index.write()  = None;
                    *self.waveform_analysis.write() = None;
                } else {
                    let new_idx = rm_idx.min(n - 1);
                    *self.waveform_focus.write() = WaveformFocus::DrumTrack(new_idx);
                    let cur_main = *self.main_track_index.read();
                    if cur_main.map_or(true, |i| i == rm_idx || i >= n) {
                        *self.main_track_index.write() = Some(new_idx);
                    }
                    if let Some(wf) = self.drum_tracks.read().get(new_idx).and_then(|t| t.waveform.clone()) {
                        *self.waveform_analysis.write() = Some(wf);
                    }
                }
                *self.status.write() = format!("Track {} removed", rm_idx + 1);
            }

            // ── Apply deferred chop removal ───────────────────────────────────
            if let Some((t_idx, c_idx)) = chop_to_remove {
                let uuid = self.drum_tracks.read().get(t_idx).map(|t| t.sample_uuid);
                if let Some(uuid) = uuid {
                    // Find and delete the mark from the global marks list
                    let marks = self.samples_manager.get_marks_for_sample(&uuid);
                    if let Some(mark) = marks.get(c_idx) {
                        let mark_id = mark.id;
                        let global_idx = self.samples_manager.get_marks()
                            .iter()
                            .position(|m| m.id == mark_id);
                        if let Some(gi) = global_idx {
                            self.samples_manager.delete_mark(gi);
                        }
                    }
                }
                // Remove corresponding per-chop arrays at that index
                let mut tracks = self.drum_tracks.write();
                if let Some(t) = tracks.get_mut(t_idx) {
                    if c_idx < t.chop_steps.len()       { t.chop_steps.remove(c_idx); }
                    if c_idx < t.chop_adsr.len()        { t.chop_adsr.remove(c_idx); }
                    if c_idx < t.chop_adsr_enabled.len(){ t.chop_adsr_enabled.remove(c_idx); }
                    if c_idx < t.chop_play_modes.len()  { t.chop_play_modes.remove(c_idx); }
                    if c_idx < t.chop_piano_notes.len() { t.chop_piano_notes.remove(c_idx); }
                }
                *self.status.write() = format!("Chop {} removed", c_idx + 1);
            }
        });
    }

    pub fn draw_recording_tracks(
        &mut self,
        ui:           &mut egui::Ui,
        current_step: usize,
        seq_playing:  bool,
        step_w:       f32,
        row_h:        f32,
        label_w:      f32,
    ) {
        let n_rec = self.rec_tracks.read().len();
        if n_rec == 0 { return; }

        let ctrl_w   = 120.0_f32;
        let knob_h   = 30.0_f32;
        let rec_base = egui::Color32::from_rgb(220, 70, 60);
        let rec_dim  = egui::Color32::from_rgb(44, 14, 12);

        ui.add_space(4.0);
        ui.label(egui::RichText::new("  🎙 Recording Tracks").size(20.0).color(egui::Color32::from_gray(70)));

        let active_rec_track = *self.rec_active_track.read();

        for rec_idx in 0..n_rec {
            let (state, short_name, has_asset, steps, muted, dur_str, take_num) = {
                let tracks = self.rec_tracks.read();
                let t = &tracks[rec_idx];
                let dur = t.duration_secs().map(|s| format!("{:.2}s", s)).unwrap_or_else(|| "–".to_string());
                (t.state.clone(), t.short_name(), t.asset.is_some(), t.steps, t.muted, dur, t.take_number)
            };

            let is_active = active_rec_track == Some(rec_idx);
            let peak      = if is_active { self.rec_manager.peak() } else { 0.0 };
            let rec_secs  = if is_active { self.rec_manager.recorded_secs() } else { 0.0 };

            ui.horizontal(|ui| {
                let (lr, lresp) = ui.allocate_exact_size(egui::vec2(label_w, row_h), egui::Sense::click());
                let border_col = if is_active { egui::Color32::from_rgb(255, 60, 60) }
                                 else if has_asset { egui::Color32::from_rgb(160, 55, 45) }
                                 else { egui::Color32::from_gray(42) };
                ui.painter().rect_filled(lr, 3.0, egui::Color32::from_rgb(22, 13, 13));
                ui.painter().rect_stroke(lr, 3.0, egui::Stroke::new(if is_active { 2.0 } else { 1.0 }, border_col));
                let bar_col = if is_active { egui::Color32::from_rgb(255, 60, 60) }
                              else if has_asset { rec_base } else { egui::Color32::from_gray(50) };
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(lr.min + egui::vec2(5.0, 7.0), egui::vec2(4.0, lr.height() - 14.0)),
                    2.0, bar_col,
                );
                if is_active {
                    ui.painter().circle_filled(egui::pos2(lr.max.x - 9.0, lr.min.y + 9.0), 4.5, egui::Color32::from_rgb(255, 50, 50));
                }
                ui.painter().text(
                    egui::pos2(lr.min.x + 14.0, lr.center().y - 5.0), egui::Align2::LEFT_CENTER,
                    &short_name, egui::FontId::proportional(10.5),
                    if muted { egui::Color32::from_gray(70) } else { rec_base },
                );
                let sub = match &state {
                    RecordState::Idle      => "idle".to_string(),
                    RecordState::Recording => format!("● {:.1}s", rec_secs),
                    RecordState::Recorded  => dur_str.clone(),
                };
                let sub_col = match state {
                    RecordState::Recording => egui::Color32::from_rgb(255, 90, 90),
                    RecordState::Recorded  => egui::Color32::from_rgb(90, 200, 100),
                    _                      => egui::Color32::from_gray(65),
                };
                ui.painter().text(
                    egui::pos2(lr.min.x + 14.0, lr.center().y + 6.0), egui::Align2::LEFT_CENTER,
                    sub, egui::FontId::proportional(8.5), sub_col,
                );
                if is_active && peak > 0.0 {
                    let mr = egui::Rect::from_min_size(egui::pos2(lr.min.x + 5.0, lr.max.y - 5.0), egui::vec2(lr.width() - 10.0, 3.0));
                    ui.painter().rect_filled(mr, 1.0, egui::Color32::from_gray(20));
                    let fill = (peak * mr.width()).min(mr.width());
                    let mc = if peak > 0.85 { egui::Color32::from_rgb(255, 50, 40) }
                             else if peak > 0.5 { egui::Color32::from_rgb(255, 200, 40) }
                             else { egui::Color32::from_rgb(50, 220, 80) };
                    ui.painter().rect_filled(egui::Rect::from_min_size(mr.min, egui::vec2(fill, 3.0)), 1.0, mc);
                }
                let rct = self.rec_tracks.clone();
                lresp.context_menu(|ui| {
                    ui.set_min_width(160.0);
                    ui.label(egui::RichText::new(&short_name).size(20.0).color(rec_base));
                    ui.separator();
                    if ui.button(if muted { "🔊 Unmute" } else { "🔇 Mute" }).clicked() {
                        if let Some(t) = rct.write().get_mut(rec_idx) { t.muted = !t.muted; }
                        ui.close_menu();
                    }
                    if ui.button("🗑 Clear Steps").clicked() {
                        if let Some(t) = rct.write().get_mut(rec_idx) { t.steps = [false; NUM_STEPS]; }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(egui::RichText::new("✕ Remove Track").color(egui::Color32::from_rgb(220,80,60))).clicked() {
                        rct.write().remove(rec_idx);
                        ui.close_menu();
                    }
                });
                ui.add_space(8.0);
                if has_asset {
                    draw_step_buttons(ui, step_w, row_h, rec_base, rec_dim, &steps, current_step, seq_playing,
                        &mut |step| {
                            if let Some(t) = self.rec_tracks.write().get_mut(rec_idx) { t.steps[step] = !t.steps[step]; }
                        },
                    );
                } else {
                    let total_w = step_w * NUM_STEPS as f32;
                    let (ph, _) = ui.allocate_exact_size(egui::vec2(total_w, row_h), egui::Sense::hover());
                    ui.painter().rect_filled(ph, 2.0, egui::Color32::from_rgb(17, 11, 11));
                    ui.painter().text(ph.center(), egui::Align2::CENTER_CENTER,
                        if is_active { "Recording…  stop to enable steps" }
                        else         { "Select a device and press 🔴 Rec to record" },
                        egui::FontId::proportional(9.0), egui::Color32::from_gray(52));
                }
            });

            ui.horizontal(|ui| {
                let (ls, _) = ui.allocate_exact_size(egui::vec2(label_w, knob_h), egui::Sense::hover());
                ui.painter().rect_filled(ls, 0.0, egui::Color32::from_rgb(14, 9, 9));
                ui.add_space(8.0);

                let devices = self.input_devices.read().clone();
                let current_label = {
                    let tracks = self.rec_tracks.read();
                    tracks.get(rec_idx).and_then(|t| t.device_label.clone()).unwrap_or_else(|| "Pick input…".to_string())
                };
                let short_lbl = {
                    let s = current_label.split(": ").nth(1).unwrap_or(&current_label);
                    if s.len() > 16 { format!("{}…", &s[..14]) } else { s.to_string() }
                };
                egui::ComboBox::from_id_source(egui::Id::new("rec_dev").with(rec_idx))
                    .selected_text(egui::RichText::new(&short_lbl).size(20.0))
                    .width(ctrl_w)
                    .show_ui(ui, |ui| {
                        let mut last_host = String::new();
                        for dev in &devices {
                            if dev.host_name != last_host {
                                ui.separator();
                                ui.label(egui::RichText::new(&dev.host_name).size(20.0).color(egui::Color32::from_gray(130)));
                                last_host = dev.host_name.clone();
                            }
                            let sel = current_label == dev.label;
                            let short = if dev.device_name.len() > 30 { format!("{}…", &dev.device_name[..28]) } else { dev.device_name.clone() };
                            if ui.selectable_label(sel, egui::RichText::new(&short).size(20.0)).clicked() {
                                if let Some(t) = self.rec_tracks.write().get_mut(rec_idx) { t.device_label = Some(dev.label.clone()); }
                            }
                        }
                        ui.separator();
                        if ui.button(egui::RichText::new("↻ Refresh devices").size(20.0)).clicked() { self.refresh_input_devices(); }
                    });

                ui.add_space(6.0);

                if is_active {
                    if ui.add(egui::Button::new(egui::RichText::new("⏹ Stop").size(20.0).color(egui::Color32::from_rgb(255, 120, 80)))).clicked() {
                        self.stop_recording(rec_idx);
                    }
                } else {
                    let can_rec = { let tracks = self.rec_tracks.read(); tracks.get(rec_idx).and_then(|t| t.device_label.as_ref()).is_some() };
                    let already_busy = self.rec_manager.is_recording();
                    if ui.add_enabled(can_rec && !already_busy, egui::Button::new(
                        egui::RichText::new("🔴 Rec").size(20.0).color(
                            if can_rec && !already_busy { egui::Color32::from_rgb(255, 60, 60) } else { egui::Color32::from_gray(65) }
                        )
                    )).on_hover_text(if already_busy { "Stop the current recording first" }
                       else if !can_rec { "Select an input device first" }
                       else { "Record from selected input" }).clicked() {
                        self.start_recording(rec_idx);
                    }
                }

                if has_asset {
                    ui.add_space(4.0);
                    if ui.add(egui::Button::new(egui::RichText::new("→ Drum Track").size(20.0).color(egui::Color32::from_rgb(80, 200, 130))))
                        .on_hover_text("Promote this recording to a full drum track").clicked() {
                        self.promote_rec_to_drum(rec_idx);
                        return;
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(format!("take {} · {}", take_num - 1, dur_str)).size(20.0).color(egui::Color32::from_gray(75)));
                }
            });

            ui.add_space(2.0);
        }
    }

    pub fn draw_piano_roll(&mut self, ctx: &egui::Context) {
        if !*self.piano_roll_open.read() { return; }
        let focus = self.waveform_focus.read().clone();
        let WaveformFocus::DrumTrack(idx) = focus else { return; };
        let (file_name, dur, sample_uuid) = {
            let tracks = self.drum_tracks.read();
            let Some(track) = tracks.get(idx) else { return; };
            (
                track.asset.file_name.clone(),
                track.asset.frames as f32 / track.asset.sample_rate as f32,
                track.sample_uuid,
            )
        };
        let main_idx = *self.main_track_index.read();
        let marks = self.samples_manager.get_marks_for_sample(&sample_uuid);

        let mut window_open = true;
        egui::Window::new(format!("🎹 Piano Roll — {}", file_name))
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
                        ui.label(egui::RichText::new("Click cell to toggle  ·  Rows = chops").size(20.0).color(egui::Color32::from_gray(95)));
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

                            let tracks = self.drum_tracks.read();
                            let track = tracks.get(idx);
                            let is_on = if Some(idx) == main_idx {
                                self.seq_grid.read()[step].contains(&pad_idx)
                            } else {
                                track.and_then(|t| t.chop_steps.get(pad_idx)).map(|s| s[step]).unwrap_or(false)
                            };
                            drop(tracks);

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