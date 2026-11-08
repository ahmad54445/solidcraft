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

/// Texts of one shell, top to bottom, laid flat on its surface.
#[derive(Clone, Debug)]
struct ShellText {
    shell: usize,
    lines: Vec<(String, egui::Color32)>,
}

/// A frame text: 2D, next to the frame's midpoint (frames have no surface to lie on).
#[derive(Clone, Debug)]
struct FrameText {
    anchor: [f64; 3],
    text: String,
    colour: egui::Color32,
    dy: f32,
}

/// What was decided at the last camera stop.
#[derive(Clone)]
struct LabelSet {
    camera: [f32; 7],
    key: (usize, usize, crate::vv::Overlay, bool),
    shells: std::sync::Arc<Vec<ShellText>>,
    frames: std::sync::Arc<Vec<FrameText>>,
}

/// Values, Labels and Sections (docs/10 P4, docs/12 §2.1).
/// - On **shells** the text lies **flat on the shell** (UI font, along local axes 1 and 2, sized
///   to the shell, turned so it is never mirrored or upside down).
/// - On **frames** it is 2D text next to the midpoint (one per 64 px cell, element order).
/// Which texts are shown – and which are hidden behind shown shells (ray test, screen-bucketed)
/// – is decided **once per camera stop**. While the camera moves the chosen texts stay and move
/// with their elements (a text may show through a surface until the camera stops).
fn stable_labels(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay) {
    let g = crate::transfer::geometry();
    let data = results_view::snap_data();
    if g.joints.is_empty() {
        return;
    }
    let probe = |p: [f64; 3]| (view.project)(p).map_or([f32::NAN; 2], |s| [s.x, s.y]);
    let (p0, p1) = (probe(g.joints[0]), probe(g.joints[g.joints.len() - 1]));
    let camera = [view.eye[0] as f32, view.eye[1] as f32, view.eye[2] as f32, p0[0], p0[1], p1[0], p1[1]];
    let extrude = crate::vv::settings().view.view_type == crate::vv::ViewType::Extrude;
    let key = (std::sync::Arc::as_ptr(&g) as usize, std::sync::Arc::as_ptr(&data) as usize, flags, extrude);
    let ctx = ui.ctx().clone();
    let (set_id, moved_id) = (egui::Id::unique("factory_label_set"), egui::Id::unique("factory_label_moved"));
    let now = ctx.input(|i| i.time);
    let same = |a: &[f32; 7], b: &[f32; 7]| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 0.5 || (x.is_nan() && y.is_nan()));
    let (last_camera, last_move) = ctx.data(|d| d.get_temp::<([f32; 7], f64)>(moved_id)).unwrap_or((camera, f64::NEG_INFINITY));
    let last_move = if same(&last_camera, &camera) { last_move } else { now };
    ctx.data_mut(|d| d.insert_temp(moved_id, (camera, last_move)));
    let set = ctx.data(|d| d.get_temp::<LabelSet>(set_id));
    let settled = now - last_move >= 0.2;
    let stale = set.as_ref().is_none_or(|s| s.key != key || (settled && !same(&s.camera, &camera)));
    let set = if stale && (settled || set.as_ref().is_none_or(|s| s.key != key)) {
        let (shells, frames) = choose(&g, &data, area, view, flags);
        let fresh = LabelSet { camera, key, shells: std::sync::Arc::new(shells), frames: std::sync::Arc::new(frames) };
        ctx.data_mut(|d| d.insert_temp(set_id, fresh.clone()));
        fresh
    } else {
        if !settled {
            ctx.request_repaint_after(std::time::Duration::from_millis(220)); // decide again once it stops
        }
        let Some(set) = set else { return };
        set
    };
    let painter = ui.painter_at(area);
    for t in set.shells.iter() {
        shell_decal(&painter, &g, t, view, extrude);
    }
    let font = egui::FontId::proportional(11.0);
    for t in set.frames.iter() {
        if let Some(s) = (view.project)(t.anchor).filter(|s| area.contains(*s)) {
            painter.text(s + egui::vec2(0.0, t.dy), egui::Align2::CENTER_CENTER, &t.text, font.clone(), t.colour);
        }
    }
}

/// Decide the texts for this camera: shells with their lines (not hidden behind other shells),
/// frames one per 64 px cell in element order (not hidden).
fn choose(g: &crate::transfer::Geometry, data: &results_view::SnapData, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay) -> (Vec<ShellText>, Vec<FrameText>) {
    let project = |p: [f64; 3]| (view.project)(p).map(|s| [s.x, s.y]);
    let occluder = results_view::Occluder::new(g, &data.shell_shown, view.eye, &project, 32.0);
    let rgb = |[r, g_, b]: [u8; 3]| egui::Color32::from_rgb(r, g_, b);
    let name = |list: &Vec<String>, k: usize| list.get(k).cloned().unwrap_or_default();
    let value_colour = egui::Color32::from_rgb(20, 20, 24);

    let mut shells = Vec::new();
    for (e, s) in g.shells.iter().enumerate() {
        if !data.shell_shown.get(e).copied().unwrap_or(true) {
            continue;
        }
        let mut lines = Vec::new();
        if let Some(c) = flags.shell_labels {
            lines.push((name(&g.shell_names, e), rgb(c)));
        }
        if flags.values
            && let Some(v) = data.shell_values.get(e).filter(|v| !v[0].is_nan())
        {
            let n = if s[3] == u32::MAX { 3 } else { 4 };
            lines.push((format_value(v[..n].iter().sum::<f32>() / n as f32), value_colour));
        }
        if let Some(c) = flags.shell_sections {
            lines.push((name(&g.shell_sections, s[4] as usize), rgb(c)));
        }
        lines.retain(|(t, _)| !t.is_empty());
        if lines.is_empty() {
            continue;
        }
        let c = shell_centre(g, e);
        let Some(sc) = project(c).filter(|p| area.contains(egui::pos2(p[0], p[1]))) else { continue };
        if !occluder.hidden(c, sc, Some(e)) {
            shells.push(ShellText { shell: e, lines });
        }
    }

    let mut frames = Vec::new();
    let mid = |e: usize| {
        let f = g.frames[e];
        match (g.joints.get(f[0] as usize), g.joints.get(f[1] as usize)) {
            (Some(a), Some(b)) => Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]),
            _ => None,
        }
    };
    let mut kinds: Vec<(Vec<([f64; 3], String)>, egui::Color32, f32)> = Vec::new();
    if let Some(col) = flags.frame_labels {
        kinds.push(((0..g.frames.len()).filter(|e| data.frame_shown.get(*e).copied().unwrap_or(true)).filter_map(|e| mid(e).map(|p| (p, name(&g.frame_names, e)))).collect(), rgb(col), 12.0));
    }
    if flags.values {
        kinds.push((data.frame_points.iter().map(|(p, v)| (*p, format_value(*v))).collect(), value_colour, 0.0));
    }
    if let Some(col) = flags.frame_sections {
        kinds.push(((0..g.frames.len()).filter(|e| data.frame_shown.get(*e).copied().unwrap_or(true)).filter_map(|e| mid(e).map(|p| (p, name(&g.frame_sections, g.frames[e][2] as usize)))).collect(), rgb(col), 24.0));
    }
    for (candidates, colour, dy) in kinds {
        let mut taken = std::collections::HashSet::new();
        for (anchor, text) in candidates {
            if text.is_empty() {
                continue;
            }
            let Some(sp) = project(anchor).filter(|p| area.contains(egui::pos2(p[0], p[1]))) else { continue };
            let slot = ((sp[0] / 64.0) as i32, (sp[1] / 64.0) as i32);
            if !taken.contains(&slot) && !occluder.hidden(anchor, sp, None) {
                taken.insert(slot);
                frames.push(FrameText { anchor, text, colour, dy });
            }
        }
    }
    (shells, frames)
}

fn shell_centre(g: &crate::transfer::Geometry, e: usize) -> [f64; 3] {
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
}

/// Lines of text laid flat on a shell: the UI font's glyph mesh mapped onto the shell plane (local
/// axes 1 and 2 from CSiBridge; the first edge and the normal when missing), sized to the shell
/// (each line ≤ 22 % of its height, ≤ 85 % of its width), turned so it reads correctly from the
/// side and the angle it is seen from. On the top face when extruded.
fn shell_decal(painter: &egui::Painter, g: &crate::transfer::Geometry, t: &ShellText, view: &ViewInfo<'_>, extrude: bool) {
    let s = g.shells[t.shell];
    let n = if s[3] == u32::MAX { 3 } else { 4 };
    let corners: Vec<[f64; 3]> = s[..n].iter().filter_map(|j| g.joints.get(*j as usize).copied()).collect();
    if corners.len() < 3 {
        return;
    }
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |u: [f64; 3], v: [f64; 3]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let unit = |v: [f64; 3]| {
        let l = dot(v, v).sqrt();
        if l > 1e-12 { Some(v.map(|c| c / l)) } else { None }
    };
    let c = shell_centre(g, t.shell);
    let axes = g.shell_axes.get(t.shell).and_then(crate::transfer::local_axes);
    let (mut right, mut up, normal) = match axes {
        Some([a1, a2, a3]) => (a1, a2, a3),
        None => {
            let Some(r) = unit(sub(corners[1], corners[0])) else { return };
            let Some(nrm) = unit(cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]))) else { return };
            (r, cross(nrm, r), nrm)
        }
    };
    // The face toward the eye (top face when extruded); seen from behind, mirror back.
    let facing = dot(normal, sub(view.eye, c)) >= 0.0;
    let half = if extrude { g.shell_thickness.get(s[4] as usize).copied().unwrap_or(0.0) / 2.0 } else { 0.0 };
    let side = if facing { 1.0 } else { -1.0 };
    let origin = [0, 1, 2].map(|i| c[i] + normal[i] * side * half);
    if !facing {
        right = right.map(|x| -x);
    }
    // Upside down on screen → turn by 180°.
    let (Some(o), Some(u)) = ((view.project)(origin), (view.project)([origin[0] + up[0], origin[1] + up[1], origin[2] + up[2]])) else { return };
    if u.y > o.y {
        right = right.map(|x| -x);
        up = up.map(|x| -x);
    }
    // The shell's extent along right / up.
    let (mut w, mut h) = (0.0f64, 0.0f64);
    for p in &corners {
        let d = sub(*p, c);
        w = w.max(dot(d, right).abs() * 2.0);
        h = h.max(dot(d, up).abs() * 2.0);
    }
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let font = egui::FontId::proportional(32.0);
    let galleys: Vec<_> = t.lines.iter().map(|(text, colour)| painter.layout_no_wrap(text.clone(), font.clone(), *colour)).collect();
    let line_px = galleys.iter().map(|g| g.size().y).fold(0.0f32, f32::max).max(1.0);
    let widest_px = galleys.iter().map(|g| g.size().x).fold(0.0f32, f32::max).max(1.0);
    // World units per galley pixel: line height ≤ 22 % of the shell, width ≤ 85 %.
    let k = (0.22 * h / f64::from(line_px)).min(0.85 * w / f64::from(widest_px));
    // Too small on screen to read: skip.
    let Some(top) = (view.project)([origin[0] + up[0] * k * f64::from(line_px), origin[1] + up[1] * k * f64::from(line_px), origin[2] + up[2] * k * f64::from(line_px)]) else { return };
    if (top - o).length() < 5.0 {
        return;
    }
    let tex = painter.fonts(|f| f.font_image_size());
    let uv_scale = egui::vec2(1.0 / tex[0] as f32, 1.0 / tex[1] as f32);
    let total = f64::from(line_px) * galleys.len() as f64;
    let mut mesh = egui::Mesh::default();
    for (i, galley) in galleys.iter().enumerate() {
        let gw = f64::from(galley.size().x);
        // Line i's top, measured down from the block's top (block centred on the shell).
        let y_top = total / 2.0 - f64::from(line_px) * i as f64;
        let colour = t.lines[i].1;
        for row in &galley.rows {
            let base = mesh.vertices.len() as u32;
            for v in &row.row.visuals.mesh.vertices {
                let (gx, gy) = (f64::from(row.pos.x + v.pos.x), f64::from(row.pos.y + v.pos.y));
                let (x, y) = ((gx - gw / 2.0) * k, (y_top - gy) * k);
                let world = [0, 1, 2].map(|j| origin[j] + right[j] * x + up[j] * y);
                let Some(p) = (view.project)(world) else { return };
                let col = if v.color == egui::Color32::PLACEHOLDER { colour } else { v.color };
                mesh.vertices.push(egui::epaint::Vertex { pos: p, uv: (v.uv.to_vec2() * uv_scale).to_pos2(), color: col });
            }
            mesh.indices.extend(row.row.visuals.mesh.indices.iter().map(|ix| base + ix));
        }
    }
    painter.add(egui::Shape::mesh(mesh));
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
