//! Factory's status bar along the window's bottom edge:
//! Selected | LC | Units ▾ ............ MIDAS: state | RAM · cache | ● backend

use custom_bridge::ui::egui;

use crate::model::{self, Backend};
use crate::units::{Force, Length, Moment, Stress};

/// Draw the bar. Called by SolidCraft every frame.
pub fn draw(ui: &mut egui::Ui) {
    // Workers (result colouring) ask for a new frame through this.
    let ctx = ui.ctx().clone();
    crate::results_view::set_repaint(Box::new(move || ctx.request_repaint()));
    let (status, units) = model::read(|f| (f.status.clone(), f.units));
    let ram = ram_mb(ui.ctx());
    ui.horizontal_centered(|ui| {
        ui.add_space(8.0);
        ui.label(format!("Selected: {} shells, {} frames", status.selected_shells, status.selected_frames));
        ui.separator();
        crate::ui_results::status_controls(ui);
        ui.separator();
        let units_button = ui.button(format!("Units: {} ▾", units.short()));
        if units_button.clicked() {
            let id = units_open_id();
            ui.data_mut(|d| {
                let open = d.get_temp::<bool>(id).unwrap_or(false);
                d.insert_temp(id, !open);
            });
        }
        units_popup(ui, units_button.rect);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let (text, colour) = match &status.backend {
                Backend::Ready => ("backend ready".to_string(), egui::Color32::from_rgb(76, 175, 80)),
                Backend::Busy(what) => (format!("backend busy: {what}"), egui::Color32::from_rgb(245, 158, 11)),
                Backend::Error(what) => (format!("backend error: {what}"), egui::Color32::from_rgb(229, 57, 53)),
            };
            ui.label(text);
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.5, colour);
            ui.separator();
            ui.label(format!("RAM {ram} MB · cache {}/{} MB", status.cache_used_mb, status.cache_budget_mb));
            ui.separator();
            ui.label(format!("{}: {}", status.program, status.connection.label()));
        });
    });
    crate::ui_browser::windows(ui.ctx());
    crate::ui_csi::show(ui.ctx());
}

fn units_open_id() -> egui::Id {
    egui::Id::unique("factory_units_open")
}

/// The units menu above the button: length, force, moment, stress.
fn units_popup(ui: &mut egui::Ui, button: egui::Rect) {
    let ctx = ui.ctx().clone();
    if !ctx.data(|d| d.get_temp::<bool>(units_open_id())).unwrap_or(false) {
        return;
    }
    let mut units = model::read(|f| f.units);
    let before = units;
    let area = egui::Area::new(egui::Id::unique("factory_units_area"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(button.left_top() - egui::vec2(0.0, 4.0))
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                egui::Grid::new("factory_units_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Length");
                    combo(ui, "length", &mut units.length, &Length::ALL, |u| u.symbol());
                    ui.end_row();
                    ui.label("Force");
                    combo(ui, "force", &mut units.force, &Force::ALL, |u| u.symbol());
                    ui.end_row();
                    ui.label("Moment");
                    combo(ui, "moment", &mut units.moment, &Moment::ALL, |u| u.symbol());
                    ui.end_row();
                    ui.label("Stress");
                    combo(ui, "stress", &mut units.stress, &Stress::ALL, |u| u.symbol());
                    ui.end_row();
                });
            });
        });
    if units != before {
        model::write(|f| f.units = units);
    }
    // Escape, or a click outside the menu and its button, closes it (the combo lists are their
    // own layers, so a click there keeps it open).
    let outside = ctx.input(|i| i.pointer.primary_clicked())
        && !area.response.contains_pointer()
        && !button.contains(ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default())
        && !egui::Popup::is_any_open(&ctx);
    if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.insert_temp(units_open_id(), false));
    }
}

fn combo<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut T, all: &[T], symbol: impl Fn(T) -> &'static str) {
    egui::ComboBox::from_id_salt(("factory_units", id)).selected_text(symbol(*value)).show_ui(ui, |ui| {
        for u in all {
            ui.selectable_value(value, *u, symbol(*u));
        }
    });
}

/// Memory used by Factory (MB), read at most once a second.
fn ram_mb(ctx: &egui::Context) -> u64 {
    let id = egui::Id::unique("factory_ram");
    let now = ctx.input(|i| i.time);
    if let Some((at, mb)) = ctx.data(|d| d.get_temp::<(f64, u64)>(id))
        && now - at < 1.0
    {
        return mb;
    }
    let mb = memory_stats::memory_stats().map_or(0, |m| (m.physical_mem / (1024 * 1024)) as u64);
    ctx.data_mut(|d| d.insert_temp(id, (now, mb)));
    mb
}
