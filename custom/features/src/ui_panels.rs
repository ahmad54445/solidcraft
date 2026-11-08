//! The results panel (docs/10 §7) and the Visibility/Graphics form (docs/12). Both only change
//! choices and settings; the view is rebuilt from the same arrays on a worker thread.

use custom_bridge::ui::egui;

use crate::display;
use crate::results_view::{self, Averaging, FRAME_COMPONENTS, SHELL_PANEL_NAMES, Step};
use crate::vv::{self, Action, Filter, Rule, Settings};

/// Draw both windows while they are open. Called every frame (from the status bar).
pub fn show(ctx: &egui::Context) {
    if display::results_panel_open() {
        results_panel(ctx);
    }
    if display::visibility_graphics_open() {
        visibility_graphics(ctx);
    }
}

/// Planned items: shown greyed with the reason.
fn planned(ui: &mut egui::Ui, text: &str, why: &str) {
    ui.add_enabled(false, egui::Checkbox::new(&mut false, text)).on_disabled_hover_text(why);
}

// ---------------------------------------------------------------------------------------------
// Results panel

/// Panel rows for shells: (component index in SHELL_COMPONENTS) in the MIDAS layout, 3 per row.
const SHELL_GRID: [[usize; 3]; 5] = [[0, 1, 2], [8, 9, 10], [3, 4, 5], [11, 12, 13], [6, 7, 14]];

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultType {
    Plate,
    Beam,
}

fn results_panel(ctx: &egui::Context) {
    let shown = results_view::shown();
    let loads = results_view::loads();
    let units = crate::model::read(|f| f.units);
    let geometry = crate::transfer::geometry();
    let mut settings = vv::settings();
    let mut choice = shown.choice.clone();
    let type_id = egui::Id::unique("factory_results_type");
    let mut kind = ctx.data(|d| d.get_temp::<bool>(type_id)).map_or(if geometry.shells.is_empty() && !geometry.frames.is_empty() { ResultType::Beam } else { ResultType::Plate }, |beam| {
        if beam { ResultType::Beam } else { ResultType::Plate }
    });
    let mut open = true;
    egui::Window::new("Results").id(egui::Id::unique("factory_results_panel")).open(&mut open).resizable(true).default_width(330.0).show(ctx, |ui| {
        if loads.is_empty() {
            ui.label("No results yet. CONNECT › CSiBridge › Send.");
            return;
        }
        // Result type
        egui::ComboBox::from_id_salt("factory_panel_type")
            .selected_text(if kind == ResultType::Plate { "Plate Forces/Moments" } else { "Beam Forces/Moments" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut kind, ResultType::Plate, "Plate Forces/Moments");
                ui.selectable_value(&mut kind, ResultType::Beam, "Beam Forces/Moments");
            });
        ui.add_space(4.0);

        // Load cases / combinations and step
        ui.group(|ui| {
            ui.strong("Load Cases/Combinations");
            egui::ComboBox::from_id_salt("factory_panel_lc").width(240.0).selected_text(choice.load.clone().unwrap_or_else(|| "—".into())).show_ui(ui, |ui| {
                for l in &loads {
                    ui.selectable_value(&mut choice.load, Some(l.clone()), l);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Step");
                ui.add_enabled_ui(shown.envelope, |ui| {
                    egui::ComboBox::from_id_salt("factory_panel_step").selected_text(if !shown.envelope { "—" } else if choice.step == Step::Max { "Max" } else { "Min" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut choice.step, Step::Max, "Max");
                        ui.selectable_value(&mut choice.step, Step::Min, "Min");
                    });
                });
            });
        });

        if kind == ResultType::Plate {
            // Plate force options
            ui.group(|ui| {
                ui.strong("Plate Force Options");
                ui.horizontal(|ui| {
                    let _ = ui.radio(true, "Local");
                    ui.add_enabled(false, egui::RadioButton::new(false, "UCS")).on_disabled_hover_text("Planned: needs the element local axes from CSiBridge (docs/10 §7.3).");
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut choice.averaging, Averaging::Element, "Element");
                    ui.radio_value(&mut choice.averaging, Averaging::Nodal, "Avg. Nodal");
                });
                ui.add_enabled(choice.averaging == Averaging::Nodal, egui::Checkbox::new(&mut choice.active_only, "Avg. Nodal Active Only"));
            });
            // Components
            ui.group(|ui| {
                ui.strong("Components");
                egui::Grid::new("factory_panel_components").num_columns(3).show(ui, |ui| {
                    for row in SHELL_GRID {
                        for c in row {
                            ui.radio_value(&mut choice.shell, c, SHELL_PANEL_NAMES[c]);
                        }
                        ui.end_row();
                    }
                });
                planned(ui, "Wood Armer Moment", "Planned: formulas and sign conventions to be fixed against the design code first (docs/10 §7.2).");
                ui.horizontal(|ui| {
                    planned(ui, "Fvector", "Planned: principal-direction arrows (docs/10 §7.2).");
                    planned(ui, "Mvector", "Planned: principal-direction arrows (docs/10 §7.2).");
                });
            });
        } else {
            ui.group(|ui| {
                ui.strong("Components");
                egui::Grid::new("factory_panel_frame_components").num_columns(3).show(ui, |ui| {
                    for (k, name) in FRAME_COMPONENTS.iter().enumerate() {
                        ui.radio_value(&mut choice.frame, k, *name);
                        if k % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            });
        }

        // Type of display
        ui.group(|ui| {
            ui.strong("Type of Display");
            egui::Grid::new("factory_panel_display").num_columns(2).show(ui, |ui| {
                ui.checkbox(&mut settings.contour, "Contour");
                planned(ui, "Deform", "Planned: needs displacements (Results.JointDispl) and the phase-2 pipeline (docs/10 §7.4).");
                ui.end_row();
                ui.checkbox(&mut settings.values, "Values");
                ui.checkbox(&mut settings.legend, "Legend");
                ui.end_row();
                planned(ui, "Animate", "Planned: phase-2 pipeline (docs/10 §7.4).");
                planned(ui, "Undeformed", "Planned: with Deform (docs/10 §7.4).");
                ui.end_row();
                planned(ui, "Mirrored", "Planned (docs/10 §7.4).");
                planned(ui, "Cutting Diagram", "Planned (docs/10 §7.4).");
                ui.end_row();
            });
            contour_range(ui, &shown, kind);
        });
    });
    ctx.data_mut(|d| d.insert_temp(type_id, kind == ResultType::Beam));
    if !open {
        display::set_results_panel(false);
    }
    if settings != vv::settings() {
        vv::apply(settings);
    }
    if choice != shown.choice || shown.units != Some(units) {
        results_view::select(choice, units);
        crate::model::write(|f| f.status.current_load = results_view::shown().choice.load);
    }
}

/// Contour range: automatic (min/max of what is shown) or typed – typed ranges change only the
/// colour scale, nothing is uploaded (rule P5).
fn contour_range(ui: &mut egui::Ui, shown: &results_view::Shown, kind: ResultType) {
    let (legend, manual) = match kind {
        ResultType::Plate => (shown.shell.as_ref(), shown.manual_shell),
        ResultType::Beam => (shown.frame.as_ref(), shown.manual_frame),
    };
    let Some(legend) = legend else { return };
    let id = egui::Id::unique(("factory_range_edit", kind == ResultType::Beam));
    let mut edit = ui.data(|d| d.get_temp::<[f32; 2]>(id)).unwrap_or(manual.unwrap_or(legend.range));
    let mut auto = manual.is_none();
    ui.collapsing("Contour range", |ui| {
        ui.horizontal(|ui| {
            ui.radio_value(&mut auto, true, "Auto");
            ui.radio_value(&mut auto, false, "Manual");
        });
        ui.add_enabled_ui(!auto, |ui| {
            ui.horizontal(|ui| {
                ui.label("min");
                ui.add(egui::DragValue::new(&mut edit[0]).speed(0.1));
                ui.label("max");
                ui.add(egui::DragValue::new(&mut edit[1]).speed(0.1));
                ui.label(&legend.unit);
            });
        });
    });
    ui.data_mut(|d| d.insert_temp(id, edit));
    let wanted = if auto || edit[1] <= edit[0] { None } else { Some(edit) };
    if wanted != manual {
        match kind {
            ResultType::Plate => results_view::set_manual_ranges(wanted, shown.manual_frame),
            ResultType::Beam => results_view::set_manual_ranges(shown.manual_shell, wanted),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Visibility/Graphics form

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Model,
    Imported,
    Filters,
}

/// Visibility/Graphic Overrides (docs/12): Model Categories, Imported Categories, Filters.
/// What is drawn on the result (contour, legend, values) is chosen in the Results panel only,
/// so the two windows never disagree.
fn visibility_graphics(ctx: &egui::Context) {
    // The form edits a copy; OK / Apply commit it in one step (rule G7).
    let edit_id = egui::Id::unique("factory_vv_edit");
    let tab_id = egui::Id::unique("factory_vv_tab");
    let mut edit: Settings = ctx.data(|d| d.get_temp::<Settings>(edit_id)).unwrap_or_else(vv::settings);
    let mut tab = ctx.data(|d| d.get_temp::<u8>(tab_id)).map_or(Tab::Model, |t| match t {
        1 => Tab::Imported,
        2 => Tab::Filters,
        _ => Tab::Model,
    });
    let geometry = crate::transfer::geometry();
    let mut open = true;
    let mut close = false;
    egui::Window::new("Visibility/Graphic Overrides").id(egui::Id::unique("factory_vv")).open(&mut open).resizable(true).default_width(760.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (t, name) in [(Tab::Model, "Model Categories"), (Tab::Imported, "Imported Categories"), (Tab::Filters, "Filters")] {
                if ui.selectable_label(tab == t, name).clicked() {
                    tab = t;
                }
            }
        });
        ui.separator();
        match tab {
            Tab::Model => model_tab(ui, &mut edit, &geometry),
            Tab::Imported => imported_tab(ui),
            Tab::Filters => filters_tab(ui, &mut edit, &geometry),
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("All").clicked() {
                edit.shells.visible = true;
                edit.frames.visible = true;
            }
            if ui.button("None").clicked() {
                edit.shells.visible = false;
                edit.frames.visible = false;
            }
            if ui.button("Invert").clicked() {
                edit.shells.visible = !edit.shells.visible;
                edit.frames.visible = !edit.frames.visible;
            }
            if ui.button("Reset view").clicked() {
                edit = Settings::default();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Apply").clicked() {
                    vv::apply(edit.clone());
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui.button("OK").clicked() {
                    vv::apply(edit.clone());
                    close = true;
                }
            });
        });
    });
    if close || !open {
        display::set_visibility_graphics(false);
        ctx.data_mut(|d| d.remove::<Settings>(edit_id));
    } else {
        ctx.data_mut(|d| d.insert_temp(edit_id, edit));
    }
    ctx.data_mut(|d| d.insert_temp(tab_id, tab as u8));
}

const NO_TRANSPARENCY: &str = "Planned: SolidCraft's point-cloud shader has no transparency (docs/12 §5).";

/// Why a row is greyed.
const NOT_TRANSFERRED: &str = "Not selected / transferred from CSiBridge yet – the agent keeps frames and areas only (docs/08 §4.2).";
const NO_JOINTS: &str = "Planned: joints are transferred, drawing them needs a point layer (docs/12 §2.1).";
const NO_RESTRAINTS: &str = "Planned: needs PointObj.GetRestraint / GetSpring in the agent (docs/08 §4.4).";
const NO_RELEASES: &str = "Planned: needs FrameObj.GetReleases in the agent (docs/12 §2.1).";

/// The categories and their nested subcategories (docs/12 §2.1), in CSiBridge's order.
const CATEGORIES: [(&str, &str, &[&str]); 7] = [
    ("joints", "Joints", &["Labels", "Restraints", "Springs", "Local Axes"]),
    ("frames", "Frames", &["Labels", "Sections", "Releases", "Local Axes"]),
    ("cables", "Cables", &["Labels", "Sections"]),
    ("tendons", "Tendons", &["Labels", "Sections", "Local Axes"]),
    ("areas", "Areas (Shells)", &["Labels", "Sections", "Local Axes"]),
    ("solids", "Solids", &["Labels", "Sections", "Local Axes"]),
    ("links", "Links", &["Labels", "Sections", "Local Axes"]),
];

fn vv_open_id(key: &str) -> egui::Id {
    egui::Id::unique(("factory_vv_open", key))
}

/// Model Categories: object types with their CSiBridge Display Options nested as subcategories
/// (docs/12 §2.1). A category's Visibility = the objects (CSiBridge "Not in View" = unticked);
/// a subcategory's = that annotation. Rows without data are greyed with the reason.
fn model_tab(ui: &mut egui::Ui, s: &mut Settings, g: &crate::transfer::Geometry) {
    ui.horizontal(|ui| {
        let all = |ui: &egui::Ui, open: bool| {
            for (key, ..) in CATEGORIES {
                ui.data_mut(|d| d.insert_temp(vv_open_id(key), open));
            }
        };
        if ui.small_button("Expand All").clicked() {
            all(ui, true);
        }
        if ui.small_button("Collapse All").clicked() {
            all(ui, false);
        }
    });
    let headers = ["Model Category", "Visibility", "Lines", "Surfaces", "Transparency", "Halftone"];
    egui::Grid::new("factory_vv_model").striped(true).num_columns(headers.len()).show(ui, |ui| {
        for h in headers {
            ui.strong(h);
        }
        ui.end_row();
        for (key, name, subs) in CATEGORIES {
            let open_default = matches!(key, "frames" | "areas");
            let mut open = ui.data(|d| d.get_temp::<bool>(vv_open_id(key))).unwrap_or(open_default);
            // The category row
            ui.horizontal(|ui| {
                if ui.small_button(if open { "▼" } else { "▶" }).clicked() {
                    open = !open;
                }
                let count = match key {
                    "frames" => format!("  ({})", g.frames.len()),
                    "areas" => format!("  ({})", g.shells.len()),
                    _ => String::new(),
                };
                ui.label(format!("{name}{count}"));
            });
            ui.data_mut(|d| d.insert_temp(vv_open_id(key), open));
            let category = match key {
                "frames" => Some((&mut s.frames, false)),
                "areas" => Some((&mut s.shells, true)),
                _ => None,
            };
            let why = if key == "joints" { NO_JOINTS } else { NOT_TRANSFERRED };
            match category {
                Some((c, surfaces)) => {
                    ui.checkbox(&mut c.visible, "");
                    egui::widgets::color_picker::color_edit_button_srgb(ui, &mut c.line);
                    if surfaces {
                        egui::widgets::color_picker::color_edit_button_srgb(ui, &mut c.surface);
                        ui.add_enabled(false, egui::Slider::new(&mut 0u8, 0..=100).suffix(" %")).on_disabled_hover_text(NO_TRANSPARENCY);
                    } else {
                        ui.label("");
                        ui.label("");
                    }
                    ui.checkbox(&mut c.halftone, "");
                    ui.end_row();
                    if open {
                        let shown = c.visible;
                        for sub in subs.iter().copied() {
                            ui.label(format!("        {sub}"));
                            let row = match sub {
                                "Labels" => Some(&mut c.labels),
                                "Sections" => Some(&mut c.sections),
                                "Local Axes" => Some(&mut c.axes),
                                _ => None,
                            };
                            match row {
                                // Built: greyed while the category itself is hidden (it keeps its setting).
                                Some(r) => {
                                    ui.add_enabled(shown, egui::Checkbox::new(&mut r.visible, ""));
                                    if sub == "Local Axes" {
                                        ui.label("1 red · 2 green · 3 blue");
                                    } else {
                                        ui.add_enabled_ui(shown, |ui| egui::widgets::color_picker::color_edit_button_srgb(ui, &mut r.colour));
                                    }
                                    ui.label("");
                                    ui.label("");
                                    ui.add_enabled(shown, egui::Checkbox::new(&mut r.halftone, ""));
                                }
                                None => {
                                    let reason = NO_RELEASES;
                                    ui.add_enabled(false, egui::Checkbox::new(&mut false, "")).on_disabled_hover_text(reason);
                                    ui.label("");
                                    ui.label("");
                                    ui.label("");
                                    ui.add_enabled(false, egui::Checkbox::new(&mut false, "")).on_disabled_hover_text(reason);
                                }
                            }
                            ui.end_row();
                        }
                    }
                }
                None => {
                    for _ in 0..5 {
                        ui.add_enabled(false, egui::Checkbox::new(&mut false, "")).on_disabled_hover_text(why);
                    }
                    ui.end_row();
                    if open {
                        for sub in subs.iter().copied() {
                            ui.add_enabled(false, egui::Label::new(format!("        {sub}"))).on_disabled_hover_text(why);
                            let reason = match sub {
                                "Restraints" | "Springs" => NO_RESTRAINTS,
                                _ => why,
                            };
                            ui.add_enabled(false, egui::Checkbox::new(&mut false, "")).on_disabled_hover_text(reason);
                            ui.label("");
                            ui.label("");
                            ui.label("");
                            ui.add_enabled(false, egui::Checkbox::new(&mut false, "")).on_disabled_hover_text(reason);
                            ui.end_row();
                        }
                    }
                }
            }
        }
    });
    ui.weak("Category Visibility unticked = CSiBridge's \"Not in View\"; its subcategories keep their setting. Subcategory Lines = text colour. Contour, legend and values: Results panel.");
}

/// Linked files (browser › Linked): listed; drawing them is planned.
fn imported_tab(ui: &mut egui::Ui) {
    let (ifc, dxf) = crate::model::read(|f| (f.model.linked_ifc.clone(), f.model.linked_dxf.clone()));
    egui::Grid::new("factory_vv_imported").striped(true).num_columns(4).show(ui, |ui| {
        for h in ["Imported Category", "Visibility", "Transparency", "Halftone"] {
            ui.strong(h);
        }
        ui.end_row();
        for (kind, files) in [("IFC", &ifc), ("DXF", &dxf)] {
            ui.label(format!("{kind}  ({})", files.len()));
            for _ in 0..3 {
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "")).on_disabled_hover_text("Planned: drawing linked files (docs/12).");
            }
            ui.end_row();
            for f in files.iter() {
                ui.label(format!("    {f}"));
                ui.end_row();
            }
        }
    });
}

fn filters_tab(ui: &mut egui::Ui, s: &mut Settings, g: &crate::transfer::Geometry) {
    let mut remove = None;
    let sections: Vec<String> = g.shell_sections.iter().chain(&g.frame_sections).cloned().collect();
    egui::Grid::new("factory_vv_filters").striped(true).num_columns(5).show(ui, |ui| {
        for h in ["On", "Rule", "Value", "Action", ""] {
            ui.strong(h);
        }
        ui.end_row();
        for (k, f) in s.filters.iter_mut().enumerate() {
            ui.checkbox(&mut f.enabled, "");
            let rule_name = match f.rule {
                Rule::SectionIs(_) => "Section is",
                Rule::ThicknessBelow(_) => "Thickness <",
                Rule::ThicknessAbove(_) => "Thickness >",
            };
            egui::ComboBox::from_id_salt(("factory_vv_rule", k)).selected_text(rule_name).show_ui(ui, |ui| {
                if ui.selectable_label(matches!(f.rule, Rule::SectionIs(_)), "Section is").clicked() {
                    f.rule = Rule::SectionIs(sections.first().cloned().unwrap_or_default());
                }
                if ui.selectable_label(matches!(f.rule, Rule::ThicknessBelow(_)), "Thickness <").clicked() {
                    f.rule = Rule::ThicknessBelow(0.5);
                }
                if ui.selectable_label(matches!(f.rule, Rule::ThicknessAbove(_)), "Thickness >").clicked() {
                    f.rule = Rule::ThicknessAbove(0.5);
                }
            });
            match &mut f.rule {
                Rule::SectionIs(name) => {
                    egui::ComboBox::from_id_salt(("factory_vv_section", k)).selected_text(name.clone()).show_ui(ui, |ui| {
                        for sec in &sections {
                            ui.selectable_value(name, sec.clone(), sec);
                        }
                    });
                }
                Rule::ThicknessBelow(t) | Rule::ThicknessAbove(t) => {
                    ui.add(egui::DragValue::new(t).speed(0.01).suffix(" m"));
                }
            }
            ui.horizontal(|ui| {
                let action_name = match f.action {
                    Action::Hide => "Hide",
                    Action::Halftone => "Halftone",
                    Action::Colour(_) => "Colour",
                };
                egui::ComboBox::from_id_salt(("factory_vv_action", k)).selected_text(action_name).show_ui(ui, |ui| {
                    if ui.selectable_label(f.action == Action::Hide, "Hide").clicked() {
                        f.action = Action::Hide;
                    }
                    if ui.selectable_label(f.action == Action::Halftone, "Halftone").clicked() {
                        f.action = Action::Halftone;
                    }
                    if ui.selectable_label(matches!(f.action, Action::Colour(_)), "Colour").clicked() {
                        f.action = Action::Colour([230, 120, 40]);
                    }
                });
                if let Action::Colour(c) = &mut f.action {
                    egui::widgets::color_picker::color_edit_button_srgb(ui, c);
                }
            });
            if ui.small_button("✕").clicked() {
                remove = Some(k);
            }
            ui.end_row();
        }
    });
    if let Some(k) = remove {
        s.filters.remove(k);
    }
    if ui.button("Add filter").clicked() {
        s.filters.push(Filter { enabled: true, rule: Rule::SectionIs(sections.first().cloned().unwrap_or_default()), action: Action::Halftone });
    }
    ui.weak("A later filter wins over an earlier one; a hidden category stays hidden (docs/12 §3).");
}
