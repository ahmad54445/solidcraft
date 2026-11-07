//! [CUSTOM] Fork hook: UI settings made by the `custom/` crates at start-up.
//!
//! This is the only ui-egui file that belongs to the fork. Upstream code knows nothing about the
//! custom crates; the app calls `custom_features::install()` at start-up, which ends up here.
//! Nothing set (tests, plain SolidCraft) = upstream behaviour unchanged.

use std::sync::OnceLock;

static TOOLBAR_TABS: OnceLock<&'static [&'static str]> = OnceLock::new();
static APP_NAME: OnceLock<&'static str> = OnceLock::new();

/// Keep only these toolbar tabs (e.g. `["MODEL"]`). Every other tab disappears from the ribbon
/// and from the toolbar menu. First call wins.
pub fn set_toolbar_tabs(tabs: &'static [&'static str]) {
    let _ = TOOLBAR_TABS.set(tabs);
}

/// Whether a toolbar tab may be shown at all (true for every tab while nothing is set).
pub fn tab_allowed(tab: &str) -> bool {
    TOOLBAR_TABS.get().is_none_or(|tabs| tabs.contains(&tab))
}

/// The product name shown in the window title and the application bar. First call wins.
pub fn set_app_name(name: &'static str) {
    let _ = APP_NAME.set(name);
}

/// The product name ("SolidCraft" while nothing is set).
pub fn app_name() -> &'static str {
    APP_NAME.get().copied().unwrap_or("SolidCraft")
}
