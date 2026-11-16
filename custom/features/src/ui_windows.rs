//! Display windows on screen (docs/14): each window's title bar and frame, the borders between
//! windows (§10.2), the layout picker (§1.1, keys §10.3) and the View Templates dialog (§10.1).
//! The pictures themselves are drawn by SolidCraft (`custom_tiles`): the active window is the
//! ordinary 3D view, the others are drawn from the same GPU data with their own camera.

use std::sync::{Mutex, PoisonError};

use custom_bridge::ui::egui;
use custom_bridge::view::{TilePlan, ViewInfo};

use crate::windows::{self, Layout, Windows};

/// Height of a window's title bar (points).
const BAR: f32 = 20.0;
/// How long the pointer rests on a maximise button before the picker opens (seconds).
const HOVER_OPEN: f64 = 0.45;
/// Width of the grip on a border between windows (points).
const GRIP: f32 = 6.0;
/// Thumbnails per row in the picker.
const PER_ROW: usize = 3;

/// The open layout picker: for which window, where, whether hovering opened it, and the zone the
/// keys marked (layout index, zone).
#[derive(Clone, Copy)]
struct Picker {
    tile: u64,
    at: egui::Pos2,
    by_hover: bool,
    focus: Option<(usize, usize)>,
}

static PICKER: Mutex<Option<Picker>> = Mutex::new(None);

fn picker() -> Option<Picker> {
    *PICKER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn set_picker(p: Option<Picker>) {
    *PICKER.lock().unwrap_or_else(PoisonError::into_inner) = p;
}

/// The view area: frames and title bars of the windows, the borders, the layout picker; where each
/// picture goes (`None` = one window, the ordinary view).
pub fn layout(ui: &mut egui::Ui, full: egui::Rect) -> Option<TilePlan> {
    windows::keep_up(); // the layout is kept between sessions (§10.4)
    windows::sync(); // a view's template follows what is changed in it (docs/15 N6)
    rename_window(ui.ctx());
    named_view_menu(ui.ctx());
    let w = windows::current();
    if windows::take_picker_request() {
        set_picker(Some(Picker { tile: w.active, at: full.left_top() + egui::vec2(8.0, 8.0), by_hover: false, focus: None }));
    }
    if windows::take_templates_request() {
        crate::ui_panels::open_template_editor(None, None);
    }
    draw_picker(ui.ctx(), &w);
    if w.windows.len() == 1 {
        return None;
    }
    let accent = ui.visuals().selection.bg_fill;
    let faint = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let bar_fill = ui.visuals().faint_bg_color;
    let text = ui.visuals().text_color();
    let now = ui.input(|i| i.time);
    let zones = w.layout.rects_with(w.splits);
    let mut tiles = Vec::new();
    for (k, win) in w.windows.iter().enumerate() {
        let zone = match w.maximised {
            Some(m) if m == win.id => full,
            Some(_) => {
                // Not shown while another is maximised; it keeps its camera.
                tiles.push((win.id, egui::Rect::from_min_size(full.min, egui::Vec2::ZERO)));
                continue;
            }
            None => {
                let [l, t, r, b] = zones.get(k).copied().unwrap_or([0.0, 0.0, 1.0, 1.0]);
                egui::Rect::from_min_max(full.lerp_inside(egui::vec2(l, t)), full.lerp_inside(egui::vec2(r, b)))
            }
        }
        .shrink(1.0);
        let active = win.id == w.active;
        let bar = egui::Rect::from_min_max(zone.min, egui::pos2(zone.max.x, zone.min.y + BAR));
        let content = egui::Rect::from_min_max(egui::pos2(zone.min.x, bar.max.y), zone.max);
        let painter = ui.painter().clone();
        painter.rect_filled(bar, 0.0, if active { accent.gamma_multiply(0.35) } else { bar_fill });
        // Buttons: maximise / restore, close.
        let close = egui::Rect::from_center_size(egui::pos2(bar.max.x - BAR / 2.0, bar.center().y), egui::vec2(BAR - 4.0, BAR - 4.0));
        let max = close.translate(egui::vec2(-BAR, 0.0));
        let title_rect = egui::Rect::from_min_max(bar.min + egui::vec2(6.0, 0.0), egui::pos2(max.min.x - 4.0, bar.max.y));
        let title = windows::title(&w, win.id);
        let galley = painter.layout_no_wrap(title.clone(), egui::FontId::proportional(12.0), text);
        painter.with_clip_rect(title_rect).galley(egui::pos2(title_rect.min.x, bar.center().y - galley.size().y / 2.0), galley, text);
        let bar_resp = ui.interact(bar, egui::Id::unique(("factory_window_bar", win.id)), egui::Sense::click()).on_hover_text(title);
        let max_resp = ui.interact(max, egui::Id::unique(("factory_window_max", win.id)), egui::Sense::click());
        let close_resp = ui.interact(close, egui::Id::unique(("factory_window_close", win.id)), egui::Sense::click());
        for (r, resp) in [(max, &max_resp), (close, &close_resp)] {
            if resp.hovered() {
                painter.rect_filled(r, 3.0, ui.visuals().widgets.hovered.bg_fill);
            }
        }
        let stroke = egui::Stroke::new(1.2, text);
        if w.maximised == Some(win.id) {
            // Restore: two overlapping squares.
            painter.rect_stroke(max.shrink(5.0).translate(egui::vec2(1.5, -1.5)), 0.0, stroke, egui::StrokeKind::Middle);
            painter.rect_stroke(max.shrink(5.0).translate(egui::vec2(-1.5, 1.5)), 0.0, stroke, egui::StrokeKind::Middle);
        } else {
            painter.rect_stroke(max.shrink(4.5), 0.0, stroke, egui::StrokeKind::Middle);
        }
        let c = close.shrink(5.0);
        painter.line_segment([c.left_top(), c.right_bottom()], stroke);
        painter.line_segment([c.right_top(), c.left_bottom()], stroke);
        // Resting on the maximise button opens the layout picker (docs/14 §1.1).
        let hover_id = egui::Id::unique(("factory_window_max_hover", win.id));
        if max_resp.hovered() {
            let since = ui.data(|d| d.get_temp::<f64>(hover_id)).unwrap_or(now);
            ui.data_mut(|d| d.insert_temp(hover_id, since));
            if now - since >= HOVER_OPEN && picker().is_none_or(|p| p.tile != win.id) {
                set_picker(Some(Picker { tile: win.id, at: max.left_bottom() + egui::vec2(-150.0, 2.0), by_hover: true, focus: None }));
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ui.data_mut(|d| d.remove::<f64>(hover_id));
        }
        if max_resp.clicked() || bar_resp.double_clicked() {
            windows::toggle_maximise(win.id);
        } else if close_resp.clicked() {
            windows::close(win.id);
        } else if bar_resp.clicked() && !active {
            windows::activate(win.id);
        }
        // Right-click on the title bar: view templates (§10.1, VT3).
        let id = win.id;
        bar_resp.context_menu(|ui| {
            view_menu_items(ui, id);
            ui.separator();
            if ui.button("New template from this view…").clicked() {
                crate::ui_panels::open_template_editor(None, Some(id));
                ui.close();
            }
            if ui.button("View Templates…").clicked() {
                crate::ui_panels::open_template_editor(None, Some(id));
                ui.close();
            }
        });
        // A click in an inactive window makes it active (no tool acts there).
        if !active {
            let resp = ui.interact(content, egui::Id::unique(("factory_window_content", win.id)), egui::Sense::click_and_drag());
            if resp.clicked() || resp.drag_started() {
                windows::activate(win.id);
            }
        }
        painter.rect_stroke(zone, 0.0, if active { egui::Stroke::new(2.0, accent) } else { egui::Stroke::new(1.0, faint) }, egui::StrokeKind::Inside);
        tiles.push((win.id, content.shrink(if active { 2.0 } else { 1.0 })));
    }
    if w.maximised.is_none() {
        borders(ui, full, &w);
    }
    Some(TilePlan { tiles, active: w.active })
}

/// The borders between windows: dragged to resize, double-click makes the windows equal (§10.2).
fn borders(ui: &mut egui::Ui, full: egui::Rect, w: &Windows) {
    let (vertical, across, right_only) = w.layout.borders();
    // Where the borders are drawn (kept within 15 %–85 % as the zones are).
    let rects = w.layout.rects_with(w.splits);
    let x = rects.first().map_or(0.5, |r| r[2]);
    let y = match w.layout {
        Layout::Four => rects.first().map_or(0.5, |r| r[3]),
        Layout::OneTwo => rects.get(1).map_or(0.5, |r| r[3]),
        _ => 0.5,
    };
    let at_x = full.left() + x * full.width();
    let at_y = full.top() + y * full.height();
    let mut grips = Vec::new();
    if vertical {
        grips.push((0, egui::Rect::from_min_max(egui::pos2(at_x - GRIP / 2.0, full.top()), egui::pos2(at_x + GRIP / 2.0, full.bottom()))));
    }
    if across || right_only {
        let left = if across { full.left() } else { at_x + GRIP / 2.0 };
        grips.push((1, egui::Rect::from_min_max(egui::pos2(left, at_y - GRIP / 2.0), egui::pos2(full.right(), at_y + GRIP / 2.0))));
    }
    for (axis, rect) in grips {
        let resp = ui.interact(rect, egui::Id::unique(("factory_window_border", axis)), egui::Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if axis == 0 { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
            ui.painter().rect_filled(rect.shrink(1.5), 1.0, ui.visuals().selection.bg_fill.gamma_multiply(0.6));
        }
        if resp.double_clicked() {
            windows::set_splits([0.5, 0.5]);
        } else if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let mut s = w.splits;
            if axis == 0 {
                s[0] = (p.x - full.left()) / full.width().max(1.0);
            } else {
                s[1] = (p.y - full.top()) / full.height().max(1.0);
            }
            windows::set_splits(s);
        }
    }
}

/// Over an inactive window: its own legend, and whether its result is still being built.
pub fn overlay(ui: &mut egui::Ui, tile: u64, area: egui::Rect, _view: &ViewInfo<'_>) {
    let w = windows::current();
    let Some(saved) = w.windows.iter().find(|x| x.id == tile).and_then(|x| x.saved.as_ref()) else { return };
    if saved.settings.legend {
        crate::ui_results::legend_of(ui, area, &saved.shown);
    }
    let painter = ui.painter_at(area);
    let note = if saved.shown.busy {
        Some(("building the result …".to_string(), ui.visuals().text_color()))
    } else {
        saved.shown.error.clone().map(|e| (e, ui.visuals().error_fg_color))
    };
    if let Some((note, colour)) = note {
        painter.text(area.left_bottom() + egui::vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, note, egui::FontId::proportional(12.0), colour);
    }
}

/// Arrow keys in the picker (§10.3): ← / → through the zones of all layouts, ↑ / ↓ by a row.
fn moved(focus: Option<(usize, usize)>, w: &Windows, key: egui::Key) -> (usize, usize) {
    let all = Layout::ALL;
    let start = all.iter().position(|l| *l == w.layout).unwrap_or(0);
    let Some((li, zone)) = focus else { return (start, 0) };
    let zones = |li: usize| all[li].zones();
    match key {
        egui::Key::ArrowRight if zone + 1 < zones(li) => (li, zone + 1),
        egui::Key::ArrowRight => ((li + 1) % all.len(), 0),
        egui::Key::ArrowLeft if zone > 0 => (li, zone - 1),
        egui::Key::ArrowLeft => {
            let prev = (li + all.len() - 1) % all.len();
            (prev, zones(prev) - 1)
        }
        egui::Key::ArrowDown => {
            let next = (li + PER_ROW).min(all.len() - 1);
            (next, zone.min(zones(next) - 1))
        }
        egui::Key::ArrowUp => {
            let prev = li.saturating_sub(PER_ROW);
            (prev, zone.min(zones(prev) - 1))
        }
        _ => (li, zone),
    }
}

/// The layout picker (docs/14 §1.1): the current arrangement (P2), then the layouts L5, L1–L4.
fn draw_picker(ctx: &egui::Context, w: &Windows) {
    let Some(mut p) = picker() else { return };
    if w.index_of(p.tile).is_none() || ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        set_picker(None);
        return;
    }
    // Keys (§10.3): taken here, so the 3D view does not get them.
    for key in [egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::ArrowUp, egui::Key::ArrowDown] {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
            p.focus = Some(moved(p.focus, w, key));
            p.by_hover = false; // the keys keep it open
        }
    }
    if let Some((li, zone)) = p.focus
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    {
        windows::set_layout(Layout::ALL[li], Some((p.tile, zone)));
        set_picker(None);
        return;
    }
    set_picker(Some(p));
    let mut chosen: Option<Box<dyn FnOnce()>> = None;
    let area = egui::Area::new(egui::Id::unique("factory_layout_picker")).order(egui::Order::Foreground).fixed_pos(p.at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            if w.windows.len() > 1 && w.maximised.is_none() {
                ui.label(egui::RichText::new("Current").small());
                if let Some(zone) = thumbnail(ui, w.layout, None, |k| w.windows.get(k).map(|x| w.number(x.id).to_string())) {
                    if let Some(other) = w.windows.get(zone) {
                        let (a, b) = (p.tile, other.id);
                        chosen = Some(Box::new(move || windows::swap(a, b)));
                    }
                }
                ui.separator();
            }
            ui.label(egui::RichText::new("Layout  (← → ↑ ↓, Enter)").small());
            for (row, layouts) in Layout::ALL.chunks(PER_ROW).enumerate() {
                ui.horizontal(|ui| {
                    for (k, &layout) in layouts.iter().enumerate() {
                        let li = row * PER_ROW + k;
                        let marked = p.focus.filter(|(f, _)| *f == li).map(|(_, z)| z);
                        if let Some(zone) = thumbnail(ui, layout, marked, |_| None) {
                            let tile = p.tile;
                            chosen = Some(Box::new(move || windows::set_layout(layout, Some((tile, zone)))));
                        }
                    }
                });
            }
        });
    });
    if let Some(f) = chosen {
        f();
        set_picker(None);
        return;
    }
    // Closes on a click elsewhere, or – opened by hovering – when the pointer goes away.
    let resp = &area.response;
    let away = p.by_hover && ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| !resp.rect.expand(40.0).contains(pos));
    if resp.clicked_elsewhere() || away {
        set_picker(None);
    }
}

/// One layout drawn as its zones; the zone under the pointer (or `marked` by the keys) lights in
/// the accent colour. Returns the zone clicked.
fn thumbnail(ui: &mut egui::Ui, layout: Layout, marked: Option<usize>, label: impl Fn(usize) -> Option<String>) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(66.0, 44.0), egui::Sense::hover());
    let accent = ui.visuals().selection.bg_fill;
    let idle = ui.visuals().widgets.inactive.bg_fill;
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    let mut clicked = None;
    for (k, [l, t, r, b]) in layout.rects().into_iter().enumerate() {
        let zone = egui::Rect::from_min_max(rect.lerp_inside(egui::vec2(l, t)), rect.lerp_inside(egui::vec2(r, b))).shrink(1.5);
        let resp = ui.interact(zone, ui.scope_id().with(("factory_zone", layout.key(), k)), egui::Sense::click());
        let fill = if resp.hovered() || marked == Some(k) { accent } else { idle };
        ui.painter().rect(zone, 3.0, fill, stroke, egui::StrokeKind::Inside);
        if let Some(text) = label(k) {
            ui.painter().text(zone.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(11.0), ui.visuals().text_color());
        }
        if resp.clicked() {
            clicked = Some(k);
        }
    }
    clicked
}

/// The View Templates editor (docs/15 §9), drawn every frame while open.
pub fn templates_window(ctx: &egui::Context) {
    crate::ui_panels::template_editor(ctx);
}

// ---- Named views (docs/15) ----

/// The window and view entries of a right-click menu: open a view here, the view's template.
fn view_menu_items(ui: &mut egui::Ui, tile: u64) {
    let w = windows::current();
    let current = w.view_of(tile).cloned();
    ui.menu_button("Open view", |ui| {
        for v in &w.views {
            let here = current.as_ref().is_some_and(|c| c.name == v.name);
            if ui.add_enabled(!here, egui::Button::new(&v.name)).clicked() {
                windows::open_view(&v.name, tile);
                ui.close();
            }
        }
    });
    if let Some(v) = &current {
        ui.menu_button("View Template", |ui| template_choice(ui, &v.name, v.template.as_deref()));
        if ui.button("Rename view…").clicked() {
            start_rename(Rename::View(v.name.clone()));
            ui.close();
        }
    }
}

/// `<None>` and every template, the view's own marked; a click links it (N3).
fn template_choice(ui: &mut egui::Ui, view: &str, now: Option<&str>) {
    if ui.selectable_label(now.is_none(), "<None>").clicked() {
        windows::set_view_template(view, None);
        ui.close();
    }
    for t in windows::templates() {
        if ui.selectable_label(now == Some(t.name.as_str()), format!("{}   ({})", t.name, t.include.summary())).clicked() {
            windows::set_view_template(view, Some(&t.name));
            ui.close();
        }
    }
}

/// What a right-click in Named Views opened a menu for.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    View(String),
    Template(String),
}

static MENU: Mutex<Option<(egui::Pos2, Target)>> = Mutex::new(None);

/// Factory's rows inside SolidCraft's Named Views folder (N1): every view (the template it uses at
/// the end; a click opens it in the active window), then **View Templates** with every template
/// (the parts it rules at the end).
pub fn named_view_rows(ui: &mut egui::Ui, depth: usize) {
    let w = windows::current();
    for v in &w.views {
        let shown = w.window_of(&v.name).map(|id| format!(" · window {}", w.number(id))).unwrap_or_default();
        let count = format!("{}{shown}", v.template.as_deref().unwrap_or(""));
        let r = custom_bridge::ui::tree_row(ui, &format!("factory_view:{}", v.name), depth, None, "perspective", &v.name, &count);
        if r.clicked {
            windows::open_view(&v.name, w.active);
        }
        if let Some(at) = r.context_at {
            *MENU.lock().unwrap_or_else(PoisonError::into_inner) = Some((at, Target::View(v.name.clone())));
        }
    }
    let key = "factory_view_templates";
    let open_id = egui::Id::unique(("factory_browser_open", key));
    let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let list = windows::templates();
    let r = custom_bridge::ui::tree_row(ui, key, depth, Some(open), "folder", "View Templates", &list.len().to_string());
    let open = if r.toggled || r.clicked { !open } else { open };
    ui.data_mut(|d| d.insert_temp(open_id, open));
    if open {
        for t in &list {
            let r = custom_bridge::ui::tree_row(ui, &format!("factory_template:{}", t.name), depth + 1, None, "params", &t.name, &t.include.summary());
            if r.clicked {
                crate::ui_panels::open_template_editor(Some(t.name.clone()), None);
            }
            if let Some(at) = r.context_at {
                *MENU.lock().unwrap_or_else(PoisonError::into_inner) = Some((at, Target::Template(t.name.clone())));
            }
        }
    }
}

/// The right-click menu of a view or a template row in Named Views.
fn named_view_menu(ctx: &egui::Context) {
    let Some((at, target)) = MENU.lock().unwrap_or_else(PoisonError::into_inner).clone() else { return };
    let w = windows::current();
    let mut done = false;
    let area = egui::Area::new(egui::Id::unique("factory_named_view_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| match &target {
            Target::View(name) => {
                if ui.button("Open").clicked() {
                    windows::open_view(name, w.active);
                    done = true;
                }
                if let Some(v) = w.views.iter().find(|v| &v.name == name) {
                    ui.menu_button("View Template", |ui| template_choice(ui, name, v.template.as_deref()));
                }
                if ui.button("Rename…").clicked() {
                    start_rename(Rename::View(name.clone()));
                    done = true;
                }
                if ui.button("Duplicate").clicked() {
                    windows::duplicate_view(name);
                    done = true;
                }
                let shown = w.window_of(name).is_some();
                if ui.add_enabled(!shown, egui::Button::new("Delete")).on_disabled_hover_text("A window shows it: open another view there first").clicked() {
                    windows::delete_view(name);
                    done = true;
                }
            }
            Target::Template(name) => {
                if ui.button("Edit…").clicked() {
                    crate::ui_panels::open_template_editor(Some(name.clone()), None);
                    done = true;
                }
                if ui.button("Rename…").clicked() {
                    start_rename(Rename::Template(name.clone()));
                    done = true;
                }
                if ui.button("Delete").on_hover_text("Its views keep what it gave them").clicked() {
                    windows::delete_template(name);
                    done = true;
                }
            }
        });
    });
    if done || area.response.clicked_elsewhere() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *MENU.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// What the Rename dialog renames, and the text typed.
#[derive(Clone, Debug, PartialEq)]
enum Rename {
    View(String),
    Template(String),
}

static RENAME: Mutex<Option<(Rename, String)>> = Mutex::new(None);

fn start_rename(what: Rename) {
    let text = match &what {
        Rename::View(n) | Rename::Template(n) => n.clone(),
    };
    *RENAME.lock().unwrap_or_else(PoisonError::into_inner) = Some((what, text));
}

/// A small dialog with the new name (names are unique, docs/15 R7).
fn rename_window(ctx: &egui::Context) {
    let Some((what, mut text)) = RENAME.lock().unwrap_or_else(PoisonError::into_inner).clone() else { return };
    let (title, old) = match &what {
        Rename::View(n) => ("Rename View", n.clone()),
        Rename::Template(n) => ("Rename View Template", n.clone()),
    };
    let mut open = true;
    let mut finished = false;
    let mut failed = false;
    egui::Window::new(title).id(egui::Id::unique("factory_rename")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let resp = ui.text_edit_singleline(&mut text);
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("OK").clicked() || enter {
            let ok = match &what {
                Rename::View(_) => windows::rename_view(&old, &text),
                Rename::Template(_) => windows::rename_template(&old, &text),
            };
            finished = ok || text.trim() == old;
            failed = !finished;
        }
        if failed {
            ui.colored_label(ui.visuals().error_fg_color, "That name is taken or empty.");
        }
    });
    let mut slot = RENAME.lock().unwrap_or_else(PoisonError::into_inner);
    *slot = if finished || !open { None } else { Some((what, text)) };
}

/// Over a form the active view's template rules (docs/15 R2): a note *Controlled by template "…"*
/// and *Edit template*; `true` when the form is locked (the caller disables it). While editing,
/// the note says the changes go to every view using the template.
pub(crate) fn template_lock(ui: &mut egui::Ui, part: Part) -> bool {
    let Some((name, inc)) = windows::active_template() else { return false };
    let ruled = match part {
        Part::Vg => inc.vg,
        Part::Result => inc.result,
    };
    if !ruled {
        return false;
    }
    let editing = windows::editing_template();
    let mut locked = !editing;
    ui.horizontal(|ui| {
        if editing {
            ui.colored_label(ui.visuals().warn_fg_color, format!("Editing template \"{name}\": changes go to every view using it."));
            if ui.button("Done").clicked() {
                windows::set_editing_template(false);
                locked = true;
            }
        } else {
            ui.label(format!("Controlled by template \"{name}\"."));
            if ui.button("Edit template").clicked() {
                windows::set_editing_template(true);
                locked = false;
            }
        }
    });
    ui.separator();
    locked
}

/// The forms a template can lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Vg,
    Result,
}
