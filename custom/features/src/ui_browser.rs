//! The browser's Structure tree (replaces SolidCraft's browser rows).
//!
//! Load Case and Load Combination have no sub-tree: right-click → Properties opens their table
//! (drawn by [`windows`], which the status bar calls every frame).

use custom_bridge::ui::egui;
use custom_bridge::ui::tree_row;

use crate::model::{self, BOUNDARY_KINDS};

/// Which table a Properties window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Table {
    LoadCases,
    LoadCombinations,
}

/// Boundary rows: (key, icon) in [`BOUNDARY_KINDS`] order.
const BOUNDARY_ROWS: [(&str, &str); 11] = [
    ("supports", "c_fix"),
    ("point_springs", "coil"),
    ("general_springs", "coil"),
    ("surface_springs", "coil"),
    ("elastic_link", "chain"),
    ("general_link", "chain"),
    ("change_general_link", "chain"),
    ("beam_end_release", "break"),
    ("beam_end_offsets", "offset"),
    ("plate_end_release", "break"),
    ("rigid_link", "rigid_group"),
];

/// Draw the tree. Called by the browser every frame it is visible.
pub fn draw(ui: &mut egui::Ui) {
    let m = model::read(|f| f.model.clone_counts());
    ui.spacing_mut().item_spacing.y = 0.0;
    folder(ui, "structure", 0, "component", "Structure", None, true, |ui| {
        leaf(ui, "nodes", 1, "point", "Nodes", m.nodes);
        leaf(ui, "elements", 1, "frame_member", "Elements", m.elements);
        leaf(ui, "node_local_axis", 1, "axis", "Node Local Axis", m.node_local_axes);
        folder(ui, "properties", 1, "params", "Properties", Some(m.materials + m.sections + m.thicknesses), false, |ui| {
            leaf(ui, "material", 2, "material", "Material", m.materials);
            leaf(ui, "section", 2, "section", "Section", m.sections);
            leaf(ui, "thickness", 2, "thicken", "Thickness", m.thicknesses);
        });
        folder(ui, "boundaries", 1, "boundary", "Boundaries", Some(m.boundaries.iter().sum()), false, |ui| {
            for (i, (key, icon)) in BOUNDARY_ROWS.iter().enumerate() {
                leaf(ui, key, 2, icon, BOUNDARY_KINDS[i], m.boundaries[i]);
            }
        });
        load_row(ui, "load_case", "list", "Load Case", m.load_cases, Table::LoadCases);
        load_row(ui, "load_combination", "combine", "Load Combination", m.load_combinations, Table::LoadCombinations);
    });
    context_menu(ui);
}

/// The counts only (the tables are read when their window is open).
struct Counts {
    nodes: usize,
    elements: usize,
    node_local_axes: usize,
    materials: usize,
    sections: usize,
    thicknesses: usize,
    boundaries: [usize; 11],
    load_cases: usize,
    load_combinations: usize,
}

impl model::Model {
    fn clone_counts(&self) -> Counts {
        Counts {
            nodes: self.nodes,
            elements: self.elements,
            node_local_axes: self.node_local_axes,
            materials: self.materials,
            sections: self.sections,
            thicknesses: self.thicknesses,
            boundaries: self.boundaries,
            load_cases: self.load_cases.len(),
            load_combinations: self.load_combinations.len(),
        }
    }
}

fn open_id(key: &str) -> egui::Id {
    egui::Id::unique(("factory_browser_open", key))
}

/// A row that folds; `count` None = no count shown.
#[allow(clippy::too_many_arguments)]
fn folder(ui: &mut egui::Ui, key: &str, depth: usize, icon: &str, label: &str, count: Option<usize>, default_open: bool, body: impl FnOnce(&mut egui::Ui)) {
    let id = open_id(key);
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    let count = count.map(|c| c.to_string()).unwrap_or_default();
    let r = tree_row(ui, key, depth, Some(open), icon, label, &count);
    let open = if r.toggled || r.clicked { !open } else { open };
    ui.data_mut(|d| d.insert_temp(id, open));
    if open {
        body(ui);
    }
}

fn leaf(ui: &mut egui::Ui, key: &str, depth: usize, icon: &str, label: &str, count: usize) {
    tree_row(ui, key, depth, None, icon, label, &count.to_string());
}

/// Load Case / Load Combination: a counted row without sub-tree; right-click opens the menu.
fn load_row(ui: &mut egui::Ui, key: &str, icon: &str, label: &str, count: usize, table: Table) {
    let r = tree_row(ui, key, 1, None, icon, label, &count.to_string());
    if let Some(at) = r.context_at {
        ui.data_mut(|d| d.insert_temp(menu_id(), (at, table)));
    }
}

fn menu_id() -> egui::Id {
    egui::Id::unique("factory_browser_menu")
}

fn window_id(table: Table) -> egui::Id {
    egui::Id::unique(("factory_properties_open", table as u8))
}

/// The right-click menu: Properties.
fn context_menu(ui: &mut egui::Ui) {
    let Some((at, table)) = ui.data(|d| d.get_temp::<(egui::Pos2, Table)>(menu_id())) else { return };
    let ctx = ui.ctx().clone();
    let area = egui::Area::new(egui::Id::unique("factory_browser_menu_area")).order(egui::Order::Foreground).fixed_pos(at).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            if ui.button("Properties").clicked() {
                ui.data_mut(|d| {
                    d.insert_temp(window_id(table), true);
                    d.remove::<(egui::Pos2, Table)>(menu_id());
                });
            }
        });
    });
    // A click anywhere else, or Escape, closes the menu.
    let elsewhere = ctx.input(|i| i.pointer.primary_clicked()) && !area.response.contains_pointer();
    if elsewhere || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.remove::<(egui::Pos2, Table)>(menu_id()));
    }
}

/// The Properties windows. Called every frame (from the status bar, which is always drawn).
pub fn windows(ctx: &egui::Context) {
    for table in [Table::LoadCases, Table::LoadCombinations] {
        let id = window_id(table);
        let mut open = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        if !open {
            continue;
        }
        let title = match table {
            Table::LoadCases => "Load Cases",
            Table::LoadCombinations => "Load Combinations",
        };
        egui::Window::new(title).id(egui::Id::unique(("factory_properties", table as u8))).open(&mut open).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| match table {
                Table::LoadCases => load_cases_table(ui),
                Table::LoadCombinations => load_combinations_table(ui),
            });
        });
        ctx.data_mut(|d| d.insert_temp(id, open));
    }
}

fn load_cases_table(ui: &mut egui::Ui) {
    model::read(|f| {
        let cases = &f.model.load_cases;
        if cases.is_empty() {
            ui.label("No load cases. Import a model (CONNECT › MIDAS Civil NX / CSI).");
            return;
        }
        egui::Grid::new("factory_load_cases").striped(true).num_columns(3).show(ui, |ui| {
            ui.strong("Name");
            ui.strong("Type");
            ui.strong("Description");
            ui.end_row();
            for c in cases {
                ui.label(&c.name);
                ui.label(&c.kind);
                ui.label(&c.description);
                ui.end_row();
            }
        });
    });
}

fn load_combinations_table(ui: &mut egui::Ui) {
    model::read(|f| {
        let combinations = &f.model.load_combinations;
        if combinations.is_empty() {
            ui.label("No load combinations. Import a model (CONNECT › MIDAS Civil NX / CSI).");
            return;
        }
        egui::Grid::new("factory_load_combinations").striped(true).num_columns(3).show(ui, |ui| {
            ui.strong("Name");
            ui.strong("Type");
            ui.strong("Load cases × factor");
            ui.end_row();
            for c in combinations {
                ui.label(&c.name);
                ui.label(&c.kind);
                let factors: Vec<String> = c.factors.iter().map(|(case, factor)| format!("{factor} × {case}")).collect();
                ui.label(factors.join(" + "));
                ui.end_row();
            }
        });
    });
}
