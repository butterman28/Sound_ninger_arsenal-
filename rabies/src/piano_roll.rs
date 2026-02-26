// src/piano_roll.rs
//
// Per-chop piano roll — FL-Studio style keyboard grid for each chop.
// Right-click a chop row label → "🎹 Piano Roll" to open this window.
// Notes placed here play back as pitch-shifted voices via the sequencer.
//
// Files to touch (see comments at bottom for exact lines):
//   main.rs          → `mod piano_roll;`
//   gui/mod.rs       → add fields + update tick_sequencer
//   gui/ui/view.rs   → call `self.draw_chop_piano_roll(ctx);`
//   gui/ui/panels.rs → right-click context menu on chop label

use eframe::egui;
use std::sync::atomic::Ordering;
use crate::gui::{AppState, NUM_STEPS};
use crate::gui::ui::widgets::pad_color;

// ── Public data model (also used by gui/mod.rs) ───────────────────────────────

/// A pitched note in a chop's piano roll.
/// `semitone` is relative to C4 (0 = C4, 12 = C5, −12 = C3 …).
/// Playback speed = 2^(semitone / 12).
#[derive(Clone, Debug)]
pub struct PianoRollNote {
    pub step:     usize,  // 0 .. NUM_STEPS-1
    pub semitone: i32,    // relative to C4
    pub velocity: f32,    // 0.0–1.0  (note opacity)
}

impl PianoRollNote {
    pub fn speed(&self) -> f32 {
        2f32.powf(self.semitone as f32 / 12.0)
    }
}

// ── Music helpers ─────────────────────────────────────────────────────────────

pub fn is_black_key(semitone: i32) -> bool {
    let pos = ((semitone % 12) + 12) % 12;
    matches!(pos, 1 | 3 | 6 | 8 | 10)
}

pub fn semitone_to_name(semitone: i32) -> String {
    let midi  = 60 + semitone;
    let names = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];
    let oct   = midi / 12 - 1;
    format!("{}{}", names[(midi.rem_euclid(12)) as usize], oct)
}

// ── Grid constants ────────────────────────────────────────────────────────────

pub const SEM_MIN: i32 = -36;  // C1  (bottom row)
pub const SEM_MAX: i32 =  37;  // C7  (exclusive; highest shown = B6 = +36)
// total rows = SEM_MAX − SEM_MIN = 73

const ROW_H:  f32 = 14.0;
const KEY_W:  f32 = 60.0;
const STEP_W: f32 = 38.0;
const HDR_H:  f32 = 22.0;

// ── AppState implementation ───────────────────────────────────────────────────

impl AppState {
    /// Call every frame from `view.rs::update()` after `draw_piano_roll`.
    pub fn draw_chop_piano_roll(&mut self, ctx: &egui::Context) {
        let open = { *self.piano_roll_chop.read() };
        let (track_idx, chop_idx) = match open { Some(v) => v, None => return };

        // Snapshot display data before entering UI (avoids holding RwLock during draw)
        let (file_name, dur_secs, chop_col) = {
            let tracks = self.drum_tracks.read();
            match tracks.get(track_idx) {
                Some(t) => (
                    t.asset.file_name.clone(),
                    t.asset.frames as f32 / t.asset.sample_rate as f32,
                    pad_color(chop_idx),
                ),
                None => { *self.piano_roll_chop.write() = None; return; }
            }
        };
        let marks    = self.samples_manager.get_marks_for_sample(&file_name);
        let mark_pos = marks.get(chop_idx).map(|m| m.position * dur_secs).unwrap_or(0.0);

        let seq_playing  = self.seq_playing.load(Ordering::Relaxed);
        let current_step = *self.seq_current_step.read();

        let total_rows = (SEM_MAX - SEM_MIN) as usize;
        let grid_w     = STEP_W * NUM_STEPS as f32;
        let grid_h     = ROW_H  * total_rows as f32;

        // Start scroll so C4 is visible near the middle
        let c4_row_y   = (SEM_MAX - 1) as f32 * ROW_H;   // y offset of C4 row
        let init_scroll = (c4_row_y - 150.0).max(0.0);

        let mut window_open = true;

        egui::Window::new(format!(
            "🎹  {}  ·  Chop {}  @{:.3}s",
            file_name, chop_idx + 1, mark_pos
        ))
        .id(egui::Id::new("chop_pr").with(track_idx).with(chop_idx))
        .default_size([820.0, 500.0])
        .min_size([540.0, 300.0])
        .resizable(true)
        .collapsible(false)
        .open(&mut window_open)
        .show(ctx, |ui| {

            // ── Toolbar ───────────────────────────────────────────────────
            ui.horizontal(|ui| {
                let (lbl, col) = if seq_playing {
                    ("⏹ Stop", egui::Color32::from_rgb(220, 80, 60))
                } else {
                    ("▶ Play", egui::Color32::from_rgb(60, 200, 100))
                };
                if ui.add(egui::Button::new(egui::RichText::new(lbl).color(col))).clicked() {
                    if seq_playing { self.stop_sequencer(); } else { self.start_sequencer(); }
                }

                let mut bpm = self.seq_bpm.load(Ordering::Relaxed);
                ui.label("BPM");
                if ui.add(
                    egui::DragValue::new(&mut bpm)
                        .speed(0.5)
                        .clamp_range(40.0..=300.0)
                        .fixed_decimals(0)
                ).changed() {
                    self.seq_bpm.store(bpm, Ordering::Relaxed);
                }

                ui.separator();

                let note_count: usize = {
                    let tracks = self.drum_tracks.read();
                    tracks.get(track_idx)
                        .and_then(|t| t.chop_piano_notes.get(chop_idx))
                        .map(|n| n.len())
                        .unwrap_or(0)
                };
                ui.label(
                    egui::RichText::new(format!(
                        "{} note{}",
                        note_count,
                        if note_count == 1 { "" } else { "s" }
                    ))
                    .small()
                    .color(chop_col),
                );

                ui.separator();

                if ui.button(
                    egui::RichText::new("🗑 Clear all")
                        .small()
                        .color(egui::Color32::from_rgb(200, 80, 80))
                ).clicked() {
                    let mut tracks = self.drum_tracks.write();
                    if let Some(t) = tracks.get_mut(track_idx) {
                        if let Some(notes) = t.chop_piano_notes.get_mut(chop_idx) {
                            notes.clear();
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Left-click = add  ·  Right-click = remove  ·  C4 = original pitch")
                            .small()
                            .color(egui::Color32::from_gray(85)),
                    );
                    if note_count == 0 {
                        ui.separator();
                        ui.label(
                            egui::RichText::new("⚠ Empty — step buttons used as fallback")
                                .small()
                                .color(egui::Color32::from_rgb(200, 175, 55)),
                        );
                    }
                });
            });

            ui.add_space(2.0);

            egui::Frame::none()
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(38)))
                .rounding(egui::Rounding::same(4.0))
                .show(ui, |ui| {

                egui::ScrollArea::both()
                    .auto_shrink([false; 2])
                    .vertical_scroll_offset(init_scroll)
                    .show(ui, |ui| {

                    let content_h = HDR_H + grid_h + 4.0;
                    let content_w = KEY_W + grid_w + 8.0;
                    let (outer, _) = ui.allocate_exact_size(
                        egui::vec2(content_w, content_h),
                        egui::Sense::hover(),
                    );
                    let p         = ui.painter_at(outer);
                    let grid_orig = egui::pos2(outer.min.x + KEY_W, outer.min.y + HDR_H);

                    // Background
                    p.rect_filled(outer, 0.0, egui::Color32::from_rgb(13, 13, 19));

                    // ── Step-number header ────────────────────────────────
                    for step in 0..NUM_STEPS {
                        let x  = grid_orig.x + step as f32 * STEP_W;
                        let hr = egui::Rect::from_min_size(
                            egui::pos2(x, outer.min.y),
                            egui::vec2(STEP_W - 1.0, HDR_H - 1.0),
                        );
                        let grp = step / 4;
                        p.rect_filled(hr, 0.0,
                            if grp % 2 == 0 { egui::Color32::from_rgb(22, 22, 33) }
                            else             { egui::Color32::from_rgb(17, 17, 26) });
                        if step % 4 == 0 {
                            p.text(hr.center(), egui::Align2::CENTER_CENTER,
                                format!("{}", step / 4 + 1),
                                egui::FontId::proportional(10.0),
                                egui::Color32::from_gray(145));
                        } else {
                            p.circle_filled(hr.center(), 1.5, egui::Color32::from_gray(55));
                        }
                        if seq_playing && current_step == step {
                            p.rect_filled(hr, 0.0,
                                egui::Color32::from_rgba_unmultiplied(255, 220, 80, 48));
                        }
                    }

                    // ── Piano key + grid rows ─────────────────────────────
                    for row_i in 0..total_rows {
                        let semitone  = SEM_MAX - 1 - row_i as i32;
                        let y         = grid_orig.y + row_i as f32 * ROW_H;
                        let black     = is_black_key(semitone);
                        let note_pos  = ((semitone % 12) + 12) % 12;
                        let is_c      = note_pos == 0;
                        let is_c4     = semitone == 0;

                        // ── Piano key ─────────────────────────────────────
                        let key_rect = egui::Rect::from_min_size(
                            egui::pos2(outer.min.x, y),
                            egui::vec2(KEY_W - 2.0, ROW_H - 0.5),
                        );

                        if black {
                            p.rect_filled(key_rect, 0.0, egui::Color32::from_rgb(28, 28, 36));
                            // Black-key dark face (shorter)
                            let face = egui::Rect::from_min_size(
                                key_rect.min + egui::vec2(2.0, 0.0),
                                egui::vec2(KEY_W * 0.6, ROW_H - 0.5),
                            );
                            p.rect_filled(face, 0.0, egui::Color32::from_rgb(16, 16, 22));
                        } else {
                            p.rect_filled(key_rect, 0.0, egui::Color32::from_rgb(205, 205, 215));
                            // Key bottom shadow
                            let bot = egui::Rect::from_min_size(
                                egui::pos2(key_rect.min.x, key_rect.max.y - 2.0),
                                egui::vec2(KEY_W - 2.0, 2.0),
                            );
                            p.rect_filled(bot, 0.0, egui::Color32::from_rgb(155, 155, 165));
                        }
                        p.rect_stroke(key_rect, 0.0, egui::Stroke::new(0.4, egui::Color32::from_gray(48)));

                        // C4 gets a special orange dot marker
                        if is_c4 {
                            p.circle_filled(
                                egui::pos2(key_rect.right() - 6.0, key_rect.center().y),
                                3.5,
                                egui::Color32::from_rgb(255, 140, 40),
                            );
                        }

                        // Note labels: C notes + every E and B (helps reading)
                        if is_c || note_pos == 4 || note_pos == 11 {
                            p.text(
                                egui::pos2(key_rect.right() - 3.0, key_rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                semitone_to_name(semitone),
                                egui::FontId::proportional(if is_c { 8.5 } else { 7.0 }),
                                if black { egui::Color32::from_gray(120) }
                                else     { egui::Color32::from_gray(68)  },
                            );
                        }

                        // ── Grid row background ───────────────────────────
                        let row_bg = if is_c4     { egui::Color32::from_rgb(30, 26, 18) } // orange-ish for C4
                                     else if is_c { egui::Color32::from_rgb(22, 24, 36) }
                                     else if black{ egui::Color32::from_rgb(14, 14, 21) }
                                     else         { egui::Color32::from_rgb(18, 18, 27) };

                        let grow = egui::Rect::from_min_size(
                            egui::pos2(grid_orig.x, y),
                            egui::vec2(grid_w, ROW_H - 0.5),
                        );
                        p.rect_filled(grow, 0.0, row_bg);

                        if is_c {
                            p.hline(
                                egui::Rangef::new(grid_orig.x, grid_orig.x + grid_w),
                                y,
                                egui::Stroke::new(0.7, egui::Color32::from_gray(46)),
                            );
                        }
                        if is_c4 {
                            // Subtle orange bottom border for C4
                            p.hline(
                                egui::Rangef::new(grid_orig.x, grid_orig.x + grid_w),
                                y + ROW_H - 1.5,
                                egui::Stroke::new(0.8, egui::Color32::from_rgba_unmultiplied(255, 140, 40, 60)),
                            );
                        }

                        // Beat vertical dividers + playhead tint per cell
                        for step in 0..NUM_STEPS {
                            let x = grid_orig.x + step as f32 * STEP_W;
                            if step % 4 == 0 {
                                p.vline(x,
                                    egui::Rangef::new(y, y + ROW_H),
                                    egui::Stroke::new(0.6, egui::Color32::from_gray(38)));
                            }
                            if seq_playing && current_step == step {
                                p.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(x, y),
                                        egui::vec2(STEP_W - 1.0, ROW_H - 0.5),
                                    ),
                                    0.0,
                                    egui::Color32::from_rgba_unmultiplied(255, 220, 80, 16),
                                );
                            }
                        }
                    }

                    // Outer grid border
                    p.rect_stroke(
                        egui::Rect::from_min_size(grid_orig, egui::vec2(grid_w, grid_h)),
                        0.0,
                        egui::Stroke::new(0.5, egui::Color32::from_gray(42)),
                    );

                    // ── Draw placed notes ─────────────────────────────────
                    let notes: Vec<PianoRollNote> = {
                        let tracks = self.drum_tracks.read();
                        tracks.get(track_idx)
                            .and_then(|t| t.chop_piano_notes.get(chop_idx))
                            .cloned()
                            .unwrap_or_default()
                    };

                    for note in &notes {
                        if note.semitone < SEM_MIN || note.semitone >= SEM_MAX { continue; }
                        let row_i = (SEM_MAX - 1 - note.semitone) as usize;
                        let y     = grid_orig.y + row_i as f32 * ROW_H;
                        let x     = grid_orig.x + note.step as f32 * STEP_W;
                        let nr    = egui::Rect::from_min_size(
                            egui::pos2(x + 2.5, y + 2.5),
                            egui::vec2(STEP_W - 5.0, ROW_H - 5.0),
                        );
                        let alpha = (note.velocity * 190.0 + 65.0) as u8;
                        p.rect_filled(nr, 2.5,
                            egui::Color32::from_rgba_unmultiplied(
                                chop_col.r(), chop_col.g(), chop_col.b(), alpha));
                        // Shine
                        p.hline(
                            egui::Rangef::new(nr.left() + 3.0, nr.right() - 3.0),
                            nr.top() + 1.5,
                            egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(255,255,255,130)),
                        );
                        p.rect_stroke(nr, 2.5,
                            egui::Stroke::new(0.8, egui::Color32::from_rgba_unmultiplied(255,255,255,55)));
                    }

                    // Full active-step column overlay
                    if seq_playing {
                        let sx = grid_orig.x + current_step as f32 * STEP_W;
                        p.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(sx, grid_orig.y),
                                egui::vec2(STEP_W - 1.0, grid_h),
                            ),
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(255, 220, 80, 10),
                        );
                    }

                    // ── Click interaction ─────────────────────────────────
                    let grid_rect = egui::Rect::from_min_size(
                        grid_orig,
                        egui::vec2(grid_w, grid_h),
                    );
                    let gresp = ui.interact(
                        grid_rect,
                        egui::Id::new("chpr").with(track_idx).with(chop_idx),
                        egui::Sense::click(),
                    );

                    if gresp.clicked() || gresp.secondary_clicked() {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            if grid_rect.contains(pos) {
                                let step = (((pos.x - grid_orig.x) / STEP_W) as usize)
                                    .min(NUM_STEPS - 1);
                                let row_i = (((pos.y - grid_orig.y) / ROW_H) as usize)
                                    .min(total_rows - 1);
                                let semitone = SEM_MAX - 1 - row_i as i32;

                                let mut tracks = self.drum_tracks.write();
                                if let Some(t) = tracks.get_mut(track_idx) {
                                    if let Some(notes) = t.chop_piano_notes.get_mut(chop_idx) {
                                        let existing = notes.iter()
                                            .position(|n| n.step == step && n.semitone == semitone);
                                        if let Some(idx) = existing {
                                            notes.remove(idx);
                                        } else if gresp.clicked() {
                                            // Left-click adds
                                            notes.push(PianoRollNote {
                                                step,
                                                semitone,
                                                velocity: 1.0,
                                            });
                                        }
                                        // Right-click already removed above; no-op if not found
                                    }
                                }
                            }
                        }
                    }

                    // ── Hover: row highlight + tooltip ────────────────────
                    if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                        if grid_rect.contains(pos) {
                            let row_i    = (((pos.y - grid_orig.y) / ROW_H) as usize)
                                .min(total_rows - 1);
                            let semitone = SEM_MAX - 1 - row_i as i32;
                            let speed    = 2f32.powf(semitone as f32 / 12.0);

                            // Row highlight
                            let hy = grid_orig.y + row_i as f32 * ROW_H;
                            p.rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(grid_orig.x, hy),
                                    egui::vec2(grid_w, ROW_H - 0.5),
                                ),
                                0.0,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 9),
                            );

                            // Tooltip
                            let tip     = format!("{}  (speed ×{:.3})", semitone_to_name(semitone), speed);
                            let tip_pos = egui::pos2(pos.x + 14.0, pos.y - 7.0);
                            let galley  = p.layout_no_wrap(
                                tip.clone(),
                                egui::FontId::proportional(10.0),
                                egui::Color32::WHITE,
                            );
                            let bubble = egui::Rect::from_min_size(
                                tip_pos - egui::vec2(3.0, 2.0),
                                galley.size() + egui::vec2(8.0, 4.0),
                            );
                            p.rect_filled(bubble, 3.0,
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 220));
                            p.text(
                                tip_pos + egui::vec2(1.0, 0.0),
                                egui::Align2::LEFT_TOP,
                                tip,
                                egui::FontId::proportional(10.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                });
            });
        });

        if !window_open {
            *self.piano_roll_chop.write() = None;
        }
    }
}