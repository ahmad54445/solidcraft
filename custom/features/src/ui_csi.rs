//! The CSiBridge selection dialog: load cases and load combinations with check boxes,
//! Select all / Clear per list, Send.

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
        .default_width(420.0)
        .show(ctx, |ui| match d.phase.clone() {
            Phase::Closed => {}
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
    Send,
}

fn ready(ui: &mut egui::Ui, d: &mut Dialog) -> Option<Action> {
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

/// A titled list of check boxes with Select all / Clear.
fn list(ui: &mut egui::Ui, title: &str, id: &str, items: &mut [Item]) {
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Clear").clicked() {
                Dialog::set_all(items, false);
            }
            if ui.small_button("Select all").clicked() {
                Dialog::set_all(items, true);
            }
        });
    });
    if items.is_empty() {
        ui.weak("none defined");
        return;
    }
    egui::ScrollArea::vertical().id_salt(id).max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
        for item in items.iter_mut() {
            let label = match &item.blocked {
                Some(why) => format!("{}  ({}, {why})", item.name, item.kind),
                None => format!("{}  ({})", item.name, item.kind),
            };
            ui.add_enabled(item.blocked.is_none(), egui::Checkbox::new(&mut item.checked, label));
        }
    });
}
