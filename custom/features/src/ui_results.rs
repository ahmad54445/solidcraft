//! Result choice in the status bar (LC ▾, Shell ▾, Frame ▾, Max/Min ▾) and the legend over the
//! 3D view (docs/10-viewport-results.md).

use custom_bridge::ui::egui;
use custom_bridge::view::{ViewInfo, ramp};

use crate::results_view::{self, Choice, FRAME_COMPONENTS, Legend, SHELL_COMPONENTS, Step};

/// The status bar's result controls. Rebuilds the view when the choice or the units change.
pub fn status_controls(ui: &mut egui::Ui) {
    let shown = results_view::shown();
    let loads = results_view::loads();
    let units = crate::model::read(|f| f.units);
    if loads.is_empty() {
        ui.label("LC: —");
        return;
    }
    let mut choice = shown.choice.clone();
    let current = choice.load.clone().unwrap_or_else(|| "—".into());
    egui::ComboBox::from_id_salt("factory_lc").selected_text(format!("LC: {current}")).show_ui(ui, |ui| {
        for l in &loads {
            ui.selectable_value(&mut choice.load, Some(l.clone()), l);
        }
    });
    let geometry = crate::transfer::geometry();
    if !geometry.shells.is_empty() {
        egui::ComboBox::from_id_salt("factory_shell_comp").selected_text(format!("Shell: {}", SHELL_COMPONENTS[choice.shell.min(7)])).show_ui(ui, |ui| {
            for (k, c) in SHELL_COMPONENTS.iter().enumerate() {
                ui.selectable_value(&mut choice.shell, k, *c);
            }
        });
    }
    if !geometry.frames.is_empty() {
        egui::ComboBox::from_id_salt("factory_frame_comp").selected_text(format!("Frame: {}", FRAME_COMPONENTS[choice.frame.min(5)])).show_ui(ui, |ui| {
            for (k, c) in FRAME_COMPONENTS.iter().enumerate() {
                ui.selectable_value(&mut choice.frame, k, *c);
            }
        });
    }
    if shown.envelope {
        egui::ComboBox::from_id_salt("factory_step").selected_text(if choice.step == Step::Max { "Max" } else { "Min" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut choice.step, Step::Max, "Max");
            ui.selectable_value(&mut choice.step, Step::Min, "Min");
        });
    }
    if shown.busy {
        ui.spinner();
    }
    if let Some(e) = &shown.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    if choice != shown.choice || shown.units != Some(units) {
        results_view::select(Choice { ..choice }, units);
        crate::model::write(|f| f.status.current_load = results_view::shown().choice.load);
    }
}

/// Drawn over the 3D view every frame: the legend and the value under the cursor.
pub fn overlay(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>) {
    let flags = crate::vv::overlay();
    if flags.shell_labels.is_some() || flags.shell_sections.is_some() || flags.frame_labels.is_some() || flags.frame_sections.is_some() {
        element_labels(ui, area, view, flags);
    }
    if flags.shell_axes.is_some() || flags.frame_axes.is_some() {
        axis_triads(ui, area, view, flags);
    }
    if flags.values {
        value_labels(ui, area, view);
    }
    if flags.legend {
        legend(ui, area);
    }
    value_snap(ui, area, view);
}

/// Subcategories Labels and Sections on the shown elements (Visibility/Graphics, docs/12 §2.1).
/// Each is drawn in its own colour, with its own one-label-per-64-px grid, nearest to the
/// camera first (rule P4); labels a line below a value label, sections a line below that.
fn element_labels(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay) {
    let g = crate::transfer::geometry();
    let data = results_view::snap_data();
    // Anchor points: shell centres and frame midpoints, shown elements only.
    let mut shells: Vec<(usize, [f64; 3])> = Vec::new();
    if flags.shell_labels.is_some() || flags.shell_sections.is_some() {
        for (e, s) in g.shells.iter().enumerate() {
            if !data.shell_shown.get(e).copied().unwrap_or(true) {
                continue;
            }
            let n = if s[3] == u32::MAX { 3 } else { 4 };
            let mut c = [0.0; 3];
            for k in 0..n {
                if let Some(p) = g.joints.get(s[k] as usize) {
                    for i in 0..3 {
                        c[i] += p[i] / n as f64;
                    }
                }
            }
            shells.push((e, c));
        }
    }
    let mut frames: Vec<(usize, [f64; 3])> = Vec::new();
    if flags.frame_labels.is_some() || flags.frame_sections.is_some() {
        for (e, f) in g.frames.iter().enumerate() {
            if !data.frame_shown.get(e).copied().unwrap_or(true) {
                continue;
            }
            if let (Some(a), Some(b)) = (g.joints.get(f[0] as usize), g.joints.get(f[1] as usize)) {
                frames.push((e, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]));
            }
        }
    }
    let mut layers: Vec<(Vec<([f64; 3], String)>, [u8; 3], f32)> = Vec::new();
    let name = |list: &Vec<String>, e: usize| list.get(e).cloned().unwrap_or_default();
    if let Some(c) = flags.shell_labels {
        layers.push((shells.iter().map(|(e, p)| (*p, name(&g.shell_names, *e))).collect(), c, 12.0));
    }
    if let Some(c) = flags.frame_labels {
        layers.push((frames.iter().map(|(e, p)| (*p, name(&g.frame_names, *e))).collect(), c, 12.0));
    }
    if let Some(c) = flags.shell_sections {
        layers.push((shells.iter().map(|(e, p)| (*p, name(&g.shell_sections, g.shells[*e][4] as usize))).collect(), c, 24.0));
    }
    if let Some(c) = flags.frame_sections {
        layers.push((frames.iter().map(|(e, p)| (*p, name(&g.frame_sections, g.frames[*e][2] as usize))).collect(), c, 24.0));
    }
    let dist = |p: [f64; 3]| (p[0] - view.eye[0]).powi(2) + (p[1] - view.eye[1]).powi(2) + (p[2] - view.eye[2]).powi(2);
    let painter = ui.painter_at(area);
    let font = egui::FontId::proportional(10.0);
    let cell = 64.0;
    for (mut items, [r, g_, b], dy) in layers {
        items.retain(|(_, t)| !t.is_empty());
        items.sort_by(|a, b| dist(a.0).total_cmp(&dist(b.0)));
        let colour = egui::Color32::from_rgb(r, g_, b);
        let mut taken = std::collections::HashSet::new();
        for (p, t) in items {
            let Some(s) = (view.project)(p).filter(|s| area.contains(*s)) else { continue };
            if taken.insert(((s.x / cell) as i32, (s.y / cell) as i32)) {
                painter.text(s + egui::vec2(0.0, dy), egui::Align2::CENTER_CENTER, t, font.clone(), colour);
            }
        }
    }
}

/// Local-axis triads (subcategory Local Axes, docs/12 §2.1): at each shown element's centre the
/// local axes 1 (red), 2 (green), 3 (blue) from CSiBridge, a quarter of the element size long;
/// one triad per 64 px cell, nearest to the camera first (rule P4).
fn axis_triads(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay) {
    let g = crate::transfer::geometry();
    let data = results_view::snap_data();
    let joint = |j: u32| g.joints.get(j as usize).copied();
    let len = |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    // (centre, size, matrix, halftone)
    let mut items: Vec<([f64; 3], f64, [f64; 9], bool)> = Vec::new();
    if let Some(half) = flags.frame_axes {
        for (e, f) in g.frames.iter().enumerate() {
            let (Some(a), Some(b), Some(m)) = (joint(f[0]), joint(f[1]), g.frame_axes.get(e)) else { continue };
            if data.frame_shown.get(e).copied().unwrap_or(true) {
                items.push(([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0], len(a, b), *m, half));
            }
        }
    }
    if let Some(half) = flags.shell_axes {
        for (e, s) in g.shells.iter().enumerate() {
            let Some(m) = g.shell_axes.get(e) else { continue };
            if !data.shell_shown.get(e).copied().unwrap_or(true) {
                continue;
            }
            let corners: Vec<[f64; 3]> = s[..4].iter().filter(|j| **j != u32::MAX).filter_map(|j| joint(*j)).collect();
            if corners.len() < 3 {
                continue;
            }
            let n = corners.len() as f64;
            let c = [0, 1, 2].map(|i| corners.iter().map(|p| p[i]).sum::<f64>() / n);
            items.push((c, len(corners[0], corners[2]), *m, half));
        }
    }
    let dist = |p: [f64; 3]| (p[0] - view.eye[0]).powi(2) + (p[1] - view.eye[1]).powi(2) + (p[2] - view.eye[2]).powi(2);
    items.sort_by(|a, b| dist(a.0).total_cmp(&dist(b.0)));
    let painter = ui.painter_at(area);
    let colours: [[u8; 3]; 3] = [[220, 50, 40], [40, 170, 60], [40, 90, 220]];
    let mut taken = std::collections::HashSet::new();
    for (c, size, m, half) in items {
        let Some(axes) = crate::transfer::local_axes(&m) else { continue };
        let Some(o) = (view.project)(c).filter(|s| area.contains(*s)) else { continue };
        if !taken.insert(((o.x / 64.0) as i32, (o.y / 64.0) as i32)) {
            continue;
        }
        let l = 0.25 * size;
        for (axis, rgb) in axes.iter().zip(colours) {
            let tip = [c[0] + axis[0] * l, c[1] + axis[1] * l, c[2] + axis[2] * l];
            if let Some(t) = (view.project)(tip) {
                let [r, g_, b] = if half { crate::vv::halftone(rgb) } else { rgb };
                painter.line_segment([o, t], egui::Stroke::new(2.0, egui::Color32::from_rgb(r, g_, b)));
            }
        }
    }
}

/// Values on the elements (docs/10 rule P4): only inside the view, at most one label per 48 px
/// cell, nearest to the camera first. Shells: the mean of their corner values at the centre;
/// frames: each result station.
fn value_labels(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>) {
    let data = results_view::snap_data();
    if data.shell_values.is_empty() && data.frame_points.is_empty() {
        return;
    }
    let g = crate::transfer::geometry();
    let dist = |p: [f64; 3]| (p[0] - view.eye[0]).powi(2) + (p[1] - view.eye[1]).powi(2) + (p[2] - view.eye[2]).powi(2);
    let mut points: Vec<([f64; 3], f32)> = Vec::new();
    for (e, s) in g.shells.iter().enumerate() {
        let Some(v) = data.shell_values.get(e).filter(|v| !v[0].is_nan()) else { continue };
        let n = if s[3] == u32::MAX { 3 } else { 4 };
        let mut c = [0.0; 3];
        for k in 0..n {
            if let Some(p) = g.joints.get(s[k] as usize) {
                for i in 0..3 {
                    c[i] += p[i] / n as f64;
                }
            }
        }
        points.push((c, v[..n].iter().sum::<f32>() / n as f32));
    }
    points.extend(data.frame_points.iter().copied());
    points.sort_by(|a, b| dist(a.0).total_cmp(&dist(b.0)));
    let painter = ui.painter_at(area);
    let font = egui::FontId::proportional(10.0);
    let colour = ui.visuals().text_color();
    let cell = 48.0;
    let mut taken = std::collections::HashSet::new();
    for (p, v) in points {
        let Some(s) = (view.project)(p).filter(|s| area.contains(*s)) else { continue };
        if taken.insert(((s.x / cell) as i32, (s.y / cell) as i32)) {
            painter.text(s, egui::Align2::CENTER_CENTER, format_value(v), font.clone(), colour);
        }
    }
}

/// The value under the cursor, as in CSiBridge: a small square on the corner/station and its value.
fn value_snap(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>) {
    let Some(cursor) = ui.input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p)) else { return };
    let data = results_view::snap_data();
    if data.shell_values.is_empty() && data.frame_points.is_empty() {
        return;
    }
    let geometry = crate::transfer::geometry();
    let project = |p: [f64; 3]| (view.project)(p).map(|s| [s.x, s.y]);
    let Some(hit) = results_view::snap(&geometry, &data, [cursor.x, cursor.y], &project, view.eye, 10.0) else { return };
    let Some(at) = (view.project)(hit.at) else { return };
    let painter = ui.painter_at(area);
    let orange = egui::Color32::from_rgb(240, 130, 30);
    painter.rect_stroke(egui::Rect::from_center_size(at, egui::vec2(9.0, 9.0)), 0.0, egui::Stroke::new(1.5, orange), egui::StrokeKind::Middle);
    let text = format!("{} {}", format_value(hit.value), hit.unit);
    let font = egui::FontId::proportional(13.0);
    let galley = painter.layout_no_wrap(text, font, egui::Color32::BLACK);
    let pos = at + egui::vec2(8.0, -8.0 - galley.size().y);
    painter.rect_filled(egui::Rect::from_min_size(pos, galley.size()).expand(2.0), 2.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 210));
    painter.galley(pos, galley, egui::Color32::BLACK);
}

/// The legend: one colour bar per shown layer, at the right edge of the 3D view.
fn legend(ui: &mut egui::Ui, area: egui::Rect) {
    let shown = results_view::shown();
    let bars: Vec<&Legend> = shown.shell.iter().chain(shown.frame.iter()).collect();
    if bars.is_empty() {
        return;
    }
    let painter = ui.painter_at(area);
    let text = ui.visuals().text_color();
    let font = egui::FontId::proportional(11.0);
    let (bar_w, bar_h, gap) = (14.0, (area.height() * 0.35).clamp(120.0, 260.0), 18.0);
    let mut top = area.top() + 12.0;
    for bar in bars {
        let left = area.right() - 120.0;
        painter.text(egui::pos2(left, top), egui::Align2::LEFT_TOP, &bar.title, font.clone(), text);
        painter.text(egui::pos2(left, top + 14.0), egui::Align2::LEFT_TOP, format!("[{}]", bar.unit), font.clone(), text);
        let rect = egui::Rect::from_min_size(egui::pos2(left, top + 32.0), egui::vec2(bar_w, bar_h));
        // High values on top, as the ramp's red end.
        let mut mesh = egui::Mesh::default();
        let steps = 32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let y = rect.bottom() - t * rect.height();
            let [r, g, b] = ramp(t);
            let c = egui::Color32::from_rgb(r, g, b);
            mesh.colored_vertex(egui::pos2(rect.left(), y), c);
            mesh.colored_vertex(egui::pos2(rect.right(), y), c);
            if i > 0 {
                let k = (i as u32) * 2;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k - 1, k + 1, k);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        for i in 0..=4 {
            let t = i as f32 / 4.0;
            let v = bar.range[0] + (bar.range[1] - bar.range[0]) * t;
            let y = rect.bottom() - t * rect.height();
            painter.text(egui::pos2(rect.right() + 6.0, y), egui::Align2::LEFT_CENTER, format_value(v), font.clone(), text);
        }
        top = rect.bottom() + gap;
    }
}

/// Short numbers for the legend: 3 significant digits, exponent for very large/small.
fn format_value(v: f32) -> String {
    let a = v.abs();
    if a != 0.0 && !(1e-3..1e5).contains(&a) { format!("{v:.2e}") } else { format!("{v:.3}") }
}
