//! [CUSTOM] Your own program. Uses only `custom_bridge`, never `solidcraft_*` crates directly.
//!
//! Add a command: write `fn(&mut Ctx, &Value) -> Result<Value>` in a module, then add one line to
//! `COMMANDS` below. It shows up in the UI command search, the CLI (`solidcraft-cli exec`),
//! scripts and MCP, like any built-in command.

mod connect;
pub mod csi;
pub mod display;
pub mod vv;
pub mod transfer;
pub mod results_view;
pub mod model;
pub mod sdf;
pub mod select_cmd;
pub mod selection;
pub mod visibility;
pub mod windows;
mod plate;
#[cfg(feature = "ui")]
mod ui_browser;
#[cfg(feature = "ui")]
mod ui_csi;
#[cfg(feature = "ui")]
mod ui_panels;
#[cfg(feature = "ui")]
mod ui_results;
#[cfg(feature = "ui")]
mod ui_select;
#[cfg(feature = "ui")]
mod ui_status;
#[cfg(feature = "ui")]
mod ui_windows;
pub mod units;

use custom_bridge::commands;

commands! {
    pub static COMMANDS = [
        { id: "custom.plate", label: "Plate with Hole", run: plate::run, icon: "extrude",
          params: "width?: mm (default 80); depth?: mm (default 50); thickness?: mm (default 10); hole?: diameter mm (default 0: none)" },
        { id: "factory.connect.midas", label: "MIDAS Civil NX", run: connect::midas, icon: "import",
          planned: "MIDAS adapter" },
        { id: "factory.connect.csi", label: "CSiBridge", run: connect::csi, icon: "import" },
        { id: "factory.view.results", label: "Results", run: display::results, icon: "graph" },
        { id: "factory.view.visibility_graphics", label: "Visibility/Graphics", run: display::visibility_graphics, icon: "eye" },
        // Display windows (docs/14-display-windows.md)
        { id: "factory.window.layout", label: "Window Layout", run: windows::layout_cmd, icon: "grid",
          params: "layout?: 1 | 2 | 2big | 1+2 | 4 (no parameters: opens the layout picker)" },
        { id: "factory.window.new", label: "New Window", run: windows::new_cmd, icon: "feature_group" },
        { id: "factory.window.close", label: "Close Window", run: windows::close_cmd, icon: "delete", params: "tile?: window number (default the active one)" },
        { id: "factory.window.activate", label: "Activate Window", run: windows::activate_cmd, icon: "select", params: "tile: window number" },
        { id: "factory.window.maximise", label: "Maximise Window", run: windows::maximise_cmd, icon: "grid", params: "tile?: window number (default the active one)" },
        { id: "factory.window.templates", label: "View Templates", run: windows::templates_cmd, icon: "folder" },
        { id: "factory.window.save_template", label: "Save View Template", run: windows::save_template_cmd, icon: "folder",
          params: "name: template name; tile?: window number (default the active one)" },
        { id: "factory.window.apply_template", label: "Apply View Template", run: windows::apply_template_cmd, icon: "folder",
          params: "name: template name; tile?: window number (default the active one)" },
        { id: "factory.window.delete_template", label: "Delete View Template", run: windows::delete_template_cmd, icon: "delete", params: "name: template name" },
        // Named views (docs/15-named-views.md)
        { id: "factory.view.new", label: "New Named View", run: windows::view_new_cmd, icon: "perspective" },
        { id: "factory.view.open", label: "Open Named View", run: windows::view_open_cmd, icon: "perspective", params: "view: view name; tile?: window number (default the active one)" },
        { id: "factory.view.rename", label: "Rename Named View", run: windows::view_rename_cmd, icon: "perspective", params: "view?: view name (default the active one); name: new name" },
        { id: "factory.view.duplicate", label: "Duplicate Named View", run: windows::view_duplicate_cmd, icon: "perspective", params: "view?: view name (default the active one)" },
        { id: "factory.view.delete", label: "Delete Named View", run: windows::view_delete_cmd, icon: "delete", params: "view: view name (not while a window shows it)" },
        { id: "factory.view.set_template", label: "Set View Template", run: windows::view_set_template_cmd, icon: "folder",
          params: "view?: view name (default the active one); template?: template name (none: detach)" },
        { id: "factory.view.set_named_template", label: "Template of a Named View", run: windows::view_set_named_template_cmd, icon: "folder",
          params: "view: Top, Front, Right, Home or a saved view; template?: template name (none: no template)" },
        { id: "factory.template.rename", label: "Rename View Template", run: windows::template_rename_cmd, icon: "folder", params: "template: name; name: new name" },
        { id: "factory.template.include", label: "View Template Parts", run: windows::template_include_cmd, icon: "folder",
          params: "template: name; vg?, result?, camera?, hidden?: bool (the parts it rules)" },
        { id: "factory.template.edit", label: "Edit View Template", run: windows::template_edit_cmd, icon: "folder", params: "on?: bool (toggles without)" },
        // Selection tools (docs/13-selection.md)
        { id: "factory.select.pointer", label: "Pointer/Window", run: select_cmd::pointer, icon: "select" },
        { id: "factory.select.poly", label: "Poly", run: select_cmd::poly, icon: "polygon" },
        { id: "factory.select.intersecting_poly", label: "Intersecting Poly", run: select_cmd::intersecting_poly, icon: "polygon_edge" },
        { id: "factory.select.intersecting_line", label: "Intersecting Line", run: select_cmd::intersecting_line, icon: "line" },
        { id: "factory.select.coordinates", label: "Coordinate Specification", run: select_cmd::coordinates, icon: "grid",
          params: "x?, y?, z?: [min, max] m; tolerance?: m (no parameters: opens the dialog)" },
        { id: "factory.select.parallel", label: "Lines Parallel To", run: select_cmd::parallel, icon: "c_parallel",
          params: "axis?: X|Y|Z; plane?: XY|XZ|YZ; tolerance?: degrees (default 1)" },
        { id: "factory.select.properties", label: "Properties", run: select_cmd::properties, icon: "filter",
          params: "shell_sections?: [names]; frame_sections?: [names]; thickness?: [min, max] m" },
        { id: "factory.select.assignments", label: "Assignments", run: select_cmd::assignments, icon: "section",
          planned: "assignments are not transferred yet (docs/13 §7)" },
        { id: "factory.select.groups", label: "Groups", run: select_cmd::groups, icon: "folder", params: "names: [group names]" },
        { id: "factory.select.labels", label: "Labels", run: select_cmd::labels, icon: "select_parts", params: "text: names, ranges 101-150, wildcards A1*" },
        { id: "factory.select.all", label: "All", run: select_cmd::all, icon: "contact_all" },
        { id: "factory.select.invert", label: "Invert", run: select_cmd::invert, icon: "select_bodies" },
        { id: "factory.select.clear", label: "Clear Selection", run: select_cmd::clear, icon: "select_sketch" },
        { id: "factory.select.previous", label: "Previous Selection", run: select_cmd::previous, icon: "undo" },
        { id: "factory.select.deselect", label: "Deselect Mode", run: select_cmd::deselect, icon: "select_components", params: "on?: bool (toggles without)" },
        { id: "factory.select.kinds", label: "Select Kinds", run: select_cmd::kinds, icon: "select_features", params: "shells?: bool; frames?: bool" },
        { id: "factory.select.save_group", label: "Save as Group", run: select_cmd::save_group, icon: "feature_group", params: "name?: group name" },
        { id: "factory.select.hide", label: "Hide", run: select_cmd::hide, icon: "suppress" },
        { id: "factory.select.isolate", label: "Isolate", run: select_cmd::isolate, icon: "select_bodies" },
        { id: "factory.select.show_all", label: "Show All", run: select_cmd::show_all, icon: "eye" },
    ];
}

/// The product name: window title, application bar, every UI text. Single source:
/// `custom/branding/name.txt` (also read by the build for the executable's details).
pub const APP_NAME: &str = include_str!("../../branding/name.txt");
/// Who programmed Factory, shown in Help › About: `custom/branding/credits.txt`.
pub const CREDITS: &str = include_str!("../../branding/credits.txt");

/// The product logo (application bar, window and taskbar icon). The executable's icon is
/// `custom/branding/logo.ico`. Both are made by `custom/branding/make_logo.py`.
#[cfg(feature = "ui")]
pub const LOGO_PNG: &[u8] = include_bytes!("../../branding/logo.png");

/// Factory's own toolbar tabs: connections to analysis programs; result display.
pub const CONNECT_TAB: &str = "CONNECT";
pub const RESULTS_TAB: &str = "RESULTS";
pub const SELECT_TAB: &str = "SELECT";

/// The only toolbar tabs Factory shows, in this order; every other SolidCraft tab is hidden.
#[cfg(feature = "ui")]
pub const TOOLBAR_TABS: &[&str] = &[custom_bridge::names::tabs::MODEL, CONNECT_TAB, RESULTS_TAB, SELECT_TAB];

/// Tabs Factory adds to the toolbar.
#[cfg(feature = "ui")]
static EXTRA_TABS: &[custom_bridge::ui::Tab] = &[
    custom_bridge::ui::Tab {
        name: CONNECT_TAB,
        icon: "import",
        panels: &[custom_bridge::ui::Panel { name: "PROGRAMS", commands: &["factory.connect.midas", "factory.connect.csi"] }],
    },
    custom_bridge::ui::Tab {
        name: RESULTS_TAB,
        icon: "graph",
        panels: &[
            custom_bridge::ui::Panel { name: "DISPLAY", commands: &["factory.view.results", "factory.view.visibility_graphics"] },
            // Display windows (docs/14): layout picker, new, close, maximise.
            custom_bridge::ui::Panel { name: "WINDOWS", commands: &["factory.window.layout", "factory.window.new", "factory.window.close", "factory.window.maximise", "factory.window.templates"] },
        ],
    },
    // Selection tools (docs/13-selection.md): screen tools, by data, the selection itself.
    custom_bridge::ui::Tab {
        name: SELECT_TAB,
        icon: "select",
        panels: &[
            custom_bridge::ui::Panel {
                name: "SCREEN",
                commands: &["factory.select.pointer", "factory.select.poly", "factory.select.intersecting_poly", "factory.select.intersecting_line"],
            },
            custom_bridge::ui::Panel {
                name: "BY DATA",
                commands: &[
                    "factory.select.coordinates",
                    "factory.select.parallel",
                    "factory.select.properties",
                    "factory.select.assignments",
                    "factory.select.groups",
                    "factory.select.labels",
                    "factory.select.all",
                ],
            },
            custom_bridge::ui::Panel {
                name: "SELECTION",
                commands: &["factory.select.invert", "factory.select.clear", "factory.select.previous", "factory.select.deselect", "factory.select.save_group"],
            },
            custom_bridge::ui::Panel { name: "VISIBILITY", commands: &["factory.select.hide", "factory.select.isolate", "factory.select.show_all"] },
        ],
    },
];

/// SolidCraft browser rows Factory leaves out (README › Browser): bodies, sketches and the other
/// modelling folders. Document Settings, Named Views and Origin stay; Factory's own branches
/// (Structure, Linked, Design, Groups) follow Origin.
#[cfg(feature = "ui")]
pub const HIDDEN_BROWSER_ITEMS: &[&str] = {
    use custom_bridge::names::browser::*;
    &[BODIES, SKETCHES, CONSTRUCTION, CANVASES, POINT_CLOUDS, ANALYSIS, FEATURES, JOINTS, COMPONENTS]
};

/// Register every custom command with SolidCraft. Called once at the start of each app.
pub fn install() {
    custom_bridge::register(COMMANDS);
    #[cfg(feature = "ui")]
    {
        custom_bridge::ui::set_app_name(APP_NAME.trim());
        custom_bridge::ui::set_about_credits(CREDITS.trim());
        custom_bridge::ui::set_logo_png(LOGO_PNG);
        custom_bridge::ui::add_toolbar_tabs(EXTRA_TABS);
        custom_bridge::ui::show_toolbar_tabs(TOOLBAR_TABS);
        custom_bridge::ui::hide_browser_items(HIDDEN_BROWSER_ITEMS);
        custom_bridge::ui::set_browser_rows(ui_browser::rows);
        custom_bridge::ui::set_status_bar(ui_status::draw);
        custom_bridge::view::set_overlay(ui_results::overlay);
        custom_bridge::view::set_tiles(ui_windows::layout, ui_windows::overlay);
        custom_bridge::ui::set_named_view_rows(ui_windows::named_view_rows); // docs/15
        custom_bridge::ui::set_named_view_hooks(ui_windows::named_view_items, windows::named_view_opened); // docs/15 §10
        windows::keep_between_sessions(); // the windows and view templates of the last session (docs/14 §10.4)
    }
}
