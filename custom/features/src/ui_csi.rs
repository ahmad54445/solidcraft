//! The CSiBridge selection dialog: load cases and load combinations with check boxes,
//! a filter and Select all / Clear per list, Send.

use custom_bridge::ui::egui;

use crate::csi::{self, Dialog, Item, Phase};

/// Draw the dialog while it is open. Called every frame (from the status bar).
pub fn show(ctx: &egui::Context) {
    if !csi::is_open() {
        return;
    }
    let mut d = csi::dialog();
    let before = d.clone();
    let mut open = true;
    let mut action = None;
    egui::Window::new(format!("{} – send to Factory", d.title))
        .id(egui::Id::unique("factory_csi_dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .show(ctx, |ui| match d.phase.clone() {
            Phase::Closed => {}
            Phase::Loading(what) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(what);
                });
                // The worker thread fills the dialog; look again shortly.
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Phase::Choose(instances) => {
                ui.label("Several CSiBridge are running. Which one?");
                ui.add_space(4.0);
                for i in instances {
                    let title = if i.title.is_empty() { format!("process {}", i.pid) } else { i.title.clone() };
                    if ui.button(title).clicked() {
                        action = Some(Action::Choose(i.pid));
                    }
                }
                ui.add_space(6.0);
                if ui.button("Cancel").clicked() {
                    action = Some(Action::Cancel);
                }
            }
            Phase::Error(message) => {
                ui.label(message);
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    action = Some(Action::Cancel);
                }
            }
            Phase::Ready => action = ready(ui, &mut d),
        });
    match action {
        _ if !open => csi::close(),
        Some(Action::Cancel) => csi::close(),
        Some(Action::Choose(pid)) => csi::choose(pid),
        Some(Action::Send) => {
            d.send();
            csi::close();
        }
        None if d != before => csi::store(d),
        None => {}
    }
}

enum Action {
    Cancel,
    Choose(u32),
    Send,
}

fn ready(ui: &mut egui::Ui, d: &mut Dialog) -> Option<Action> {
    if !d.program.is_empty() {
        ui.weak(format!("{} – {}", d.program, d.model));
    }
    ui.label(format!("Selected in CSiBridge: {} shells, {} frames", d.shells, d.frames));
    if d.shells + d.frames == 0 {
        ui.colored_label(ui.visuals().warn_fg_color, "Select shell and/or frame objects in CSiBridge first.");
    }
    ui.add_space(6.0);
    list(ui, "Load Cases", "factory_csi_cases", &mut d.cases);
    ui.add_space(6.0);
    list(ui, "Load Combinations", "factory_csi_combos", &mut d.combos);
    ui.add_space(8.0);
    ui.separator();
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(format!("{} cases, {} combinations", d.ticked_cases(), d.ticked_combos()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(d.can_send(), egui::Button::new("Send")).clicked() {
                action = Some(Action::Send);
            }
            if ui.button("Cancel").clicked() {
                action = Some(Action::Cancel);
            }
        });
    });
    action
}

/// A titled list of check boxes with a filter, Select all / Clear (both act on what the filter
/// shows).
fn list(ui: &mut egui::Ui, title: &str, id: &str, items: &mut [Item]) {
    let filter_id = egui::Id::unique((id, "filter"));
    let mut filter = ui.data(|d| d.get_temp::<String>(filter_id)).unwrap_or_default();
    let mut select = None;
    ui.horizontal(|ui| {
        ui.strong(format!("{title} ({})", items.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Clear").clicked() {
                select = Some(false);
            }
            if ui.small_button("Select all").clicked() {
                select = Some(true);
            }
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text("filter").desired_width(120.0));
        });
    });
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    let needle = filter.to_lowercase();
    let shown = |i: &Item| needle.is_empty() || i.name.to_lowercase().contains(&needle);
    if let Some(on) = select {
        for i in items.iter_mut().filter(|i| shown(i)) {
            i.checked = on && i.blocked.is_none();
        }
    }
    if items.is_empty() {
        ui.weak("none defined");
        return;
    }
    egui::ScrollArea::vertical().id_salt(id).max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
        for item in items.iter_mut().filter(|i| shown(i)) {
            let label = match &item.blocked {
                Some(why) => format!("{}  ({}, {why})", item.name, item.kind),
                None => format!("{}  ({})", item.name, item.kind),
            };
            ui.add_enabled(item.blocked.is_none(), egui::Checkbox::new(&mut item.checked, label));
        }
    });
}
