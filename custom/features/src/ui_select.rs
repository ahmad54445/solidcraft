//! Selecting in the 3D view (docs/13-selection.md): the screen tools T1–T4 on the view, the
//! highlight of the selection (R6), the keys (Esc, Ctrl+A) and the dialogs of T5–T10.

use custom_bridge::ui::egui;
use custom_bridge::view::{Segment, ViewInfo};

use crate::select_cmd::{self, Dialog, Tool};
use crate::selection::{self, Area, Kinds, Parallel, Scope};
use crate::{results_view, transfer};

/// The selection colour (R6): bright, unlike the result ramp's ends.
const SELECTION_COLOUR: [u8; 3] = [255, 196, 0];

/// Every frame over the 3D view: gestures, clicks, keys, the polygon being drawn and the highlight.
pub fn overlay(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>) {
    let g = transfer::geometry();
    let gestures = custom_bridge::view::take_box_gestures();
    highlight(&g);
    if g.shells.is_empty() && g.frames.is_empty() || crate::cut::mode().is_some() {
        return; // nothing to select, or Cut Diagram mode takes the clicks (docs/16 C1)
    }
    let data = results_view::snap_data();
    let project = |p: [f64; 3]| (view.project)(p).map(|s| [s.x, s.y]);
    let scope = Scope { g: &g, shell_shown: &data.shell_shown, frame_shown: &data.frame_shown, kinds: selection::kinds() };
    // Keys count only while nothing takes typed text (a dialog's text field).
    let typing = ui.ctx().text_edit_focused();
    // Only plain values are read inside `input`: it holds egui's lock, so asking the context
    // anything there (e.g. `layer_id_at`) waits for itself – the app froze and closed.
    let (hover, clicked, double, ctrl, enter, esc, ctrl_a) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_clicked(),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
            i.modifiers.ctrl || i.modifiers.command,
            !typing && i.key_pressed(egui::Key::Enter),
            !typing && i.key_pressed(egui::Key::Escape),
            !typing && i.modifiers.command && i.key_pressed(egui::Key::A),
        )
    });
    // The pointer counts only on the view itself – not on a window or menu above it. The view's own
    // response knows (a layer test here missed the view after a camera change: nothing selected).
    let hover = hover.filter(|p| area.contains(*p) && view.pointer_on_view);
    let tool = select_cmd::tool();
    let points_id = egui::Id::unique("factory_select_points");
    let (drawn_with, mut points): (Tool, Vec<[f32; 2]>) = ui.ctx().data(|d| d.get_temp(points_id)).unwrap_or_default();
    if drawn_with != tool {
        points.clear(); // another tool: start over
    }

    // T1 box / lasso: SolidCraft's own gesture, read back through the hook. Ctrl removes (C1).
    if tool == Tool::Pointer {
        for b in gestures {
            let (shape, crossing) = if b.lasso.is_empty() { (Area::Rect { min: b.min, max: b.max }, b.crossing) } else { (Area::Polygon(b.lasso), false) };
            selection::apply(selection::in_area(scope, &data, &shape, crossing, &project), ctrl);
        }
    }
    if let Some(at) = hover {
        let at = [at.x, at.y];
        if clicked {
            match tool {
                Tool::Pointer => {
                    // Also an empty click reports ("0 shells, 0 frames added"); the selection stays.
                    selection::apply(selection::at_point(scope, &data, at, &project, view.eye, 6.0), ctrl);
                }
                _ => {
                    if points.last().is_none_or(|p| (p[0] - at[0]).hypot(p[1] - at[1]) > 3.0) {
                        points.push(at);
                    }
                }
            }
        }
        if ctrl_a {
            selection::apply(selection::all(scope), false);
        }
    }
    // T2–T4: double-click or Enter finishes; Esc cancels the drawing, else back to the pointer,
    // else clears the selection (C3).
    let finish = (double || enter) && tool != Tool::Pointer;
    if finish {
        let enough = if tool == Tool::IntersectingLine { points.len() >= 2 } else { points.len() >= 3 };
        if enough {
            let found = match tool {
                Tool::Poly => selection::in_area(scope, &data, &Area::Polygon(points.clone()), false, &project),
                Tool::IntersectingPoly => selection::in_area(scope, &data, &Area::Polygon(points.clone()), true, &project),
                _ => selection::crossed_by_line(scope, &data, &points, &project),
            };
            selection::apply(found, ctrl);
        }
        points.clear();
    }
    if esc && hover.is_some() {
        if !points.is_empty() {
            points.clear();
        } else if tool != Tool::Pointer {
            select_cmd::set_tool(Tool::Pointer);
        } else {
            selection::clear();
        }
    }
    // The polygon / line being drawn, closed to the cursor.
    if tool != Tool::Pointer {
        let painter = ui.painter_at(area);
        let ink = egui::Stroke::new(1.5, egui::Color32::from_rgb(SELECTION_COLOUR[0], SELECTION_COLOUR[1], SELECTION_COLOUR[2]));
        let mut line: Vec<egui::Pos2> = points.iter().map(|p| egui::pos2(p[0], p[1])).collect();
        if let Some(h) = hover {
            line.push(h);
        }
        if tool != Tool::IntersectingLine && line.len() > 2 {
            line.push(line[0]);
        }
        painter.add(egui::Shape::line(line, ink));
        for p in &points {
            painter.circle_filled(egui::pos2(p[0], p[1]), 3.0, ink.color);
        }
        let hint = match tool {
            Tool::Poly => "Poly: click the corners · double-click / Enter: select inside · Esc: cancel",
            Tool::IntersectingPoly => "Intersecting Poly: click the corners · double-click / Enter: select what it touches · Esc: cancel",
            _ => "Intersecting Line: click the points · double-click / Enter: select what it crosses · Esc: cancel",
        };
        painter.text(area.left_top() + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(12.0), ui.visuals().text_color());
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(points_id, (tool, points)));
}

/// R6: the selected elements drawn as thick lines in the selection colour – rebuilt only when the
/// selection, the geometry or the View Options change.
fn highlight(g: &std::sync::Arc<transfer::Geometry>) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<(u64, usize, usize)>> = Mutex::new(None);
    let data = results_view::snap_data();
    let key = (selection::version(), std::sync::Arc::as_ptr(g) as usize, std::sync::Arc::as_ptr(&data) as usize);
    {
        let mut last = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *last == Some(key) {
            return;
        }
        *last = Some(key);
    }
    let set = selection::current();
    let mut segs: Vec<Segment> = Vec::new();
    let seg = |a: [f32; 3], b: [f32; 3]| Segment { ends: [a, b], values: [0.0, 0.0] };
    for e in set.shells.ones().filter(|e| *e < g.shells.len()) {
        let n = if g.shells[e][3] == u32::MAX { 3 } else { 4 };
        for f in results_view::drawn_faces(g, &data, e) {
            for k in 0..n {
                segs.push(seg(f[k], f[(k + 1) % n]));
            }
            // A cross over the shell, so a selected shell reads as selected from any side.
            if n == 4 {
                segs.push(seg(f[0], f[2]));
                segs.push(seg(f[1], f[3]));
            }
        }
    }
    for e in set.frames.ones().filter(|e| *e < g.frames.len()) {
        if let Some((a, b)) = selection::drawn_frame_ends(g, &data, e) {
            segs.push(seg(a.map(|v| v as f32), b.map(|v| v as f32)));
        }
    }
    // Bead width: a small fraction of the model's size (at least ~1.5 px on screen anyway).
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in &g.joints {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let diag = if g.joints.is_empty() { 1.0 } else { ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() };
    custom_bridge::view::show_selection(&segs, SELECTION_COLOUR, (diag * 0.0015) as f32);
}

/// The status bar's selection text (R6).
pub fn status_text() -> String {
    let (s, f) = selection::current().counts();
    let mut t = format!("Selected: {s} shells, {f} frames");
    if selection::deselect_mode() {
        t.push_str(" · DESELECT");
    }
    // The last action, so a click that found nothing is visible too.
    let m = selection::message();
    if !m.is_empty() {
        t.push_str(&format!(" · {m}"));
    }
    t
}

// ---- Dialogs (T5, T6, T7, T9, T10) ----------------------------------------------------------------

/// The dialog's inputs, kept while it is open.
#[derive(Clone)]
struct Inputs {
    axes: [bool; 3],
    ranges: [[f64; 2]; 3],
    tolerance: f64,
    parallel: usize,
    angle: f64,
    shell_sections: Vec<bool>,
    frame_sections: Vec<bool>,
    thickness_on: bool,
    thickness: [f64; 2],
    groups: Vec<bool>,
    group_name: String,
    labels: String,
}

impl Default for Inputs {
    fn default() -> Self {
        Inputs {
            axes: [false; 3],
            ranges: [[0.0; 2]; 3],
            tolerance: 0.001,
            parallel: 0,
            angle: 1.0,
            shell_sections: Vec::new(),
            frame_sections: Vec::new(),
            thickness_on: false,
            thickness: [0.0, 1.0],
            groups: Vec::new(),
            group_name: String::new(),
            labels: String::new(),
        }
    }
}

/// Draw the open dialog (called every frame with the other windows).
pub fn window(ctx: &egui::Context) {
    let Some(dialog) = select_cmd::dialog() else { return };
    let g = transfer::geometry();
    let id = egui::Id::unique("factory_select_inputs");
    let mut inp: Inputs = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let length = crate::model::read(|f| f.units.length);
    let (unit, to_m) = (length.symbol(), length.to_si());
    let title = match dialog {
        Dialog::Coordinates => "Select – Coordinate Specification",
        Dialog::Parallel => "Select – Lines Parallel To",
        Dialog::Properties => "Select – Properties",
        Dialog::Groups => "Select – Groups",
        Dialog::Labels => "Select – Labels",
    };
    let mut open = true;
    let mut action: Option<bool> = None; // Some(false) select, Some(true) deselect
    egui::Window::new(title).open(&mut open).resizable(true).default_width(380.0).show(ctx, |ui| {
        kinds_row(ui);
        ui.separator();
        match dialog {
            Dialog::Coordinates => {
                ui.label(format!("Elements whose every node lies in the ranges ({unit}). Equal min and max: a plane."));
                egui::Grid::new("factory_sel_coords").num_columns(4).show(ui, |ui| {
                    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.checkbox(&mut inp.axes[i], *axis);
                        ui.add_enabled(inp.axes[i], egui::DragValue::new(&mut inp.ranges[i][0]).speed(0.1).prefix("min "));
                        ui.add_enabled(inp.axes[i], egui::DragValue::new(&mut inp.ranges[i][1]).speed(0.1).prefix("max "));
                        ui.label(unit);
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Tolerance");
                    ui.add(egui::DragValue::new(&mut inp.tolerance).speed(0.001).range(0.0..=f64::MAX));
                    ui.label(unit);
                });
            }
            Dialog::Parallel => {
                ui.label("Frames parallel to:");
                for (k, name) in ["Global X", "Global Y", "Global Z", "Plane XY", "Plane XZ", "Plane YZ", "The first selected frame"].iter().enumerate() {
                    ui.radio_value(&mut inp.parallel, k, *name);
                }
                ui.horizontal(|ui| {
                    ui.label("Angle tolerance");
                    ui.add(egui::DragValue::new(&mut inp.angle).speed(0.1).range(0.0..=45.0).suffix("°"));
                });
            }
            Dialog::Properties => {
                inp.shell_sections.resize(g.shell_sections.len(), false);
                inp.frame_sections.resize(g.frame_sections.len(), false);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    ui.strong("Shell sections");
                    if g.shell_sections.is_empty() {
                        ui.weak("none transferred");
                    }
                    for (k, name) in g.shell_sections.iter().enumerate() {
                        let t = g.shell_thickness.get(k).map(|t| format!("  (t = {:.3} {unit})", t / to_m)).unwrap_or_default();
                        ui.checkbox(&mut inp.shell_sections[k], format!("{name}{t}"));
                    }
                    ui.strong("Frame sections");
                    if g.frame_sections.is_empty() {
                        ui.weak("none transferred");
                    }
                    for (k, name) in g.frame_sections.iter().enumerate() {
                        ui.checkbox(&mut inp.frame_sections[k], name.as_str());
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut inp.thickness_on, "Shell thickness from");
                    ui.add_enabled(inp.thickness_on, egui::DragValue::new(&mut inp.thickness[0]).speed(0.01));
                    ui.label("to");
                    ui.add_enabled(inp.thickness_on, egui::DragValue::new(&mut inp.thickness[1]).speed(0.01));
                    ui.label(unit);
                });
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "Material")).on_disabled_hover_text("The transfer does not give a material per section yet (docs/13 §7).");
            }
            Dialog::Groups => {
                let groups = crate::model::read(|f| f.model.groups.clone());
                inp.groups.resize(groups.len(), false);
                if groups.is_empty() {
                    ui.weak("No groups yet. CSiBridge's groups are not transferred yet; save a selection as a group below.");
                }
                for (k, (name, members)) in groups.iter().enumerate() {
                    ui.checkbox(&mut inp.groups[k], format!("{name}  ({} elements)", members.len()));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Save the selection as");
                    ui.text_edit_singleline(&mut inp.group_name);
                    if ui.button("Save group").clicked() {
                        select_cmd::save_selection_as_group(&inp.group_name);
                    }
                });
            }
            Dialog::Labels => {
                ui.label("Names: lists (A1, A5), ranges (101-150, A101-A150), wildcards (A1*, B?2).");
                ui.add(egui::TextEdit::multiline(&mut inp.labels).desired_rows(3).desired_width(f32::INFINITY));
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Select").clicked() {
                action = Some(false);
            }
            if ui.button("Deselect").clicked() {
                action = Some(true);
            }
            let m = selection::message();
            if !m.is_empty() {
                ui.weak(m);
            }
        });
    });
    if let Some(remove) = action {
        let found = select_cmd::with_scope(|s| run(dialog, s, &inp, to_m));
        if let Some(found) = found {
            selection::apply(found, remove);
        }
    }
    ctx.data_mut(|d| d.insert_temp(id, inp));
    if !open {
        select_cmd::set_dialog(None);
    }
}

/// What the dialog's inputs find.
fn run(dialog: Dialog, s: Scope<'_>, inp: &Inputs, to_m: f64) -> Option<selection::Found> {
    Some(match dialog {
        Dialog::Coordinates => {
            if !inp.axes.iter().any(|on| *on) {
                return None; // no axis ticked: nothing to select by (not the whole model)
            }
            let ranges = [0, 1, 2].map(|i| inp.axes[i].then(|| (inp.ranges[i][0] * to_m, inp.ranges[i][1] * to_m)));
            selection::in_coordinates(s, ranges, inp.tolerance * to_m)
        }
        Dialog::Parallel => {
            let to = match inp.parallel {
                0 => Parallel::To([1.0, 0.0, 0.0]),
                1 => Parallel::To([0.0, 1.0, 0.0]),
                2 => Parallel::To([0.0, 0.0, 1.0]),
                3 => Parallel::Plane([0.0, 0.0, 1.0]),
                4 => Parallel::Plane([0.0, 1.0, 0.0]),
                5 => Parallel::Plane([1.0, 0.0, 0.0]),
                _ => {
                    let first = selection::current().frames.ones().next()?;
                    let f = s.g.frames.get(first)?;
                    let (a, b) = (s.g.joints.get(f[0] as usize)?, s.g.joints.get(f[1] as usize)?);
                    Parallel::To([b[0] - a[0], b[1] - a[1], b[2] - a[2]])
                }
            };
            selection::parallel(s, to, inp.angle)
        }
        Dialog::Properties => {
            let picked = |v: &[bool]| v.iter().enumerate().filter(|(_, on)| **on).map(|(k, _)| k).collect::<Vec<_>>();
            let thickness = inp.thickness_on.then(|| (inp.thickness[0] * to_m, inp.thickness[1] * to_m));
            selection::with_properties(s, &picked(&inp.shell_sections), &picked(&inp.frame_sections), thickness)
        }
        Dialog::Groups => {
            let groups = crate::model::read(|f| f.model.groups.clone());
            let members: Vec<String> = groups.iter().zip(&inp.groups).filter(|(_, on)| **on).flat_map(|((_, m), _)| m.clone()).collect();
            selection::in_groups(s, &members)
        }
        Dialog::Labels => selection::with_labels(s, &inp.labels),
    })
}

/// R1: which kinds every tool takes (Joints follow when they are drawn).
fn kinds_row(ui: &mut egui::Ui) {
    let mut k = selection::kinds();
    let before = k;
    ui.horizontal(|ui| {
        ui.label("Select:");
        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Joints")).on_disabled_hover_text("Joints are not drawn yet (docs/13 §7).");
        ui.checkbox(&mut k.frames, "Frames");
        ui.checkbox(&mut k.shells, "Shells");
        let mut deselect = selection::deselect_mode();
        if ui.checkbox(&mut deselect, "Deselect mode").changed() {
            selection::set_deselect_mode(deselect);
        }
    });
    if k != before {
        selection::set_kinds(Kinds { ..k });
    }
}
