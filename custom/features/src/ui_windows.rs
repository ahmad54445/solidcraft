//! Display windows on screen (docs/14): each window's title bar and frame, and the layout picker
//! (§1.1). The pictures themselves are drawn by SolidCraft (`custom_tiles`): the active window is
//! the ordinary 3D view, the others are drawn from the same GPU data with their own camera.

use std::sync::{Mutex, PoisonError};

use custom_bridge::ui::egui;
use custom_bridge::view::{TilePlan, ViewInfo};

use crate::windows::{self, Layout, Windows};

/// Height of a window's title bar (points).
const BAR: f32 = 20.0;
/// How long the pointer rests on a maximise button before the picker opens (seconds).
const HOVER_OPEN: f64 = 0.45;

/// The open layout picker: for which window, where, and whether hovering opened it.
#[derive(Clone, Copy)]
struct Picker {
    tile: u64,
    at: egui::Pos2,
    by_hover: bool,
}

static PICKER: Mutex<Option<Picker>> = Mutex::new(None);

fn picker() -> Option<Picker> {
    *PICKER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn set_picker(p: Option<Picker>) {
    *PICKER.lock().unwrap_or_else(PoisonError::into_inner) = p;
}

/// The view area: frames and title bars of the windows, the layout picker; where each picture
/// goes (`None` = one window, the ordinary view).
pub fn layout(ui: &mut egui::Ui, full: egui::Rect) -> Option<TilePlan> {
    let w = windows::current();
    if windows::take_picker_request() {
        set_picker(Some(Picker { tile: w.active, at: full.left_top() + egui::vec2(8.0, 8.0), by_hover: false }));
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
    let zones = w.layout.rects();
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
                set_picker(Some(Picker { tile: win.id, at: max.left_bottom() + egui::vec2(-150.0, 2.0), by_hover: true }));
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
    Some(TilePlan { tiles, active: w.active })
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

/// The layout picker (docs/14 §1.1): the current arrangement (P2), then the layouts L5, L1–L4.
fn draw_picker(ctx: &egui::Context, w: &Windows) {
    let Some(p) = picker() else { return };
    if w.index_of(p.tile).is_none() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        set_picker(None);
        return;
    }
    let mut chosen: Option<Box<dyn FnOnce()>> = None;
    let area = egui::Area::new(egui::Id::unique("factory_layout_picker")).order(egui::Order::Foreground).fixed_pos(p.at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            if w.windows.len() > 1 && w.maximised.is_none() {
                ui.label(egui::RichText::new("Current").small());
                if let Some(zone) = thumbnail(ui, w.layout, |k| w.windows.get(k).map(|x| w.number(x.id).to_string())) {
                    if let Some(other) = w.windows.get(zone) {
                        let (a, b) = (p.tile, other.id);
                        chosen = Some(Box::new(move || windows::swap(a, b)));
                    }
                }
                ui.separator();
            }
            ui.label(egui::RichText::new("Layout").small());
            for row in Layout::ALL.chunks(3) {
                ui.horizontal(|ui| {
                    for &layout in row {
                        if let Some(zone) = thumbnail(ui, layout, |_| None) {
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

/// One layout drawn as its zones; the zone under the pointer lights in the accent colour.
/// Returns the zone clicked.
fn thumbnail(ui: &mut egui::Ui, layout: Layout, label: impl Fn(usize) -> Option<String>) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(66.0, 44.0), egui::Sense::hover());
    let accent = ui.visuals().selection.bg_fill;
    let idle = ui.visuals().widgets.inactive.bg_fill;
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    let mut clicked = None;
    for (k, [l, t, r, b]) in layout.rects().into_iter().enumerate() {
        let zone = egui::Rect::from_min_max(rect.lerp_inside(egui::vec2(l, t)), rect.lerp_inside(egui::vec2(r, b))).shrink(1.5);
        let resp = ui.interact(zone, ui.scope_id().with(("factory_zone", layout.key(), k)), egui::Sense::click());
        let fill = if resp.hovered() { accent } else { idle };
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
