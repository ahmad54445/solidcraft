//! Factory's branches in the browser, after SolidCraft's Document Settings, Named Views and
//! Origin: Structure, Linked, Design, Groups (README › Browser).
//!
//! Every row with children has a fold arrow; rows without children have none. Right-click on a
//! branch: Expand All / Collapse All (that branch and its descendants). Load Case and Load
//! Combination: right-click → Properties (their tables are drawn by [`windows`], which the status
//! bar calls every frame).
//!
//! Hide / Isolate (docs/13 §8): Elements › Shells / Frames, every section under Properties ›
//! Section and every group have an **eye** – a click hides them all when any is shown, else shows
//! them; a click on the row's name **selects** them (added to the selection, Ctrl removes).
//! *Hidden elements* shows how many are hidden by hand; its eye shows them all again.

use std::sync::Arc;

use custom_bridge::ui::egui;
use custom_bridge::ui::{tree_row, tree_row_eye};

use crate::selection::{self, Kinds, Scope, Set};

use crate::model::{self, BOUNDARY_KINDS, DESIGN_KINDS};

/// Which table a Properties window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Table {
    LoadCases,
    LoadCombinations,
}

/// What a right-click opened a menu for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Menu {
    /// A branch: Expand All / Collapse All.
    Branch(String),
    /// Load Case / Load Combination: Properties.
    Load(Table),
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

/// Design rows: (key, icon) in [`DESIGN_KINDS`] order.
const DESIGN_ROWS: [(&str, &str); 4] = [("design_slab", "shell"), ("design_raft", "box"), ("design_column", "frame_member"), ("design_beam", "frame_member")];

/// What the rows need this frame (counts; lists only where a branch is open).
struct View {
    nodes: usize,
    elements: usize,
    node_local_axes: usize,
    materials: usize,
    sections: usize,
    thicknesses: usize,
    boundaries: [usize; 11],
    load_cases: usize,
    load_combinations: usize,
    ifc: Vec<String>,
    dxf: Vec<String>,
    design: [Vec<String>; 4],
    groups: Vec<(String, Vec<String>)>,
}

/// The rows, starting at `depth` (SolidCraft calls this right after Origin).
pub fn rows(ui: &mut egui::Ui, depth: usize) {
    let v = model::read(|f| {
        let m = &f.model;
        View {
            nodes: m.nodes,
            elements: m.elements,
            node_local_axes: m.node_local_axes,
            materials: m.materials,
            sections: m.sections,
            thicknesses: m.thicknesses,
            boundaries: m.boundaries,
            load_cases: m.load_cases.len(),
            load_combinations: m.load_combinations.len(),
            ifc: m.linked_ifc.clone(),
            dxf: m.linked_dxf.clone(),
            design: m.design.clone(),
            groups: m.groups.clone(),
        }
    });
    let d = depth;

    // Structure
    folder(ui, "structure", d, "component", "Structure", Some(v.elements), true, |ui| {
        leaf(ui, "nodes", d + 1, "point", "Nodes", Some(v.nodes));
        let sets = element_sets();
        folder(ui, "elements", d + 1, "frame_member", "Elements", Some(v.elements), true, |ui| {
            eye_row(ui, "elements/shells", d + 2, "shell", "Shells", &sets.shells);
            eye_row(ui, "elements/frames", d + 2, "frame_member", "Frames", &sets.frames);
        });
        hidden_row(ui, d + 1);
        leaf(ui, "node_local_axis", d + 1, "axis", "Node Local Axis", Some(v.node_local_axes));
        folder(ui, "properties", d + 1, "params", "Properties", Some(v.materials + v.sections + v.thicknesses), true, |ui| {
            leaf(ui, "material", d + 2, "material", "Material", Some(v.materials));
            folder(ui, "section", d + 2, "section", "Section", Some(v.sections), !sets.sections.is_empty(), |ui| {
                for (k, (name, set)) in sets.sections.iter().enumerate() {
                    eye_row(ui, &format!("section/{k}"), d + 3, "section", name, set);
                }
            });
            leaf(ui, "thickness", d + 2, "thicken", "Thickness", Some(v.thicknesses));
        });
        folder(ui, "boundaries", d + 1, "boundary", "Boundaries", Some(v.boundaries.iter().sum()), true, |ui| {
            for (i, (key, icon)) in BOUNDARY_ROWS.iter().enumerate() {
                leaf(ui, key, d + 2, icon, BOUNDARY_KINDS[i], Some(v.boundaries[i]));
            }
        });
        load_row(ui, "load_case", d + 1, "list", "Load Case", v.load_cases, Table::LoadCases);
        load_row(ui, "load_combination", d + 1, "combine", "Load Combination", v.load_combinations, Table::LoadCombinations);
        crate::ui_cut::rows(ui, d + 1); // Cut Diagrams (docs/16 C4)
    });

    // Linked: IFC and DXF expand to their files
    folder(ui, "linked", d, "include", "Linked", Some(v.ifc.len() + v.dxf.len()), true, |ui| {
        list(ui, "linked_ifc", d + 1, "import", "IFC", &v.ifc, "open");
        list(ui, "linked_dxf", d + 1, "dxf", "DXF", &v.dxf, "open");
    });

    // Design: Slab, Raft, Column, Beam expand to their items
    folder(ui, "design", d, "inspect", "Design", Some(v.design.iter().map(Vec::len).sum()), true, |ui| {
        for (i, (key, icon)) in DESIGN_ROWS.iter().enumerate() {
            list(ui, key, d + 1, icon, DESIGN_KINDS[i], &v.design[i], icon);
        }
    });

    // Groups: each group expands to its members
    folder(ui, "groups", d, "folder", "Groups", Some(v.groups.len()), !v.groups.is_empty(), |ui| {
        for (i, (name, members)) in v.groups.iter().enumerate() {
            group_row(ui, &group_key(i), d + 1, name, members);
        }
    });

    context_menu(ui, &v);
}

fn group_key(i: usize) -> String {
    format!("group:{i}")
}

fn open_id(key: &str) -> egui::Id {
    egui::Id::unique(("factory_browser_open", key))
}

fn is_open(ui: &egui::Ui, key: &str, default: bool) -> bool {
    ui.data(|d| d.get_temp::<bool>(open_id(key))).unwrap_or(default)
}

fn set_open(ctx: &egui::Context, key: &str, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(key), open));
}

/// A row that folds when it has children (`has_children`), else a plain row without arrow.
#[allow(clippy::too_many_arguments)]
fn folder(ui: &mut egui::Ui, key: &str, depth: usize, icon: &str, label: &str, count: Option<usize>, has_children: bool, body: impl FnOnce(&mut egui::Ui)) {
    let count = count.map(|c| c.to_string()).unwrap_or_default();
    if !has_children {
        let r = tree_row(ui, key, depth, None, icon, label, &count);
        if let Some(at) = r.context_at {
            open_menu(ui, at, Menu::Branch(key.to_string()));
        }
        return;
    }
    let open = is_open(ui, key, false);
    let r = tree_row(ui, key, depth, Some(open), icon, label, &count);
    let open = if r.toggled || r.clicked { !open } else { open };
    set_open(ui.ctx(), key, open);
    if let Some(at) = r.context_at {
        open_menu(ui, at, Menu::Branch(key.to_string()));
    }
    if open {
        body(ui);
    }
}

fn leaf(ui: &mut egui::Ui, key: &str, depth: usize, icon: &str, label: &str, count: Option<usize>) {
    tree_row(ui, key, depth, None, icon, label, &count.map(|c| c.to_string()).unwrap_or_default());
}

/// A branch whose children are the names in `items` (an arrow only when there are any).
fn list(ui: &mut egui::Ui, key: &str, depth: usize, icon: &str, label: &str, items: &[String], item_icon: &str) {
    folder(ui, key, depth, icon, label, Some(items.len()), !items.is_empty(), |ui| {
        for (i, name) in items.iter().enumerate() {
            leaf(ui, &format!("{key}/{i}"), depth + 1, item_icon, name, None);
        }
    });
}

/// Load Case / Load Combination: a counted row without sub-tree; right-click opens the menu.
fn load_row(ui: &mut egui::Ui, key: &str, depth: usize, icon: &str, label: &str, count: usize, table: Table) {
    let r = tree_row(ui, key, depth, None, icon, label, &count.to_string());
    if let Some(at) = r.context_at {
        open_menu(ui, at, Menu::Load(table));
    }
}

fn menu_id() -> egui::Id {
    egui::Id::unique("factory_browser_menu")
}

fn open_menu(ui: &egui::Ui, at: egui::Pos2, menu: Menu) {
    ui.data_mut(|d| d.insert_temp(menu_id(), (at, menu)));
}

fn window_id(table: Table) -> egui::Id {
    egui::Id::unique(("factory_properties_open", table as u8))
}

/// The branches below `key` that fold (for Expand All / Collapse All).
fn children(key: &str, v: &View) -> Vec<String> {
    let owned = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect();
    match key {
        "structure" => owned(&["properties", "boundaries"]),
        "linked" => owned(&["linked_ifc", "linked_dxf"]),
        "design" => DESIGN_ROWS.iter().map(|(k, _)| k.to_string()).collect(),
        "groups" => (0..v.groups.len()).map(group_key).collect(),
        _ => Vec::new(),
    }
}

/// Open or close `key` and every branch below it.
fn set_open_all(ctx: &egui::Context, key: &str, open: bool, v: &View) {
    set_open(ctx, key, open);
    for child in children(key, v) {
        set_open_all(ctx, &child, open, v);
    }
}

/// The right-click menu: Expand All / Collapse All on branches, Properties on loads.
fn context_menu(ui: &mut egui::Ui, v: &View) {
    let Some((at, menu)) = ui.data(|d| d.get_temp::<(egui::Pos2, Menu)>(menu_id())) else { return };
    let ctx = ui.ctx().clone();
    let mut done = false;
    let area = egui::Area::new(egui::Id::unique("factory_browser_menu_area")).order(egui::Order::Foreground).fixed_pos(at).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| match &menu {
            Menu::Branch(key) => {
                if ui.button("Expand All").clicked() {
                    set_open_all(&ctx, key, true, v);
                    done = true;
                }
                if ui.button("Collapse All").clicked() {
                    set_open_all(&ctx, key, false, v);
                    done = true;
                }
            }
            Menu::Load(table) => {
                if ui.button("Properties").clicked() {
                    ctx.data_mut(|d| d.insert_temp(window_id(*table), true));
                    done = true;
                }
            }
        });
    });
    // A choice, a click anywhere else, or Escape closes the menu.
    let elsewhere = ctx.input(|i| i.pointer.primary_clicked()) && !area.response.contains_pointer();
    if done || elsewhere || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.remove::<(egui::Pos2, Menu)>(menu_id()));
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
            ui.label("No load cases. Import a model (CONNECT › MIDAS Civil NX / CSiBridge).");
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
            ui.label("No load combinations. Import a model (CONNECT › MIDAS Civil NX / CSiBridge).");
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

// ---- Hide / Isolate from the Browser (docs/13 §8) -----------------------------------------------

/// The element sets the Browser's eyes stand for, built once per transfer.
struct Sets {
    shells: Set,
    frames: Set,
    /// Shell sections, then frame sections: (name, its elements).
    sections: Vec<(String, Set)>,
}

fn element_sets() -> Arc<Sets> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(usize, Arc<Sets>)>> = Mutex::new(None);
    let g = crate::transfer::geometry();
    let key = Arc::as_ptr(&g) as usize;
    let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((k, sets)) = cache.as_ref()
        && *k == key
    {
        return sets.clone();
    }
    let mut shells = Set::empty(&g);
    let mut frames = Set::empty(&g);
    let mut shell_sections: Vec<Set> = (0..g.shell_sections.len()).map(|_| Set::empty(&g)).collect();
    let mut frame_sections: Vec<Set> = (0..g.frame_sections.len()).map(|_| Set::empty(&g)).collect();
    for (e, s) in g.shells.iter().enumerate() {
        shells.shells.set(e, true);
        if let Some(set) = shell_sections.get_mut(s[4] as usize) {
            set.shells.set(e, true);
        }
    }
    for (e, f) in g.frames.iter().enumerate() {
        frames.frames.set(e, true);
        if let Some(set) = frame_sections.get_mut(f[2] as usize) {
            set.frames.set(e, true);
        }
    }
    let sections = g
        .shell_sections
        .iter()
        .cloned()
        .zip(shell_sections)
        .chain(g.frame_sections.iter().cloned().zip(frame_sections))
        .filter(|(_, set)| !set.is_empty())
        .collect();
    let sets = Arc::new(Sets { shells, frames, sections });
    *cache = Some((key, sets.clone()));
    sets
}

/// A row standing for `set`: count, eye (hide / show), click on the name selects.
fn eye_row(ui: &mut egui::Ui, key: &str, depth: usize, icon: &str, label: &str, set: &Set) {
    let (hidden, total) = crate::visibility::hidden_of(set);
    let count = if hidden > 0 { format!("{} / {total}", total - hidden) } else { total.to_string() };
    let r = tree_row_eye(ui, key, depth, None, icon, label, &count, hidden < total);
    eye_and_select(ui, &r, set);
}

/// A group: the arrow folds it (its members), the eye hides / shows it, the name selects it.
fn group_row(ui: &mut egui::Ui, key: &str, depth: usize, name: &str, members: &[String]) {
    let g = crate::transfer::geometry();
    let set = selection::in_groups(Scope { g: &g, shell_shown: &[], frame_shown: &[], kinds: Kinds::default() }, members).set;
    let (hidden, total) = crate::visibility::hidden_of(&set);
    let open = is_open(ui, key, false);
    let fold = (!members.is_empty()).then_some(open);
    let r = tree_row_eye(ui, key, depth, fold, "folder", name, &members.len().to_string(), total == 0 || hidden < total);
    if r.toggled {
        set_open(ui.ctx(), key, !open);
    }
    if let Some(at) = r.context_at {
        open_menu(ui, at, Menu::Branch(key.to_string()));
    }
    eye_and_select(ui, &r, &set);
    if open && !r.toggled {
        for (i, m) in members.iter().enumerate() {
            leaf(ui, &format!("{key}/{i}"), depth + 1, "list", m, None);
        }
    }
}

fn eye_and_select(ui: &egui::Ui, r: &custom_bridge::ui::TreeRow, set: &Set) {
    if r.eye {
        crate::visibility::toggle(set);
        // Hidden elements leave the selection (they cannot be selected, docs/13 R2).
        selection::apply(selection::Found { set: crate::visibility::hidden(), hidden: 0 }, true);
        crate::results_view::refresh();
    } else if r.clicked {
        let remove = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        let shown = crate::results_view::snap_data();
        let g = crate::transfer::geometry();
        let scope = Scope { g: &g, shell_shown: &shown.shell_shown, frame_shown: &shown.frame_shown, kinds: selection::kinds() };
        selection::apply(selection::within(scope, set), remove);
    }
}

/// How many elements are hidden by hand; its eye shows them all again.
fn hidden_row(ui: &mut egui::Ui, depth: usize) {
    let (shells, frames) = crate::visibility::hidden().counts();
    let r = tree_row_eye(ui, "hidden_elements", depth, None, "suppress", "Hidden elements", &(shells + frames).to_string(), shells + frames == 0);
    if r.eye && shells + frames > 0 {
        crate::visibility::show_all();
        crate::results_view::refresh();
    }
}
