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

/// Drawn over the 3D view every frame: the legend and the value under the cursor. Labels,
/// sections and values are drawn by the GPU (see `stable_labels`).
pub fn overlay(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>) {
    let flags = crate::vv::overlay();
    stable_labels(ui, area, view, flags);
    if flags.legend {
        legend(ui, area);
    }
    value_snap(ui, area, view);
}

/// One text block in the view: lines (text, colour) stacked around a world anchor, lifted toward
/// the eye by `lift` (world units; half the thickness when extruded).
#[derive(Clone, Debug)]
struct Placed {
    anchor: [f64; 3],
    lift: f64,
    lines: Vec<(String, [u8; 3])>,
}

/// What the texts were last chosen for.
#[derive(Clone, PartialEq)]
struct LabelSet {
    camera: [f32; 7],
    key: (usize, usize, crate::vv::Overlay, bool, u32),
}

/// Text size (points per em) and line distance (points).
const TEXT_PT: f32 = 12.0;
const LINE_PT: f32 = 13.0;
/// The SDF atlas as uploaded (one, built once).
const ATLAS_ID: u64 = 1;

fn atlas() -> &'static std::sync::Arc<custom_bridge::view::SpriteAtlas> {
    static ATLAS: std::sync::OnceLock<std::sync::Arc<custom_bridge::view::SpriteAtlas>> = std::sync::OnceLock::new();
    ATLAS.get_or_init(|| {
        let f = crate::sdf::font();
        std::sync::Arc::new(custom_bridge::view::SpriteAtlas { size: f.size, texels: f.texels.clone() })
    })
}

/// Values, Labels and Sections (docs/10 P4, docs/12 §2.1) as **GPU text** (signed distance field
/// font, `sdf.rs`): 2D, facing the screen, constant size, drawn by the GPU every frame at its
/// element with the model's depth – a surface in front hides it, also while orbiting.
/// **Which** texts are shown is decided once per camera stop (0.2 s still): element order, a text
/// only where it does not overlap one already placed and not behind a shown shell. While the
/// camera moves, the chosen texts stay (no jumping) and move with their elements.
fn stable_labels(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay) {
    let g = crate::transfer::geometry();
    let data = results_view::snap_data();
    let ctx = ui.ctx().clone();
    let (set_id, moved_id) = (egui::Id::unique("factory_label_set"), egui::Id::unique("factory_label_moved"));
    let wanted = flags.values || flags.shell_labels.is_some() || flags.shell_sections.is_some() || flags.frame_labels.is_some() || flags.frame_sections.is_some();
    if !wanted || g.joints.is_empty() {
        if ctx.data(|d| d.get_temp::<LabelSet>(set_id)).is_some() {
            custom_bridge::view::clear_sprites();
            ctx.data_mut(|d| d.remove::<LabelSet>(set_id));
            ctx.request_repaint();
        }
        return;
    }
    let probe = |p: [f64; 3]| (view.project)(p).map_or([f32::NAN; 2], |s| [s.x, s.y]);
    let (p0, p1) = (probe(g.joints[0]), probe(g.joints[g.joints.len() - 1]));
    let camera = [view.eye[0] as f32, view.eye[1] as f32, view.eye[2] as f32, p0[0], p0[1], p1[0], p1[1]];
    let extrude = crate::vv::settings().view.view_type == crate::vv::ViewType::Extrude;
    let ppp = ctx.pixels_per_point();
    let key = (std::sync::Arc::as_ptr(&g) as usize, std::sync::Arc::as_ptr(&data) as usize, flags, extrude, ppp.to_bits());
    let now = ctx.input(|i| i.time);
    let same = |a: &[f32; 7], b: &[f32; 7]| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 0.5 || (x.is_nan() && y.is_nan()));
    let (last_camera, last_move) = ctx.data(|d| d.get_temp::<([f32; 7], f64)>(moved_id)).unwrap_or((camera, f64::NEG_INFINITY));
    let last_move = if same(&last_camera, &camera) { last_move } else { now };
    ctx.data_mut(|d| d.insert_temp(moved_id, (camera, last_move)));
    let set = ctx.data(|d| d.get_temp::<LabelSet>(set_id));
    let changed = set.as_ref().is_none_or(|s| s.key != key);
    let moved = set.as_ref().is_some_and(|s| !same(&s.camera, &camera));
    if !changed && !moved {
        return;
    }
    if !changed && now - last_move < 0.2 {
        ctx.request_repaint_after(std::time::Duration::from_millis(220)); // choose again once it stops
        return;
    }
    let placed = choose(&g, &data, area, view, flags, extrude);
    custom_bridge::view::show_sprites(atlas(), ATLAS_ID, sprites(&placed, ppp));
    ctx.data_mut(|d| d.insert_temp(set_id, LabelSet { camera, key }));
    ctx.request_repaint();
}

/// The glyph sprites of the placed texts (physical pixels).
fn sprites(placed: &[Placed], ppp: f32) -> Vec<custom_bridge::view::Sprite> {
    let font = crate::sdf::font();
    let (px, line) = (TEXT_PT * ppp, LINE_PT * ppp);
    let mut out = Vec::new();
    for t in placed {
        let n = t.lines.len() as f32;
        for (i, (text, [r, g, b])) in t.lines.iter().enumerate() {
            let dy = (i as f32 - (n - 1.0) / 2.0) * line;
            for (rect, uv) in font.layout(text, px, dy) {
                out.push(custom_bridge::view::Sprite { anchor: t.anchor.map(|v| v as f32), lift: t.lift as f32, rect, uv, rgba: [*r, *g, *b, 255] });
            }
        }
    }
    out
}

/// Decide the texts for this camera, in element order (shells, then frames, then frame values):
/// a block is taken where its screen rectangle overlaps no block taken before (16 px grid) and no
/// shown shell lies in front of it (ray test, screen-bucketed).
fn choose(g: &crate::transfer::Geometry, data: &results_view::SnapData, area: egui::Rect, view: &ViewInfo<'_>, flags: crate::vv::Overlay, extrude: bool) -> Vec<Placed> {
    let project = |p: [f64; 3]| (view.project)(p).map(|s| [s.x, s.y]);
    let occluder = results_view::Occluder::new(g, &data.shell_shown, view.eye, &project, 32.0);
    let name = |list: &Vec<String>, k: usize| list.get(k).cloned().unwrap_or_default();
    let value_colour = [20, 20, 24];
    let font = crate::sdf::font();
    let mut taken: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
    let mut out = Vec::new();
    // Take a block if it fits; `own` is its shell (not counted as in front of it).
    let mut take = |p: Placed, own: Option<usize>| {
        let lines: Vec<(String, [u8; 3])> = p.lines.into_iter().filter(|(t, _)| !t.is_empty()).collect();
        if lines.is_empty() {
            return;
        }
        let Some(sp) = project(p.anchor).filter(|s| area.contains(egui::pos2(s[0], s[1]))) else { return };
        let w = lines.iter().map(|(t, _)| font.width(t)).fold(0.0f32, f32::max) * TEXT_PT + 4.0;
        let h = lines.len() as f32 * LINE_PT;
        let cells = |lo: f32, hi: f32| ((lo / 16.0).floor() as i32)..=((hi / 16.0).floor() as i32);
        let block: Vec<(i32, i32)> = cells(sp[0] - w / 2.0, sp[0] + w / 2.0).flat_map(|x| cells(sp[1] - h / 2.0, sp[1] + h / 2.0).map(move |y| (x, y))).collect();
        if block.iter().any(|c| taken.contains(c)) || occluder.hidden(p.anchor, sp, own) {
            return;
        }
        taken.extend(block);
        out.push(Placed { lines, ..p });
    };
    for (e, s) in g.shells.iter().enumerate() {
        if !data.shell_shown.get(e).copied().unwrap_or(true) {
            continue;
        }
        let mut lines = Vec::new();
        if let Some(c) = flags.shell_labels {
            lines.push((name(&g.shell_names, e), c));
        }
        if flags.values
            && let Some(v) = data.shell_values.get(e).filter(|v| !v[0].is_nan())
        {
            let n = if s[3] == u32::MAX { 3 } else { 4 };
            lines.push((format_value(v[..n].iter().sum::<f32>() / n as f32), value_colour));
        }
        if let Some(c) = flags.shell_sections {
            lines.push((name(&g.shell_sections, s[4] as usize), c));
        }
        let lift = if extrude { g.shell_thickness.get(s[4] as usize).copied().unwrap_or(0.0) / 2.0 } else { 0.0 };
        take(Placed { anchor: shell_centre(g, e), lift, lines }, Some(e));
    }
    for (e, f) in g.frames.iter().enumerate() {
        if !data.frame_shown.get(e).copied().unwrap_or(true) {
            continue;
        }
        let (Some(a), Some(b)) = (g.joints.get(f[0] as usize), g.joints.get(f[1] as usize)) else { continue };
        let mut lines = Vec::new();
        if let Some(c) = flags.frame_labels {
            lines.push((name(&g.frame_names, e), c));
        }
        if let Some(c) = flags.frame_sections {
            lines.push((name(&g.frame_sections, f[2] as usize), c));
        }
        take(Placed { anchor: [0, 1, 2].map(|i| (a[i] + b[i]) / 2.0), lift: 0.0, lines }, None);
    }
    if flags.values {
        for (p, v) in &data.frame_points {
            take(Placed { anchor: *p, lift: 0.0, lines: vec![(format_value(*v), value_colour)] }, None);
        }
    }
    drop(take);
    out
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
