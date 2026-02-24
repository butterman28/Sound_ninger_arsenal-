use eframe::egui;
use crate::adsr::ADSREnvelope;
use crate::gui::NUM_STEPS;

const PAD_COLORS: &[(u8, u8, u8)] = &[
    (80, 160, 255), (80, 220, 140), (240, 160, 60), (200, 80, 200),
    (240, 80, 80),  (80, 220, 220), (240, 200, 60), (160, 120, 240),
    (255, 120, 160),(100, 220, 180),(200, 140, 60), (120, 160, 240),
];

pub fn pad_color(idx: usize) -> egui::Color32 {
    let (r, g, b) = PAD_COLORS[idx % PAD_COLORS.len()];
    egui::Color32::from_rgb(r, g, b)
}

pub fn pad_color_dim(idx: usize) -> egui::Color32 {
    let (r, g, b) = PAD_COLORS[idx % PAD_COLORS.len()];
    egui::Color32::from_rgb(r / 5, g / 5, b / 5)
}

pub fn drum_color(idx: usize) -> egui::Color32 { pad_color(idx + 4) }
pub fn drum_color_dim(idx: usize) -> egui::Color32 { pad_color_dim(idx + 4) }

pub fn draw_knob(
    painter: &egui::Painter,
    ui: &mut egui::Ui,
    center: egui::Pos2,
    radius: f32,
    value: &mut f32,
    color: egui::Color32,
    label: &str,
    id: egui::Id,
) -> bool {
    use std::f32::consts::PI;
    let start_angle = PI * 0.75;
    let end_angle   = PI * 2.25;
    let sweep       = end_angle - start_angle;
    let rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.2, radius * 2.2));
    let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
    let mut changed = false;

    if resp.dragged() {
        let delta = resp.drag_delta();
        let change = (-delta.y / 80.0).clamp(-1.0, 1.0);
        *value = (*value + change).clamp(0.0, 1.0);
        changed = true;
    }
    if resp.double_clicked() {
        *value = 0.5;
        changed = true;
    }

    let bg = egui::Color32::from_rgb(28, 28, 38);
    let ring = egui::Color32::from_gray(45);
    painter.circle_filled(center, radius, bg);
    painter.circle_stroke(center, radius, egui::Stroke::new(1.5, ring));

    let n_seg = 32;
    for i in 0..n_seg {
        let t0 = start_angle + (i as f32 / n_seg as f32) * sweep;
        let t1 = start_angle + ((i + 1) as f32 / n_seg as f32) * sweep;
        let p0 = center + egui::vec2(t0.cos(), t0.sin()) * (radius - 2.5);
        let p1 = center + egui::vec2(t1.cos(), t1.sin()) * (radius - 2.5);
        painter.line_segment([p0, p1], egui::Stroke::new(2.0, egui::Color32::from_gray(55)));
    }

    let fill_segs = ((*value * n_seg as f32) as usize).min(n_seg);
    for i in 0..fill_segs {
        let t0 = start_angle + (i as f32 / n_seg as f32) * sweep;
        let t1 = start_angle + ((i + 1) as f32 / n_seg as f32) * sweep;
        let p0 = center + egui::vec2(t0.cos(), t0.sin()) * (radius - 2.5);
        let p1 = center + egui::vec2(t1.cos(), t1.sin()) * (radius - 2.5);
        painter.line_segment([p0, p1], egui::Stroke::new(2.5, color));
    }

    let angle = start_angle + *value * sweep;
    let inner = center + egui::vec2(angle.cos(), angle.sin()) * (radius * 0.35);
    let outer = center + egui::vec2(angle.cos(), angle.sin()) * (radius * 0.82);
    painter.line_segment([inner, outer], egui::Stroke::new(2.0, egui::Color32::WHITE));
    painter.text(
        egui::pos2(center.x, center.y + radius + 4.0),
        egui::Align2::CENTER_TOP,
        label,
        egui::FontId::proportional(8.5),
        egui::Color32::from_gray(120),
    );

    if resp.hovered() {
        painter.circle_stroke(center, radius, egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(255,255,255,60)));
    }
    changed
}

pub fn draw_step_buttons(
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

pub fn draw_adsr_knobs(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    knob_area: egui::Rect,
    adsr: ADSREnvelope,
    color: egui::Color32,
    base_id: egui::Id,
) -> (ADSREnvelope, bool) {
    let mut adsr = adsr;
    let mut changed = false;
    let knob_r = (knob_area.height() * 0.35).min(14.0);
    let knob_w = knob_area.width() / 4.0;
    let cy = knob_area.center().y;
    let params: [(&str, f32, f32, &mut f32); 4] = [
        ("A", 0.0, 2.0, &mut adsr.attack),
        ("D", 0.0, 2.0, &mut adsr.decay),
        ("S", 0.0, 1.0, &mut adsr.sustain),
        ("R", 0.0, 3.0, &mut adsr.release),
    ];
    for (i, (label, min, max, val)) in params.into_iter().enumerate() {
        let cx = knob_area.left() + knob_w * i as f32 + knob_w * 0.5;
        let center = egui::pos2(cx, cy);
        let norm = ((*val - min) / (max - min)).clamp(0.0, 1.0);
        let mut norm_mut = norm;
        let kid = base_id.with(i);
        if draw_knob(painter, ui, center, knob_r, &mut norm_mut, color, label, kid) {
            *val = min + norm_mut * (max - min);
            changed = true;
        }
    }
    (adsr, changed)
}