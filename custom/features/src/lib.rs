//! [CUSTOM] Your own program. Uses only `custom_bridge`, never `solidcraft_*` crates directly.
//!
//! Add a command: write `fn(&mut Ctx, &Value) -> Result<Value>` in a module, then add one line to
//! `COMMANDS` below. It shows up in the UI command search, the CLI (`solidcraft-cli exec`),
//! scripts and MCP, like any built-in command.

mod plate;

use custom_bridge::commands;

commands! {
    pub static COMMANDS = [
        { id: "custom.plate", label: "Plate with Hole", run: plate::run, icon: "extrude",
          params: "width?: mm (default 80); depth?: mm (default 50); thickness?: mm (default 10); hole?: diameter mm (default 0: none)" },
    ];
}

/// The product name: window title, application bar, every UI text. Single source:
/// `custom/branding/name.txt` (also read by the build for the executable's details).
pub const APP_NAME: &str = include_str!("../../branding/name.txt");

/// The product logo (application bar, window and taskbar icon). The executable's icon is
/// `custom/branding/logo.ico`. Both are made by `custom/branding/make_logo.py`.
#[cfg(feature = "ui")]
pub const LOGO_PNG: &[u8] = include_bytes!("../../branding/logo.png");

/// The only toolbar tabs Factory shows; every other SolidCraft tab is hidden.
#[cfg(feature = "ui")]
pub const TOOLBAR_TABS: &[&str] = &[custom_bridge::names::tabs::MODEL];

/// Register every custom command with SolidCraft. Called once at the start of each app.
pub fn install() {
    custom_bridge::register(COMMANDS);
    #[cfg(feature = "ui")]
    {
        custom_bridge::ui::set_app_name(APP_NAME.trim());
        custom_bridge::ui::set_logo_png(LOGO_PNG);
        custom_bridge::ui::show_toolbar_tabs(TOOLBAR_TABS);
    }
}
