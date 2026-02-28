// src/gui/ui/pattern_playlist.rs
//! Song Editor + FL Studio–faithful Playlist panels.
//!
//! draw_song_editor  – classic paint/erase block grid (original)
//! draw_fl_playlist  – FL Studio Playlist: left sidebar of patterns,
//!                     right track-row grid, draw/select/erase tools
//! draw_pattern_tabs – compact tab bar inside the step-sequencer header

use eframe::egui;
use std::sync::atomic::Ordering;
use crate::gui::AppState;

// ─── Layout constants ────────────────────────────────────────────────────────
const ROW_H:      f32 = 32.0;
const LABEL_W:    f32 = 140.0;
const BAR_W:      f32 = 28.0;
const HEADER_H:   f32 = 20.0;

// FL Playlist-specific
const FL_SIDEBAR_W:  f32 = 160.0;  // left pattern-list panel
const FL_TRACK_H:    f32 = 34.0;   // height of each track row
const FL_BAR_W:      f32 = 26.0;   // width of one bar cell
const FL_HEADER_H:   f32 = 22.0;   // bar-number header height
const FL_NUM_TRACKS: usize = 16;   // default visible tracks
const FL_TRACK_LBL:  f32 = 80.0;   // "Track N" label column inside grid

// ─── Colours matching FL Studio's dark theme ─────────────────────────────────
fn fl_bg()              -> egui::Color32 { egui::Color32::from_rgb(40, 44, 52) }
fn fl_sidebar_bg()      -> egui::Color32 { egui::Color32::from_rgb(32, 35, 42) }
fn fl_toolbar_bg()      -> egui::Color32 { egui::Color32::from_rgb(28, 31, 38) }
fn fl_header_bg()       -> egui::Color32 { egui::Color32::from_rgb(34, 37, 45) }
fn fl_track_even()      -> egui::Color32 { egui::Color32::from_rgb(44, 48, 58) }
fn fl_track_odd()       -> egui::Color32 { egui::Color32::from_rgb(40, 44, 53) }
fn fl_grid_major()      -> egui::Color32 { egui::Color32::from_rgb(28, 31, 38) }
fn fl_grid_minor()      -> egui::Color32 { egui::Color32::from_rgb(36, 39, 47) }
fn fl_border()          -> egui::Color32 { egui::Color32::from_rgb(22, 24, 30) }
fn fl_text()            -> egui::Color32 { egui::Color32::from_rgb(200, 202, 210) }
fn fl_text_dim()        -> egui::Color32 { egui::Color32::from_rgb(110, 115, 130) }
fn fl_selected_pat_bg() -> egui::Color32 { egui::Color32::from_rgb(60, 90, 130) }
fn fl_playhead()        -> egui::Color32 { egui::Color32::from_rgb(255, 220, 60) }
fn fl_accent()          -> egui::Color32 { egui::Color32::from_rgb(100, 180, 255) }

impl AppState {
    // =========================================================================
    //  Song Editor  (original paint/erase view — unchanged)
    // =========================================================================
    pub fn draw_song_editor(&mut self, ui: &mut egui::Ui) {
        let open = self.song_editor_open.load(Ordering::Relaxed);
        if !open { return; }

        let frame = egui::Frame::none()
            .fill(egui::Color32::from_rgb(12, 14, 22))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 90, 160)));

        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("SONG EDITOR")
                    .small().strong().color(egui::Color32::from_rgb(100, 149, 237)));
                ui.separator();

                let song_playing = self.song_editor.is_playing();
                let (lbl, col) = if song_playing {
                    ("⏹ Stop Song", egui::Color32::from_rgb(220, 80, 60))
                } else {
                    ("▶ Play Song", egui::Color32::from_rgb(60, 200, 100))
                };
                if ui.add(egui::Button::new(egui::RichText::new(lbl).small().color(col))).clicked() {
                    if song_playing { self.stop_song(); } else { self.start_song(); }
                }

                ui.separator();
                let bar   = self.song_editor.current_bar.load(Ordering::Relaxed);
                let total = *self.song_editor.total_bars.read();
                ui.label(egui::RichText::new(format!("Bar {}/{}", bar + 1, total))
                    .small().color(egui::Color32::from_gray(140)));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(
                        egui::RichText::new("🗑 Clear").small().color(egui::Color32::from_rgb(180,60,60))
                    )).clicked() { self.song_editor.clear_arrangement(); }
                    ui.add_space(6.0);
                    if ui.add(egui::Button::new(
                        egui::RichText::new("＋ Bar").small().color(egui::Color32::from_gray(140))
                    )).clicked() {
                        let cur = *self.song_editor.total_bars.read();
                        self.song_editor.ensure_bar_count(cur + 8);
                    }
                    ui.add_space(6.0);
                    if ui.add(egui::Button::new(
                        egui::RichText::new("＋ Pattern").small().color(egui::Color32::from_rgb(80, 220, 140))
                    )).clicked() { self.create_new_pattern(); }
                });
            });

            ui.add_space(4.0);

            let n_patterns = self.song_editor.pattern_count();
            let total_bars = *self.song_editor.total_bars.read();
            let grid_w     = BAR_W * total_bars as f32;
            let content_h  = HEADER_H + ROW_H * n_patterns as f32 + 4.0;

            egui::ScrollArea::both()
                .id_source("song_editor_scroll")
                .auto_shrink([false, true])
                .max_height(260.0)
                .show(ui, |ui| {
                    let (outer, _) = ui.allocate_exact_size(
                        egui::vec2(LABEL_W + grid_w + 12.0, content_h),
                        egui::Sense::hover(),
                    );
                    let p          = ui.painter_at(outer);
                    let grid_orig  = egui::pos2(outer.min.x + LABEL_W, outer.min.y + HEADER_H);

                    p.rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(outer.min.x + LABEL_W, outer.min.y),
                            egui::vec2(grid_w, HEADER_H),
                        ),
                        0.0, egui::Color32::from_rgb(16, 18, 28),
                    );
                    for bar in 0..total_bars {
                        let x = grid_orig.x + bar as f32 * BAR_W;
                        if bar % 4 == 0 {
                            p.text(
                                egui::pos2(x + 2.0, outer.min.y + HEADER_H * 0.5),
                                egui::Align2::LEFT_CENTER,
                                format!("{}", bar + 1),
                                egui::FontId::proportional(9.0),
                                egui::Color32::from_gray(130),
                            );
                        }
                        p.vline(x, egui::Rangef::new(outer.min.y, outer.min.y + HEADER_H),
                            egui::Stroke::new(if bar % 4 == 0 { 0.8 } else { 0.3 }, egui::Color32::from_gray(45)));
                    }

                    if self.song_editor.is_playing() {
                        let cur_bar = self.song_editor.current_bar.load(Ordering::Relaxed);
                        let px = grid_orig.x + cur_bar as f32 * BAR_W;
                        p.rect_filled(
                            egui::Rect::from_min_size(egui::pos2(px, outer.min.y), egui::vec2(BAR_W, content_h)),
                            0.0, egui::Color32::from_rgba_unmultiplied(255, 220, 80, 20),
                        );
                        p.vline(px, egui::Rangef::new(outer.min.y, outer.min.y + content_h),
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 220, 80)));
                    }

                    let active_idx = self.song_editor.active_edit_idx();
                    let arr = self.song_editor.get_arrangement_snapshot();

                    for row_i in 0..n_patterns {
                        let y       = grid_orig.y + row_i as f32 * ROW_H;
                        let pattern = match self.song_editor.get_pattern_by_idx(row_i) {
                            Some(p) => p, None => continue,
                        };
                        let color     = pattern.egui_color();
                        let color_dim = pattern.egui_color_dim();
                        let is_active = row_i == active_idx;

                        let label_rect = egui::Rect::from_min_size(
                            egui::pos2(outer.min.x, y),
                            egui::vec2(LABEL_W - 4.0, ROW_H - 1.0),
                        );
                        let label_bg = if is_active {
                            egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30)
                        } else { egui::Color32::from_rgb(18, 18, 26) };
                        p.rect_filled(label_rect, 3.0, label_bg);
                        p.rect_stroke(label_rect, 3.0, egui::Stroke::new(
                            if is_active { 1.5 } else { 0.5 },
                            if is_active { color } else { egui::Color32::from_gray(38) },
                        ));
                        p.rect_filled(
                            egui::Rect::from_min_size(label_rect.min + egui::vec2(4.0, 6.0), egui::vec2(4.0, ROW_H - 12.0)),
                            2.0, color,
                        );
                        let name = if pattern.name.len() > 14 { format!("{}…", &pattern.name[..12]) } else { pattern.name.clone() };
                        p.text(egui::pos2(label_rect.min.x + 13.0, label_rect.center().y - 4.0),
                            egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(11.0),
                            if is_active { color } else { egui::Color32::from_gray(160) });
                        let tc = pattern.tracks.len();
                        p.text(egui::pos2(label_rect.min.x + 13.0, label_rect.center().y + 6.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{} track{}", tc, if tc == 1 { "" } else { "s" }),
                            egui::FontId::proportional(8.5), egui::Color32::from_gray(90));

                        let label_resp = ui.interact(label_rect, egui::Id::new("se_label").with(row_i), egui::Sense::click());
                        if label_resp.clicked() && !is_active { self.switch_pattern(row_i); }
                        label_resp.context_menu(|ui| {
                            ui.set_min_width(160.0);
                            ui.label(egui::RichText::new(&pattern.name).small().color(color));
                            ui.separator();
                            if ui.button("✏ Edit (switch here)").clicked() { self.switch_pattern(row_i); ui.close_menu(); }
                            if ui.button("⎘ Duplicate").clicked() { self.song_editor.duplicate_pattern(row_i); ui.close_menu(); }
                            ui.separator();
                            if ui.button(egui::RichText::new("✕ Remove").color(egui::Color32::from_rgb(200,80,80))).clicked() {
                                self.song_editor.remove_pattern(row_i); ui.close_menu();
                            }
                        });

                        let row_rect = egui::Rect::from_min_size(egui::pos2(grid_orig.x, y), egui::vec2(grid_w, ROW_H - 1.0));
                        p.rect_filled(row_rect, 0.0,
                            if row_i % 2 == 0 { egui::Color32::from_rgb(17, 17, 25) } else { egui::Color32::from_rgb(14, 14, 22) });

                        let row_arr = arr.get(row_i);
                        for bar in 0..total_bars {
                            let x    = grid_orig.x + bar as f32 * BAR_W;
                            let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(BAR_W - 1.0, ROW_H - 1.0));
                            let occupied = row_arr.and_then(|r| r.get(bar)).copied().unwrap_or(false);
                            if occupied {
                                p.rect_filled(cell.shrink(2.0), 3.0, color);
                                p.hline(
                                    egui::Rangef::new(cell.shrink(2.0).left() + 2.0, cell.shrink(2.0).right() - 2.0),
                                    cell.shrink(2.0).top() + 1.5,
                                    egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(255,255,255,70)),
                                );
                            } else { p.rect_filled(cell.shrink(3.0), 2.0, color_dim); }
                            let lc = if bar % 4 == 0 { egui::Color32::from_gray(50) } else { egui::Color32::from_gray(28) };
                            p.vline(x, egui::Rangef::new(y, y + ROW_H - 1.0), egui::Stroke::new(0.5, lc));
                        }
                        p.hline(
                            egui::Rangef::new(outer.min.x, outer.min.x + LABEL_W + grid_w),
                            y + ROW_H - 0.5,
                            egui::Stroke::new(0.5, egui::Color32::from_gray(28)),
                        );
                    }

                    let grid_rect = egui::Rect::from_min_size(grid_orig, egui::vec2(grid_w, ROW_H * n_patterns as f32));
                    let gresp = ui.interact(grid_rect, egui::Id::new("se_grid"), egui::Sense::click_and_drag());
                    let primary   = ui.input(|i| i.pointer.primary_down());
                    let secondary = ui.input(|i| i.pointer.secondary_down());
                    if (primary || secondary) && (gresp.dragged() || gresp.clicked() || gresp.drag_started()) {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            if grid_rect.contains(pos) {
                                let bar   = ((pos.x - grid_orig.x) / BAR_W) as usize;
                                let row_i = ((pos.y - grid_orig.y) / ROW_H) as usize;
                                let bar   = bar.min(total_bars.saturating_sub(1));
                                let row_i = row_i.min(n_patterns.saturating_sub(1));
                                if primary   { self.song_editor.set_block(row_i, bar, true); }
                                if secondary { self.song_editor.set_block(row_i, bar, false); }
                            }
                        }
                    }
                });

            ui.add_space(2.0);
            ui.label(egui::RichText::new(
                "Left-drag = paint  ·  Right-drag = erase  ·  Click pattern name to edit  ·  Right-click pattern for options"
            ).small().color(egui::Color32::from_gray(55)));
        });
    }

    // =========================================================================
    //  FL Studio–faithful Playlist
    //
    //  Key fix: everything (sidebar + grid) is drawn inside ONE ScrollArea
    //  using a single allocated rect + painter. The sidebar occupies the
    //  left FL_SIDEBAR_W pixels; the track grid occupies the rest.
    //  This avoids nested layout containers fighting each other.
    // =========================================================================
    pub fn draw_fl_playlist(&mut self, ui: &mut egui::Ui) {
        let open = self.playlist_view_open.load(Ordering::Relaxed);
        if !open { return; }

        let n_patterns = self.song_editor.pattern_count();
        let total_bars = {
            self.song_editor.ensure_bar_count(32);
            *self.song_editor.total_bars.read()
        };
        let n_tracks = FL_NUM_TRACKS.max(n_patterns);

        // Ensure arrangement rows exist
        for row in 0..n_tracks {
            let v = self.song_editor.get_block(row, 0);
            self.song_editor.set_block(row, 0, v);
        }

        let active_edit  = self.song_editor.active_edit_idx();
        let song_playing = self.song_editor.is_playing();

        let outer_frame = egui::Frame::none()
            .fill(fl_toolbar_bg())
            .rounding(egui::Rounding::same(4.0))
            .stroke(egui::Stroke::new(1.5, fl_border()));

        outer_frame.show(ui, |ui| {

            // ── TOOLBAR ──────────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let (play_lbl, play_col) = if song_playing {
                    ("⏹", egui::Color32::from_rgb(220, 80, 60))
                } else {
                    ("▶", egui::Color32::from_rgb(80, 220, 100))
                };
                if ui.add(
                    egui::Button::new(egui::RichText::new(play_lbl).size(14.0).color(play_col))
                        .min_size(egui::vec2(26.0, 22.0))
                        .fill(egui::Color32::from_rgb(38, 42, 52))
                ).clicked() {
                    if song_playing { self.stop_song(); } else { self.start_song(); }
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Playlist — Arrangement").size(11.0).color(fl_text_dim()));
                ui.separator();
                if ui.add(egui::Button::new(
                    egui::RichText::new("＋ Pattern").size(10.5).color(egui::Color32::from_rgb(100, 220, 140)))
                    .fill(egui::Color32::from_rgb(30, 45, 35))
                ).clicked() { self.create_new_pattern(); }
                ui.add_space(4.0);
                if ui.add(egui::Button::new(
                    egui::RichText::new("＋ Bar").size(10.5).color(fl_text_dim()))
                    .fill(egui::Color32::from_rgb(38, 42, 52))
                ).clicked() {
                    let cur = *self.song_editor.total_bars.read();
                    self.song_editor.ensure_bar_count(cur + 8);
                }
                ui.add_space(4.0);
                if ui.add(egui::Button::new(
                    egui::RichText::new("🗑 Clear").size(10.5).color(egui::Color32::from_rgb(200, 80, 80)))
                    .fill(egui::Color32::from_rgb(45, 32, 32))
                ).clicked() {
                    self.song_editor.clear_arrangement();
                    *self.pl_drag_src.write() = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    let bar = self.song_editor.current_bar.load(Ordering::Relaxed);
                    ui.label(egui::RichText::new(format!("Bar {}", bar + 1)).size(10.5).color(fl_text_dim()));
                    if self.pl_drag_src.read().is_some() {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("✊ Drag to move · Right-click to cancel")
                            .size(10.0).color(egui::Color32::from_rgb(237, 164, 80)));
                    }
                });
            });

            ui.add(egui::Separator::default().horizontal().spacing(0.0));

            // ── BODY: single ScrollArea, painter draws sidebar + grid ────
            //
            //  Content layout (x axis):
            //    [0 .. FL_SIDEBAR_W)           = pattern sidebar
            //    [FL_SIDEBAR_W .. +FL_TRACK_LBL) = "Track N" label column
            //    [FL_SIDEBAR_W+FL_TRACK_LBL ..) = bar grid
            //
            //  Content layout (y axis):
            //    [0 .. FL_HEADER_H)            = header row (bar numbers / column labels)
            //    [FL_HEADER_H .. )              = track rows

            let grid_total_w = FL_BAR_W * total_bars as f32;
            let content_w    = FL_SIDEBAR_W + FL_TRACK_LBL + grid_total_w + 8.0;
            let content_h    = FL_HEADER_H + FL_TRACK_H * n_tracks as f32 + 4.0;
            let max_h        = content_h.min(400.0).max(200.0);

            egui::ScrollArea::both()
                .id_source("fl_pl_body_v4")
                .auto_shrink([false, false])
                .max_height(max_h)
                .show(ui, |ui| {

                let (outer, _) = ui.allocate_exact_size(
                    egui::vec2(content_w, content_h),
                    egui::Sense::hover(),
                );
                let p = ui.painter_at(outer);

                // ── Sidebar ───────────────────────────────────────────────
                let sidebar_rect = egui::Rect::from_min_size(
                    outer.min,
                    egui::vec2(FL_SIDEBAR_W, content_h),
                );
                p.rect_filled(sidebar_rect, 0.0, fl_sidebar_bg());

                // Sidebar header
                let sb_hdr = egui::Rect::from_min_size(outer.min, egui::vec2(FL_SIDEBAR_W, FL_HEADER_H));
                p.rect_filled(sb_hdr, 0.0, fl_toolbar_bg());
                p.hline(sb_hdr.x_range(), sb_hdr.bottom(), egui::Stroke::new(0.7, fl_border()));
                for (i, lbl) in ["NOTE", "CHAN", "PAT"].iter().enumerate() {
                    p.text(
                        egui::pos2(sb_hdr.left() + 8.0 + i as f32 * 48.0, sb_hdr.center().y),
                        egui::Align2::LEFT_CENTER, lbl,
                        egui::FontId::monospace(8.0), fl_text_dim(),
                    );
                }

                // Sidebar right border (vertical divider)
                p.vline(
                    sidebar_rect.right(),
                    egui::Rangef::new(outer.top(), outer.bottom()),
                    egui::Stroke::new(1.0, fl_border()),
                );

                // ── Pattern rows in sidebar (vertical list) ───────────────
                let item_h = 24.0_f32;
                for row_i in 0..n_patterns {
                    let pattern = match self.song_editor.get_pattern_by_idx(row_i) {
                        Some(pat) => pat, None => continue,
                    };
                    let color     = pattern.egui_color();
                    let is_active = row_i == active_edit;

                    // Each item sits below the sidebar header, stacked vertically
                    let y = outer.top() + FL_HEADER_H + row_i as f32 * item_h;
                    let item_rect = egui::Rect::from_min_size(
                        egui::pos2(outer.left(), y),
                        egui::vec2(FL_SIDEBAR_W - 1.0, item_h - 1.0),
                    );

                    let item_bg = if is_active { fl_selected_pat_bg() } else { fl_sidebar_bg() };
                    p.rect_filled(item_rect, 0.0, item_bg);

                    // Active indicator ▸
                    if is_active {
                        p.text(
                            egui::pos2(item_rect.left() + 4.0, item_rect.center().y),
                            egui::Align2::LEFT_CENTER, "▸",
                            egui::FontId::proportional(10.0), color,
                        );
                    }

                    // Pattern name
                    let name = if pattern.name.len() > 14 {
                        format!("{}…", &pattern.name[..12])
                    } else { pattern.name.clone() };
                    p.text(
                        egui::pos2(item_rect.left() + 16.0, item_rect.center().y),
                        egui::Align2::LEFT_CENTER, name,
                        egui::FontId::proportional(10.5),
                        if is_active { egui::Color32::WHITE } else { fl_text() },
                    );

                    // Coloured dot on the right
                    let dot_x = item_rect.right() - 11.0;
                    p.circle_filled(egui::pos2(dot_x, item_rect.center().y), 4.5, color);
                    p.circle_stroke(egui::pos2(dot_x, item_rect.center().y), 4.5,
                        egui::Stroke::new(0.8, egui::Color32::from_rgba_unmultiplied(255,255,255,50)));

                    // Bottom divider
                    p.hline(item_rect.x_range(), item_rect.bottom(),
                        egui::Stroke::new(0.5, fl_border()));

                    // Interaction (hover highlight + click)
                    let resp = ui.interact(item_rect, egui::Id::new("fl_sb4").with(row_i), egui::Sense::click());
                    if resp.hovered() && !is_active {
                        p.rect_filled(item_rect, 0.0,
                            egui::Color32::from_rgba_unmultiplied(255,255,255,8));
                    }
                    if resp.clicked() { self.song_editor.set_active_edit_idx(row_i); }
                    if resp.double_clicked() { self.switch_pattern(row_i); }
                    resp.context_menu(|ui| {
                        ui.set_min_width(160.0);
                        ui.label(egui::RichText::new(&pattern.name).small().color(color));
                        ui.separator();
                        if ui.button("✏ Edit pattern").clicked() { self.switch_pattern(row_i); ui.close_menu(); }
                        if ui.button("🖌 Select as brush").clicked() { self.song_editor.set_active_edit_idx(row_i); ui.close_menu(); }
                        if ui.button("⎘ Duplicate").clicked() { self.song_editor.duplicate_pattern(row_i); ui.close_menu(); }
                        ui.separator();
                        if ui.button(egui::RichText::new("✕ Remove").color(egui::Color32::from_rgb(200,80,80))).clicked() {
                            self.song_editor.remove_pattern(row_i);
                            *self.pl_drag_src.write() = None;
                            ui.close_menu();
                        }
                    });
                }

                // ── Grid area ──────────────────────────────────────────────
                // X-offset of the track-label column inside the grid region
                let grid_x0   = outer.left() + FL_SIDEBAR_W;         // start of track-label col
                let grid_orig = egui::pos2(grid_x0 + FL_TRACK_LBL, outer.top() + FL_HEADER_H);

                // Header background (track label + bar numbers)
                let hdr_rect = egui::Rect::from_min_size(
                    egui::pos2(grid_x0, outer.top()),
                    egui::vec2(FL_TRACK_LBL + grid_total_w, FL_HEADER_H),
                );
                p.rect_filled(hdr_rect, 0.0, fl_header_bg());
                // "Track" column label
                p.text(
                    egui::pos2(grid_x0 + FL_TRACK_LBL * 0.5, outer.top() + FL_HEADER_H * 0.5),
                    egui::Align2::CENTER_CENTER, "Track",
                    egui::FontId::monospace(8.5), fl_text_dim(),
                );
                // Bar numbers
                for bar in 0..total_bars {
                    let x = grid_orig.x + bar as f32 * FL_BAR_W;
                    if bar % 2 == 0 {
                        p.text(
                            egui::pos2(x + 3.0, outer.top() + FL_HEADER_H * 0.5),
                            egui::Align2::LEFT_CENTER,
                            format!("{}", bar + 1),
                            egui::FontId::proportional(9.0),
                            egui::Color32::from_gray(150),
                        );
                    }
                    let lc = if bar % 4 == 0 { egui::Color32::from_gray(55) } else { egui::Color32::from_gray(35) };
                    p.vline(x, egui::Rangef::new(outer.top(), outer.top() + FL_HEADER_H),
                        egui::Stroke::new(if bar % 4 == 0 { 0.8 } else { 0.3 }, lc));
                }
                p.hline(hdr_rect.x_range(), hdr_rect.bottom(), egui::Stroke::new(1.0, fl_border()));

                // ── Playhead ───────────────────────────────────────────────
                if song_playing {
                    let cur_bar = self.song_editor.current_bar.load(Ordering::Relaxed);
                    let px = grid_orig.x + cur_bar as f32 * FL_BAR_W;
                    p.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(px, outer.top()), egui::vec2(FL_BAR_W, content_h)),
                        0.0, egui::Color32::from_rgba_unmultiplied(255, 220, 60, 15),
                    );
                    p.vline(px, egui::Rangef::new(outer.top(), outer.top() + content_h),
                        egui::Stroke::new(2.0, fl_playhead()));
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(px, outer.top() + FL_HEADER_H),
                            egui::pos2(px - 5.0, outer.top() + FL_HEADER_H - 7.0),
                            egui::pos2(px + 5.0, outer.top() + FL_HEADER_H - 7.0),
                        ],
                        fl_playhead(), egui::Stroke::NONE,
                    ));
                }

                // ── Track rows ─────────────────────────────────────────────
                let arr      = self.song_editor.get_arrangement_snapshot();
                let drag_src = *self.pl_drag_src.read();

                let grid_area = egui::Rect::from_min_size(
                    grid_orig,
                    egui::vec2(grid_total_w, FL_TRACK_H * n_tracks as f32),
                );
                let pointer_pos = ui.input(|i| i.pointer.interact_pos());
                let hover_cell: Option<(usize, usize)> = pointer_pos.and_then(|pos| {
                    if grid_area.contains(pos) {
                        let bar   = ((pos.x - grid_orig.x) / FL_BAR_W) as usize;
                        let track = ((pos.y - grid_orig.y) / FL_TRACK_H) as usize;
                        Some((track.min(n_tracks.saturating_sub(1)), bar.min(total_bars.saturating_sub(1))))
                    } else { None }
                });

                for track_i in 0..n_tracks {
                    let y = grid_orig.y + track_i as f32 * FL_TRACK_H;
                    let track_bg = if track_i % 2 == 0 { fl_track_even() } else { fl_track_odd() };

                    // Track label cell
                    let label_cell = egui::Rect::from_min_size(
                        egui::pos2(grid_x0, y),
                        egui::vec2(FL_TRACK_LBL - 1.0, FL_TRACK_H - 1.0),
                    );
                    p.rect_filled(label_cell, 0.0, egui::Color32::from_rgb(
                        track_bg.r().saturating_sub(8),
                        track_bg.g().saturating_sub(8),
                        track_bg.b().saturating_sub(8),
                    ));
                    p.circle_filled(
                        egui::pos2(label_cell.left() + 10.0, label_cell.center().y),
                        3.5, egui::Color32::from_rgb(60, 200, 80),
                    );
                    p.text(
                        egui::pos2(label_cell.left() + 22.0, label_cell.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("Track {}", track_i + 1),
                        egui::FontId::proportional(10.5),
                        fl_text_dim(),
                    );
                    p.vline(label_cell.right(),
                        egui::Rangef::new(y, y + FL_TRACK_H - 1.0),
                        egui::Stroke::new(1.0, fl_border()));

                    // Row background
                    p.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(grid_orig.x, y), egui::vec2(grid_total_w, FL_TRACK_H - 1.0)),
                        0.0, track_bg,
                    );

                    // Bar cells
                    let row_arr = arr.get(track_i);
                    for bar in 0..total_bars {
                        let x    = grid_orig.x + bar as f32 * FL_BAR_W;
                        let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(FL_BAR_W - 1.0, FL_TRACK_H - 1.0));
                        let occupied    = row_arr.and_then(|r| r.get(bar)).copied().unwrap_or(false);
                        let is_drag_src = drag_src == Some((track_i, bar));
                        let is_hover    = hover_cell == Some((track_i, bar));
                        let is_ghost    = is_hover && drag_src.is_some();

                        let pat_idx = active_edit.min(n_patterns.saturating_sub(1));
                        let cell_color = if n_patterns > 0 {
                            self.song_editor.get_pattern_by_idx(pat_idx)
                                .map(|pat| pat.egui_color()).unwrap_or(fl_accent())
                        } else { fl_accent() };

                        if is_drag_src {
                            p.rect_stroke(cell.shrink(2.0), 2.0,
                                egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(
                                    cell_color.r(), cell_color.g(), cell_color.b(), 100)));
                        } else if is_ghost {
                            p.rect_filled(cell.shrink(1.5), 3.0,
                                egui::Color32::from_rgba_unmultiplied(cell_color.r(), cell_color.g(), cell_color.b(), 130));
                            p.rect_stroke(cell.shrink(1.5), 3.0, egui::Stroke::new(2.0, cell_color));
                        } else if occupied {
                            p.rect_filled(cell.shrink(1.0), 2.0, cell_color);
                            p.hline(
                                egui::Rangef::new(cell.shrink(2.0).left(), cell.shrink(2.0).right()),
                                cell.shrink(2.0).top() + 1.0,
                                egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(255,255,255,80)),
                            );
                            p.hline(
                                egui::Rangef::new(cell.left(), cell.right()),
                                cell.bottom() - 1.0,
                                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0,0,0,80)),
                            );
                            for wi in 0..3_usize {
                                let wh = if wi == 1 { cell.height() * 0.55 } else { cell.height() * 0.3 };
                                let wx = cell.left() + 4.0 + wi as f32 * (cell.width().max(8.0) * 0.28);
                                let cy = cell.center().y;
                                p.vline(wx, egui::Rangef::new(cy - wh*0.5, cy + wh*0.5),
                                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255,255,255,55)));
                            }
                        } else if is_hover && drag_src.is_none() {
                            p.rect_filled(cell.shrink(1.0), 2.0,
                                egui::Color32::from_rgba_unmultiplied(cell_color.r(), cell_color.g(), cell_color.b(), 50));
                        }

                        // Grid lines
                        let gl = if bar % 4 == 0 { fl_grid_major() } else { fl_grid_minor() };
                        p.vline(x, egui::Rangef::new(y, y + FL_TRACK_H - 1.0),
                            egui::Stroke::new(if bar % 4 == 0 { 0.8 } else { 0.4 }, gl));
                    }

                    // Row divider
                    p.hline(
                        egui::Rangef::new(grid_x0, grid_x0 + FL_TRACK_LBL + grid_total_w),
                        y + FL_TRACK_H - 0.5,
                        egui::Stroke::new(0.5, fl_border()),
                    );
                }

                // ── Grid interactions ──────────────────────────────────────
                let gresp = ui.interact(
                    grid_area, egui::Id::new("fl_pl_grid_v4"), egui::Sense::click_and_drag(),
                );

                let primary_pressed  = ui.input(|i| i.pointer.primary_pressed());
                let primary_down     = ui.input(|i| i.pointer.primary_down());
                let primary_released = ui.input(|i| i.pointer.primary_released());
                let secondary_down   = ui.input(|i| i.pointer.secondary_down());

                if primary_pressed {
                    if let Some((ti, bar)) = hover_cell {
                        let occupied = arr.get(ti).and_then(|r| r.get(bar)).copied().unwrap_or(false);
                        if occupied {
                            self.song_editor.set_block(ti, bar, false);
                            *self.pl_drag_src.write() = Some((ti, bar));
                        } else {
                            self.song_editor.set_block(ti, bar, true);
                        }
                    }
                }
                if primary_down && drag_src.is_none() && gresp.dragged() {
                    if let Some((ti, bar)) = hover_cell {
                        if !self.song_editor.get_block(ti, bar) {
                            self.song_editor.set_block(ti, bar, true);
                        }
                    }
                }
                if primary_released {
                    let held = *self.pl_drag_src.read();
                    if let Some((src_ti, src_bar)) = held {
                        let target = hover_cell.unwrap_or((src_ti, src_bar));
                        self.song_editor.set_block(target.0, target.1, true);
                        *self.pl_drag_src.write() = None;
                    }
                }
                if secondary_down && (gresp.dragged() || gresp.secondary_clicked() || gresp.clicked()) {
                    if let Some((ti, bar)) = hover_cell {
                        self.song_editor.set_block(ti, bar, false);
                    }
                    let held = *self.pl_drag_src.read();
                    if let Some((src_ti, src_bar)) = held {
                        self.song_editor.set_block(src_ti, src_bar, true);
                        *self.pl_drag_src.write() = None;
                    }
                }

                // Cursor icon
                let drag_now = *self.pl_drag_src.read();
                if drag_now.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if let Some((ti, bar)) = hover_cell {
                    if self.song_editor.get_block(ti, bar) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    } else {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                }
            }); // end ScrollArea

            // ── Status bar ────────────────────────────────────────────────
            ui.add(egui::Separator::default().horizontal().spacing(0.0));
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(
                    "Left-click empty = draw  ·  Drag block = move  ·  Right-click = erase  ·  Single-click = select brush  ·  Double-click = edit"
                ).size(9.5).color(fl_text_dim()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = self.song_editor.pattern_count();
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("{} pattern{}", n, if n == 1 {""} else {"s"}))
                        .size(9.5).color(fl_text_dim()));
                });
            });
        }); // end outer_frame
    }

    // =========================================================================
    //  Pattern-tab bar  (inside step-sequencer header — unchanged)
    // =========================================================================
    pub fn draw_pattern_tabs(&mut self, ui: &mut egui::Ui) {
        let n      = self.song_editor.pattern_count();
        let active = self.song_editor.active_edit_idx();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("PAT").small().color(egui::Color32::from_gray(80)));
            ui.add_space(2.0);

            for i in 0..n {
                let pattern = match self.song_editor.get_pattern_by_idx(i) {
                    Some(p) => p, None => continue,
                };
                let color     = pattern.egui_color();
                let is_active = i == active;

                let lbl  = if pattern.name.len() > 8 { format!("{}…", &pattern.name[..6]) } else { pattern.name.clone() };
                let rich  = egui::RichText::new(&lbl)
                    .small()
                    .color(if is_active { color } else { egui::Color32::from_gray(120) });
                let btn = egui::Button::new(rich)
                    .fill(if is_active {
                        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40)
                    } else { egui::Color32::from_rgb(22, 22, 30) })
                    .stroke(egui::Stroke::new(
                        if is_active { 1.5 } else { 0.5 },
                        if is_active { color } else { egui::Color32::from_gray(38) },
                    ));

                let resp = ui.add(btn);
                if resp.clicked() && !is_active { self.switch_pattern(i); }
                resp.context_menu(|ui| {
                    ui.set_min_width(130.0);
                    ui.label(egui::RichText::new(&pattern.name).small().color(color));
                    ui.separator();
                    if ui.button("⎘ Duplicate").clicked() {
                        self.song_editor.duplicate_pattern(i); ui.close_menu();
                    }
                    if n > 1 {
                        if ui.button(egui::RichText::new("✕ Remove").color(egui::Color32::from_rgb(200,80,80))).clicked() {
                            let new_active = if active >= n - 1 { n.saturating_sub(2) } else { active };
                            self.song_editor.remove_pattern(i);
                            if i != active || new_active != active {
                                self.song_editor.set_active_edit_idx(
                                    new_active.min(self.song_editor.pattern_count().saturating_sub(1))
                                );
                            }
                            ui.close_menu();
                        }
                    }
                });
            }

            if ui.add(
                egui::Button::new(egui::RichText::new("＋").small().color(egui::Color32::from_rgb(80, 220, 140)))
                    .fill(egui::Color32::from_rgb(18, 28, 20))
                    .stroke(egui::Stroke::new(0.8, egui::Color32::from_rgb(50, 130, 70)))
            ).on_hover_text("Create new pattern (fresh workspace)").clicked() {
                self.create_new_pattern();
            }
        });
    }
}