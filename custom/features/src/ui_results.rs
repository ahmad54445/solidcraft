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
    if flags.values || flags.shell_labels.is_some() || flags.shell_sections.is_some() || flags.frame_labels.is_some() || flags.frame_sections.is_some() {
        stable_labels(ui, area, view, flags);
    }
    if flags.legend {
        legend(ui, area);
    }
    value_snap(ui, area, view);
}

/// One text on the view, anchored to a world point.
#[derive(Clone, Debug)]
struct Placed {
    anchor: [f64; 3],
    text: String,
    colour: egui::Color32,
    /// Vertical offset (px): values 0, labels +12, sections +24.
    dy: f32,
}

/// The texts chosen at the last camera stop, and for what.
#[derive(Clone)]
struct LabelCache {
    camera: [f32; 7],
    key: (usize, usize, crate::vv::Overlay),
    items: std::sync::Arc<Vec<Placed>>,
}

/// Values, Labels and Sections (docs/10 P4, docs/12 §2.1). Which texts are shown is decided
/// **once per camera stop** – thinned per grid cell with a fixed order (element number, not
/// "nearest", which changes every frame) and hidden when a shown shell lies between the eye and
/// the text (ray test). While the camera moves, the chosen texts just follow their elements.
fn stable_labels(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay) {
    let g = crate::transfer::geometry();
    let data = results_view::snap_data();
    if g.joints.is_empty() {
        return;
    }
    // Camera signature: the eye and two model points on screen.
    let probe = |p: [f64; 3]| (view.project)(p).map_or([f32::NAN; 2], |s| [s.x, s.y]);
    let (p0, p1) = (probe(g.joints[0]), probe(g.joints[g.joints.len() - 1]));
    let camera = [view.eye[0] as f32, view.eye[1] as f32, view.eye[2] as f32, p0[0], p0[1], p1[0], p1[1]];
    let key = (std::sync::Arc::as_ptr(&g) as usize, std::sync::Arc::as_ptr(&data) as usize, flags);
    let ctx = ui.ctx().clone();
    let (cache_id, moved_id) = (egui::Id::unique("factory_label_cache"), egui::Id::unique("factory_label_moved"));
    let now = ctx.input(|i| i.time);
    let cache = ctx.data(|d| d.get_temp::<LabelCache>(cache_id));
    let (last_camera, last_move) = ctx.data(|d| d.get_temp::<([f32; 7], f64)>(moved_id)).unwrap_or((camera, f64::NEG_INFINITY));
    let same = |a: &[f32; 7], b: &[f32; 7]| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 0.5 || (x.is_nan() && y.is_nan()));
    let last_move = if same(&last_camera, &camera) { last_move } else { now };
    ctx.data_mut(|d| d.insert_temp(moved_id, (camera, last_move)));
    let settled = now - last_move >= 0.2;
    let stale = cache.as_ref().is_none_or(|c| c.key != key || (settled && !same(&c.camera, &camera)));
    let items = match cache {
        Some(c) if !stale => c.items,
        Some(c) if !settled && c.key == key => {
            ctx.request_repaint_after(std::time::Duration::from_millis(220)); // lay out again once it stops
            c.items
        }
        _ => {
            let items = std::sync::Arc::new(layout(&g, &data, area, view, flags));
            ctx.data_mut(|d| d.insert_temp(cache_id, LabelCache { camera, key, items: items.clone() }));
            items
        }
    };
    let painter = ui.painter_at(area);
    let font = egui::FontId::proportional(10.0);
    for t in items.iter() {
        if let Some(s) = (view.project)(t.anchor).filter(|s| area.contains(*s)) {
            painter.text(s + egui::vec2(0.0, t.dy), egui::Align2::CENTER_CENTER, &t.text, font.clone(), t.colour);
        }
    }
}

/// Choose the texts for the current camera: per kind its own grid (values 48 px, names 64 px),
/// candidates in element order, a cell taken by the first visible one.
fn layout(g: &crate::transfer::Geometry, data: &results_view::SnapData, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay) -> Vec<Placed> {
    let shown_shell = |e: usize| data.shell_shown.get(e).copied().unwrap_or(true);
    let shown_frame = |e: usize| data.frame_shown.get(e).copied().unwrap_or(true);
    let centre = |e: usize| {
        let s = g.shells[e];
        let n = if s[3] == u32::MAX { 3 } else { 4 };
        let mut c = [0.0; 3];
        for k in 0..n {
            if let Some(p) = g.joints.get(s[k] as usize) {
                for i in 0..3 {
                    c[i] += p[i] / n as f64;
                }
            }
        }
        c
    };
    let mid = |e: usize| {
        let f = g.frames[e];
        match (g.joints.get(f[0] as usize), g.joints.get(f[1] as usize)) {
            (Some(a), Some(b)) => Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]),
            _ => None,
        }
    };
    // (anchor, text, own shell, colour, dy, cell)
    type Candidate = ([f64; 3], String, Option<usize>);
    let mut kinds: Vec<(Vec<Candidate>, egui::Color32, f32, f32)> = Vec::new();
    let rgb = |[r, g_, b]: [u8; 3]| egui::Color32::from_rgb(r, g_, b);
    if flags.values {
        let mut c: Vec<Candidate> = Vec::new();
        for (e, v) in data.shell_values.iter().enumerate() {
            if v[0].is_nan() || !shown_shell(e) {
                continue;
            }
            let n = if g.shells[e][3] == u32::MAX { 3 } else { 4 };
            c.push((centre(e), format_value(v[..n].iter().sum::<f32>() / n as f32), Some(e)));
        }
        c.extend(data.frame_points.iter().map(|(p, v)| (*p, format_value(*v), None)));
        kinds.push((c, ui_text_colour(), 0.0, 48.0));
    }
    let name = |list: &Vec<String>, k: usize| list.get(k).cloned().unwrap_or_default();
    if let Some(col) = flags.shell_labels {
        let c = (0..g.shells.len()).filter(|e| shown_shell(*e)).map(|e| (centre(e), name(&g.shell_names, e), Some(e))).collect();
        kinds.push((c, rgb(col), 12.0, 64.0));
    }
    if let Some(col) = flags.frame_labels {
        let c = (0..g.frames.len()).filter(|e| shown_frame(*e)).filter_map(|e| mid(e).map(|p| (p, name(&g.frame_names, e), None))).collect();
        kinds.push((c, rgb(col), 12.0, 64.0));
    }
    if let Some(col) = flags.shell_sections {
        let c = (0..g.shells.len()).filter(|e| shown_shell(*e)).map(|e| (centre(e), name(&g.shell_sections, g.shells[e][4] as usize), Some(e))).collect();
        kinds.push((c, rgb(col), 24.0, 64.0));
    }
    if let Some(col) = flags.frame_sections {
        let c = (0..g.frames.len()).filter(|e| shown_frame(*e)).filter_map(|e| mid(e).map(|p| (p, name(&g.frame_sections, g.frames[e][2] as usize), None))).collect();
        kinds.push((c, rgb(col), 24.0, 64.0));
    }
    let mut out = Vec::new();
    for (candidates, colour, dy, cell) in kinds {
        let mut taken = std::collections::HashSet::new();
        for (anchor, text, own) in candidates {
            if text.is_empty() {
                continue;
            }
            let Some(s) = (view.project)(anchor).filter(|s| area.contains(*s)) else { continue };
            let slot = ((s.x / cell) as i32, (s.y / cell) as i32);
            if taken.contains(&slot) || results_view::hidden_by_shells(g, &data.shell_shown, view.eye, anchor, own) {
                continue;
            }
            taken.insert(slot);
            out.push(Placed { anchor, text, colour, dy });
        }
    }
    out
}

/// Text colour for values (dark on the light view).
fn ui_text_colour() -> egui::Color32 {
    egui::Color32::from_rgb(20, 20, 24)
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
