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
    Annotation,
    Results,
    Filters,
    Linked,
}

fn visibility_graphics(ctx: &egui::Context) {
    // The form edits a copy; OK / Apply commit it in one step (rule G7).
    let edit_id = egui::Id::unique("factory_vv_edit");
    let tab_id = egui::Id::unique("factory_vv_tab");
    let mut edit: Settings = ctx.data(|d| d.get_temp::<Settings>(edit_id)).unwrap_or_else(vv::settings);
    let mut tab = ctx.data(|d| d.get_temp::<u8>(tab_id)).map_or(Tab::Model, |t| match t {
        1 => Tab::Annotation,
        2 => Tab::Results,
        3 => Tab::Filters,
        4 => Tab::Linked,
        _ => Tab::Model,
    });
    let geometry = crate::transfer::geometry();
    let mut open = true;
    let mut close = false;
    egui::Window::new("Visibility/Graphics").id(egui::Id::unique("factory_vv")).open(&mut open).resizable(true).default_width(620.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (t, name) in [(Tab::Model, "Model"), (Tab::Annotation, "Annotation"), (Tab::Results, "Results"), (Tab::Filters, "Filters"), (Tab::Linked, "Linked")] {
                if ui.selectable_label(tab == t, name).clicked() {
                    tab = t;
                }
            }
        });
        ui.separator();
        match tab {
            Tab::Model => model_tab(ui, &mut edit),
            Tab::Annotation => {
                ui.checkbox(&mut edit.legend, "Legend");
                ui.checkbox(&mut edit.values, "Value labels");
                planned(ui, "Node numbers", "Planned (docs/12 §2).");
                planned(ui, "Element numbers", "Planned (docs/12 §2).");
                planned(ui, "Section names", "Planned (docs/12 §2).");
            }
            Tab::Results => {
                ui.checkbox(&mut edit.contour, "Contour");
                planned(ui, "Deform", "Planned: needs displacements (docs/10 §7.4).");
                planned(ui, "Vectors", "Planned (docs/10 §7.2).");
            }
            Tab::Filters => filters_tab(ui, &mut edit, &geometry),
            Tab::Linked => {
                let (ifc, dxf) = crate::model::read(|f| (f.model.linked_ifc.clone(), f.model.linked_dxf.clone()));
                if ifc.is_empty() && dxf.is_empty() {
                    ui.weak("No linked files (browser › Linked).");
                }
                for name in ifc.iter().chain(&dxf) {
                    ui.label(name);
                }
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
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

fn model_tab(ui: &mut egui::Ui, s: &mut Settings) {
    egui::Grid::new("factory_vv_model").striped(true).num_columns(6).show(ui, |ui| {
        for h in ["Category", "Visible", "Lines", "Surfaces", "Transparency", "Halftone"] {
            ui.strong(h);
        }
        ui.end_row();
        for (name, c, surfaces) in [("Shells", &mut s.shells, true), ("Frames", &mut s.frames, false)] {
            ui.label(name);
            ui.checkbox(&mut c.visible, "");
            egui::widgets::color_picker::color_edit_button_srgb(ui, &mut c.line);
            if surfaces {
                egui::widgets::color_picker::color_edit_button_srgb(ui, &mut c.surface);
            } else {
                ui.label("");
            }
            ui.add_enabled(false, egui::Slider::new(&mut 0u8, 0..=100).suffix(" %"))
                .on_disabled_hover_text("Planned: SolidCraft's point-cloud shader has no transparency (docs/12 §5).");
            ui.checkbox(&mut c.halftone, "");
            ui.end_row();
        }
    });
    ui.weak("Halftone and filter colours show the elements plain (without contour). Line widths and transparency: docs/12 §5.");
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
