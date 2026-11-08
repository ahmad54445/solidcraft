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
mod ui_status;
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
    ];
}

/// The product name: window title, application bar, every UI text. Single source:
/// `custom/branding/name.txt` (also read by the build for the executable's details).
pub const APP_NAME: &str = include_str!("../../branding/name.txt");

/// The product logo (application bar, window and taskbar icon). The executable's icon is
/// `custom/branding/logo.ico`. Both are made by `custom/branding/make_logo.py`.
#[cfg(feature = "ui")]
pub const LOGO_PNG: &[u8] = include_bytes!("../../branding/logo.png");

/// Factory's own toolbar tabs: connections to analysis programs; result display.
pub const CONNECT_TAB: &str = "CONNECT";
pub const RESULTS_TAB: &str = "RESULTS";

/// The only toolbar tabs Factory shows, in this order; every other SolidCraft tab is hidden.
#[cfg(feature = "ui")]
pub const TOOLBAR_TABS: &[&str] = &[custom_bridge::names::tabs::MODEL, CONNECT_TAB, RESULTS_TAB];

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
        panels: &[custom_bridge::ui::Panel { name: "DISPLAY", commands: &["factory.view.results", "factory.view.visibility_graphics"] }],
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
        custom_bridge::ui::set_logo_png(LOGO_PNG);
        custom_bridge::ui::add_toolbar_tabs(EXTRA_TABS);
        custom_bridge::ui::show_toolbar_tabs(TOOLBAR_TABS);
        custom_bridge::ui::hide_browser_items(HIDDEN_BROWSER_ITEMS);
        custom_bridge::ui::set_browser_rows(ui_browser::rows);
        custom_bridge::ui::set_status_bar(ui_status::draw);
        custom_bridge::view::set_overlay(ui_results::overlay);
    }
}
