//! Cut Diagram on screen (docs/16): the mode's clicks (C1), the diagram over each display window
//! (C3), the Browser rows under Structure (C4), Properties (C5) and the table (C6). The numbers
//! come from [`crate::cut`].

use std::sync::{Arc, Mutex, PoisonError};

use custom_bridge::ui::egui;
use custom_bridge::ui::{tree_row, tree_row_eye};
use custom_bridge::view::ViewInfo;

use crate::cut::{self, CUT_COMPONENTS, Cut, Diagram, Direction};
use crate::results_view::{self, Choice, SnapData};
use crate::{model, transfer, vv};

/// The cut chosen in the Browser (drawn thicker).
static SELECTED: Mutex<Option<String>> = Mutex::new(None);
/// Properties: (the cut's name when opened, the working copy).
static PROPERTIES: Mutex<Option<(String, Cut)>> = Mutex::new(None);
/// The table window's cut.
static TABLE: Mutex<Option<String>> = Mutex::new(None);
/// Mode option: vertical cut plane (default, decided) or along the view direction.
static VERTICAL: Mutex<bool> = Mutex::new(true);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- the active window: mode and diagrams ----

/// Over the active window. `hit` is the value snap's point under the cursor (a joint within the
/// snap radius, else the point on the shell), used as the clicked point.
pub fn overlay(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, hit: Option<[f64; 3]>) {
    let s = vv::settings();
    draw_all(ui, area, view, &results_view::shown().choice, &results_view::snap_data(), &s);
    mode(ui, area, view, hit);
}

/// Over an inactive display window `tile` (docs/14): its own result and V/G.
pub fn overlay_tile(ui: &mut egui::Ui, tile: u64, area: egui::Rect, view: &ViewInfo<'_>) {
    let w = crate::windows::current();
    let Some(saved) = w.windows.iter().find(|x| x.id == tile).and_then(|x| x.saved.as_ref()) else { return };
    draw_all(ui, area, view, &saved.shown.choice, &saved.snap, &saved.settings);
}

fn draw_all(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, choice: &Choice, snap: &Arc<SnapData>, s: &vv::Settings) {
    let selected = lock(&SELECTED).clone();
    for c in cut::cuts().iter().filter(|c| !s.cuts_hidden.contains(&c.name)) {
        let sel = selected.as_deref() == Some(c.name.as_str());
        match cut::diagram_for(c, choice, snap) {
            Some(d) => match d.as_ref() {
                Ok(d) => draw(ui, area, view, c, d, s.cut_plain, sel),
                Err(e) => note(ui, area, view, c, e),
            },
            None => note(ui, area, view, c, "computing …"),
        }
    }
}

/// The mode (C1): two clicks make the cut; Esc leaves. A small box says what to do.
fn mode(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, hit: Option<[f64; 3]>) {
    let Some(m) = cut::mode() else { return };
    let typing = ui.ctx().text_edit_focused();
    let (hover, clicked, esc) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.primary_clicked(), !typing && i.key_pressed(egui::Key::Escape)));
    if esc {
        cut::set_mode(None);
        return;
    }
    let on_view = hover.filter(|p| area.contains(*p) && view.pointer_on_view);
    let painter = ui.painter_at(area);
    let orange = egui::Color32::from_rgb(240, 130, 30);
    if let Some(first) = m.first.and_then(|p| (view.project)(p)) {
        painter.rect_filled(egui::Rect::from_center_size(first, egui::vec2(7.0, 7.0)), 0.0, orange);
        let to = hit.and_then(|p| (view.project)(p)).or(on_view);
        if let Some(to) = to {
            painter.line_segment([first, to], egui::Stroke::new(2.0, orange));
        }
    }
    let mut vertical = *lock(&VERTICAL);
    egui::Area::new(egui::Id::unique("factory_cut_mode")).order(egui::Order::Foreground).fixed_pos(area.left_top() + egui::vec2(12.0, 12.0)).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.strong(match (&m.redraw, m.first) {
                (Some(n), None) => format!("Redraw {n}: click point 1"),
                (Some(n), Some(_)) => format!("Redraw {n}: click point 2"),
                (None, None) => "Cut Diagram: click point 1 on the model".into(),
                (None, Some(_)) => "Cut Diagram: click point 2".into(),
            });
            ui.checkbox(&mut vertical, "Vertical cut plane (Z)").on_hover_text("Off: the plane goes along the view direction");
            ui.weak("Snaps to joints within 10 px · Esc leaves");
        });
    });
    *lock(&VERTICAL) = vertical;
    if clicked && on_view.is_some() {
        if let Some(p) = hit {
            let dir = [p[0] - view.eye[0], p[1] - view.eye[1], p[2] - view.eye[2]];
            if let Some(name) = cut::click(p, vertical, dir) {
                *lock(&SELECTED) = Some(name);
            }
        }
    }
}

// ---- drawing (C3) ----

fn fmt(v: f64) -> String {
    let a = v.abs();
    if a >= 100.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

fn colour(v: f64, lo: f64, hi: f64, plain: bool, alpha: u8) -> egui::Color32 {
    let [r, g, b] = if plain {
        if v >= 0.0 { [40, 90, 220] } else { [220, 50, 40] }
    } else {
        let t = if hi > lo { ((v - lo) / (hi - lo)) as f32 } else { 0.5 };
        custom_bridge::view::ramp(t.clamp(0.0, 1.0))
    };
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// The screen direction values are drawn to: the cut plane's extrusion as seen, or – looking
/// along it (a vertical cut seen from above) – square to the cut line on the screen.
fn offset_dir(view: &ViewInfo<'_>, c: &Cut, f: &cut::Frame) -> Option<egui::Vec2> {
    let mid = [0, 1, 2].map(|i| (c.p1[i] + c.p2[i]) / 2.0);
    let to = [mid[0] - view.eye[0], mid[1] - view.eye[1], mid[2] - view.eye[2]];
    let l = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt().max(1e-12);
    let along = (to[0] * f.d[0] + to[1] * f.d[1] + to[2] * f.d[2]).abs() / l;
    let m = (view.project)(mid)?;
    if along < 0.9 {
        let k = f.length.max(1e-3) * 0.1;
        let tip = (view.project)([mid[0] + f.d[0] * k, mid[1] + f.d[1] * k, mid[2] + f.d[2] * k])?;
        let v = tip - m;
        if v.length() > 1.0 {
            return Some(v.normalized());
        }
    }
    let (a, b) = ((view.project)(c.p1)?, (view.project)(c.p2)?);
    let line = (b - a).normalized();
    let perp = egui::vec2(line.y, -line.x);
    Some(if perp.y > 0.0 { -perp } else { perp })
}

fn draw(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, c: &Cut, d: &Diagram, plain: bool, selected: bool) {
    let painter = ui.painter_at(area);
    let ink = ui.visuals().text_color();
    let (Some(a), Some(b)) = ((view.project)(c.p1), (view.project)(c.p2)) else { return };
    painter.line_segment([a, b], egui::Stroke::new(if selected { 3.0 } else { 1.5 }, ink.gamma_multiply(0.7)));
    let Some(f) = d.frame else { return };
    let Some(dir) = offset_dir(view, c, &f) else { return };
    let big = d.largest();
    let k = if big > 0.0 { f64::from(c.scale_px) / big } else { 0.0 };
    let (lo, hi) = (d.min().map(|s| s.value).unwrap_or(0.0), d.max().map(|s| s.value).unwrap_or(0.0));
    let mut mesh = egui::Mesh::default();
    let mut tri = |p: [egui::Pos2; 3], v: [f64; 3]| {
        let i = mesh.vertices.len() as u32;
        for (p, v) in p.iter().zip(v) {
            mesh.colored_vertex(*p, colour(v, lo, hi, plain, 150));
        }
        mesh.add_triangle(i, i + 1, i + 2);
    };
    let font = egui::FontId::proportional(11.0);
    let mut labels: Vec<(egui::Pos2, f64)> = Vec::new();
    for [s0, s1] in &d.pieces {
        let (Some(p0), Some(p1)) = ((view.project)(s0.at), (view.project)(s1.at)) else { continue };
        let (t0, t1) = (p0 + dir * (s0.value * k) as f32, p1 + dir * (s1.value * k) as f32);
        if c.fill {
            if s0.value * s1.value < 0.0 {
                let r = (s0.value / (s0.value - s1.value)) as f32;
                let z = p0 + (p1 - p0) * r;
                tri([p0, z, t0], [s0.value, 0.0, s0.value]);
                tri([z, p1, t1], [0.0, s1.value, s1.value]);
            } else {
                tri([p0, p1, t1], [s0.value, s1.value, s1.value]);
                tri([p0, t1, t0], [s0.value, s1.value, s0.value]);
            }
        }
        let edge = colour((s0.value + s1.value) / 2.0, lo, hi, plain, 255);
        painter.line(vec![p0, t0, t1, p1], egui::Stroke::new(1.2, edge));
        if c.text && c.all_values {
            labels.push((t0, s0.value));
            labels.push((t1, s1.value));
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    if c.text {
        let at = |st: &cut::Station| (view.project)(st.at).map(|p| p + dir * (st.value * k) as f32);
        let ends = [d.pieces.first().map(|p| &p[0]), d.pieces.last().map(|p| &p[1]), d.max(), d.min()];
        for st in ends.into_iter().flatten() {
            if let Some(p) = at(st) {
                labels.push((p, st.value));
            }
        }
        let mut placed: Vec<egui::Pos2> = Vec::new();
        for (p, v) in labels {
            if placed.iter().any(|q| q.distance(p) < 14.0) {
                continue;
            }
            placed.push(p);
            let p = p + dir * 8.0 * v.signum() as f32;
            painter.text(p, egui::Align2::CENTER_CENTER, fmt(v), font.clone(), ink);
        }
    }
    let comp = CUT_COMPONENTS.get(d.component).copied().unwrap_or("");
    let mut title = format!("{} · {comp} [{}]", c.name, d.unit);
    if let Some((r, u)) = &d.resultant {
        title.push_str(&format!("   ∫ = {} {u}", fmt(*r)));
    }
    painter.text(b + egui::vec2(6.0, 6.0), egui::Align2::LEFT_TOP, title, font.clone(), ink);
    for (i, n) in d.notes.iter().enumerate() {
        painter.text(b + egui::vec2(6.0, 20.0 + 13.0 * i as f32), egui::Align2::LEFT_TOP, n, font.clone(), ui.visuals().weak_text_color());
    }
}

fn note(ui: &mut egui::Ui, area: egui::Rect, view: &ViewInfo<'_>, c: &Cut, text: &str) {
    let (Some(a), Some(b)) = ((view.project)(c.p1), (view.project)(c.p2)) else { return };
    let painter = ui.painter_at(area);
    let ink = ui.visuals().text_color();
    painter.line_segment([a, b], egui::Stroke::new(1.5, ink.gamma_multiply(0.7)));
    painter.text(b + egui::vec2(6.0, 6.0), egui::Align2::LEFT_TOP, format!("{}: {text}", c.name), egui::FontId::proportional(11.0), ui.visuals().weak_text_color());
}

// ---- Browser (C4) ----

#[derive(Clone, Debug, PartialEq)]
enum Menu {
    Folder,
    Cut(String),
}

fn open_id() -> egui::Id {
    egui::Id::unique("factory_cut_rows_open")
}

fn menu_id() -> egui::Id {
    egui::Id::unique("factory_cut_menu")
}

/// Structure › Cut Diagrams: one row per cut with an eye (this view shows it); right-click menus.
pub fn rows(ui: &mut egui::Ui, depth: usize) {
    let list = cut::cuts();
    let open = ui.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    let r = tree_row(ui, "cut_diagrams", depth, (!list.is_empty()).then_some(open), "line", "Cut Diagrams", &list.len().to_string());
    if r.toggled || (r.clicked && !list.is_empty()) {
        ui.data_mut(|d| d.insert_temp(open_id(), !open));
    }
    if let Some(at) = r.context_at {
        ui.data_mut(|d| d.insert_temp(menu_id(), (at, Menu::Folder)));
    }
    if open && !r.toggled {
        let selected = lock(&SELECTED).clone();
        for c in &list {
            let shown = cut::shown_in_view(&c.name);
            let comp = c.component.and_then(|k| CUT_COMPONENTS.get(k).copied()).unwrap_or("window");
            let label = if selected.as_deref() == Some(c.name.as_str()) { format!("▸ {}", c.name) } else { c.name.clone() };
            let rr = tree_row_eye(ui, &format!("cut_diagrams/{}", c.name), depth + 1, None, "line", &label, comp, shown);
            if rr.eye {
                cut::show_in_view(&c.name, !shown);
            } else if rr.clicked {
                *lock(&SELECTED) = Some(c.name.clone());
                results_view::repaint();
            }
            if let Some(at) = rr.context_at {
                ui.data_mut(|d| d.insert_temp(menu_id(), (at, Menu::Cut(c.name.clone()))));
            }
        }
    }
    menu(ui);
}

fn menu(ui: &mut egui::Ui) {
    let Some((at, menu)) = ui.data(|d| d.get_temp::<(egui::Pos2, Menu)>(menu_id())) else { return };
    let ctx = ui.ctx().clone();
    let mut done = false;
    let area = egui::Area::new(egui::Id::unique("factory_cut_menu_area")).order(egui::Order::Foreground).fixed_pos(at).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| match &menu {
            Menu::Folder => {
                if ui.button("New Cut Diagram").clicked() {
                    cut::set_mode(Some(cut::Mode::default()));
                    done = true;
                }
            }
            Menu::Cut(name) => {
                let shown = cut::shown_in_view(name);
                if ui.button(if shown { "Hide" } else { "Show" }).clicked() {
                    cut::show_in_view(name, !shown);
                    done = true;
                }
                let rename = ui.button("Rename…").clicked();
                if ui.button("Properties…").clicked() || rename {
                    open_properties(name);
                    done = true;
                }
                if ui.button("Redraw").on_hover_text("Click two new points").clicked() {
                    cut::set_mode(Some(cut::Mode { redraw: Some(name.clone()), first: None }));
                    done = true;
                }
                if ui.button("Table…").clicked() {
                    *lock(&TABLE) = Some(name.clone());
                    done = true;
                }
                ui.separator();
                if ui.button("Delete").clicked() {
                    cut::delete(name);
                    done = true;
                }
            }
        });
    });
    let elsewhere = ctx.input(|i| i.pointer.primary_clicked()) && !area.response.contains_pointer();
    if done || elsewhere || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.remove::<(egui::Pos2, Menu)>(menu_id()));
    }
}

fn open_properties(name: &str) {
    if let Some(c) = cut::get(name) {
        *lock(&PROPERTIES) = Some((name.to_string(), c));
    }
}

// ---- Properties (C5) and Table (C6) ----

/// The cut windows; called every frame (from the status bar, as the Browser's tables).
pub fn windows(ctx: &egui::Context) {
    properties(ctx);
    table_window(ctx);
}

fn point_row(ui: &mut egui::Ui, label: &str, p: &mut [f64; 3], scale: f64, unit: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        for v in p.iter_mut() {
            let mut shown = *v / scale;
            if ui.add(egui::DragValue::new(&mut shown).speed(0.01).suffix(format!(" {unit}"))).changed() {
                *v = shown * scale;
            }
        }
    });
    ui.end_row();
}

fn properties(ctx: &egui::Context) {
    let Some((name, mut work)) = lock(&PROPERTIES).clone() else { return };
    let units = model::read(|f| f.units);
    let (scale, unit) = (units.length.to_si(), units.length.symbol());
    let mut open = true;
    let mut close = false;
    let mut error = None;
    egui::Window::new(format!("Cut Diagram – {name}")).id(egui::Id::unique("factory_cut_properties")).open(&mut open).resizable(false).show(ctx, |ui| {
        egui::Grid::new("factory_cut_props").num_columns(2).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut work.name);
            ui.end_row();
            point_row(ui, "Point 1", &mut work.p1, scale, unit);
            point_row(ui, "Point 2", &mut work.p2, scale, unit);
            ui.label("Cut plane");
            ui.horizontal(|ui| {
                let mut vertical = work.direction == Direction::Vertical;
                ui.radio_value(&mut vertical, true, "Vertical (Z)");
                ui.radio_value(&mut vertical, false, "Along");
                let mut d = match work.direction {
                    Direction::Along(d) => d,
                    Direction::Vertical => [0.0, 0.0, 1.0],
                };
                ui.add_enabled_ui(!vertical, |ui| {
                    for v in d.iter_mut() {
                        ui.add(egui::DragValue::new(v).speed(0.01));
                    }
                });
                work.direction = if vertical { Direction::Vertical } else { Direction::Along(d) };
            });
            ui.end_row();
            ui.label("Component");
            let current = work.component.and_then(|k| CUT_COMPONENTS.get(k).copied()).unwrap_or("Window's component");
            egui::ComboBox::from_id_salt("factory_cut_component").selected_text(current).show_ui(ui, |ui| {
                ui.selectable_value(&mut work.component, None, "Window's component");
                for (k, n) in CUT_COMPONENTS.iter().enumerate() {
                    let hover = if k >= cut::CUT_AXES { "Normal / tangential to the cut (element local axes rotated, docs/16 R4)" } else { "Element local axes, as the contour" };
                    ui.selectable_value(&mut work.component, Some(k), *n).on_hover_text(hover);
                }
            });
            ui.end_row();
            ui.label("Scale");
            ui.add(egui::Slider::new(&mut work.scale_px, 10.0..=300.0).suffix(" px for the largest value"));
            ui.end_row();
            ui.label("Show");
            ui.horizontal(|ui| {
                ui.checkbox(&mut work.text, "Values");
                ui.add_enabled(work.text, egui::Checkbox::new(&mut work.all_values, "At every element edge"));
                ui.checkbox(&mut work.fill, "Fill");
            });
            ui.end_row();
        });
        ui.weak("Colours: Visibility/Graphics › View Options › Cut Diagram.");
        ui.separator();
        ui.horizontal(|ui| {
            let mut commit = |w: &Cut| {
                if cut::replace(&name, w.clone()) {
                    true
                } else {
                    error = Some("Not changed: the name is empty or taken, or the points make no cut.");
                    false
                }
            };
            if ui.button("OK").clicked() && commit(&work) {
                close = true;
            }
            if ui.button("Apply").clicked() && commit(&work) {
                if let Some(s) = lock(&SELECTED).as_mut().filter(|s| **s == name) {
                    *s = work.name.clone();
                }
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
        if let Some(e) = error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    });
    *lock(&PROPERTIES) = if close || !open {
        None
    } else {
        // after Apply with a new name, keep editing under it
        let key = if cut::get(&name).is_none() && cut::get(&work.name).is_some() { work.name.clone() } else { name };
        Some((key, work))
    };
}

fn table_window(ctx: &egui::Context) {
    let Some(name) = lock(&TABLE).clone() else { return };
    let Some(c) = cut::get(&name) else {
        *lock(&TABLE) = None;
        return;
    };
    let g = transfer::geometry();
    let units = model::read(|f| f.units);
    let d = cut::diagram_for(&c, &results_view::shown().choice, &results_view::snap_data());
    let mut open = true;
    egui::Window::new(format!("Cut Diagram Table – {name}")).id(egui::Id::unique("factory_cut_table")).open(&mut open).resizable(true).default_width(560.0).show(ctx, |ui| {
        let Some(d) = d else {
            ui.label("Computing …");
            return;
        };
        let d = match d.as_ref() {
            Ok(d) => d,
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return;
            }
        };
        ui.horizontal(|ui| {
            if ui.button("Copy").on_hover_text("Tab-separated, for Excel").clicked() {
                ui.ctx().copy_text(cut::table_text(&g, d, units, '\t'));
            }
            if ui.button("Copy CSV").clicked() {
                ui.ctx().copy_text(cut::table_text(&g, d, units, ','));
            }
            if ui.button("Save CSV").on_hover_text("Documents\\Factory\\<cut>.csv").clicked() {
                let msg = save_csv(&name, &cut::table_text(&g, d, units, ','));
                ui.ctx().data_mut(|m| m.insert_temp(egui::Id::unique("factory_cut_saved"), msg));
            }
        });
        if let Some(msg) = ui.ctx().data(|m| m.get_temp::<String>(egui::Id::unique("factory_cut_saved"))) {
            ui.weak(msg);
        }
        let len = units.length.symbol();
        let comp = CUT_COMPONENTS.get(d.component).copied().unwrap_or("");
        if let (Some(hi), Some(lo)) = (d.max(), d.min()) {
            ui.label(format!("{comp} [{}]   max {}   min {}", d.unit, fmt(hi.value), fmt(lo.value)));
        }
        if let Some((r, u)) = &d.resultant {
            ui.strong(format!("Resultant ∫ {comp} ds = {} {u}", fmt(*r)));
        }
        for n in &d.notes {
            ui.weak(n);
        }
        ui.separator();
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            egui::Grid::new("factory_cut_table_grid").striped(true).num_columns(6).show(ui, |ui| {
                for h in [format!("Station [{len}]"), format!("X [{len}]"), format!("Y [{len}]"), format!("Z [{len}]"), "Element".into(), format!("{comp} [{}]", d.unit)] {
                    ui.strong(h);
                }
                ui.end_row();
                for (s, at, e, v) in cut::table(&g, d, units) {
                    ui.label(format!("{s:.3}"));
                    for x in at {
                        ui.label(format!("{x:.3}"));
                    }
                    ui.label(e);
                    ui.label(fmt(v));
                    ui.end_row();
                }
            });
        });
    });
    if !open {
        *lock(&TABLE) = None;
    }
}

fn save_csv(name: &str, text: &str) -> String {
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else { return "no user folder".into() };
    let dir = std::path::PathBuf::from(home).join("Documents").join("Factory");
    let file: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let path = dir.join(format!("{file}.csv"));
    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text)) {
        Ok(()) => format!("Saved: {}", path.display()),
        Err(e) => format!("Not saved: {e}"),
    }
}
