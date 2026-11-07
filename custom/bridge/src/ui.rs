//! [CUSTOM] Desktop UI settings (feature `ui`): what the app shows and what it is called.

/// Show only these toolbar tabs (names from [`crate::names::tabs`]); every other tab is hidden
/// from the ribbon and from the toolbar menu. Call once at start-up, before the window opens.
pub fn show_toolbar_tabs(tabs: &'static [&'static str]) {
    solidcraft_ui_egui::custom_hook::set_toolbar_tabs(tabs);
}

/// The product name in the window title and the application bar. Call once at start-up.
pub fn set_app_name(name: &'static str) {
    solidcraft_ui_egui::custom_hook::set_app_name(name);
}

/// The product logo (PNG bytes, square) in the application bar and as the window icon.
pub fn set_logo_png(png: &'static [u8]) {
    solidcraft_ui_egui::custom_hook::set_logo_png(png);
}

/// The toolbar tabs SolidCraft has (before any filter), for the upstream contract test.
pub fn upstream_toolbar_tabs() -> Vec<&'static str> {
    solidcraft_ui_egui::workspace::TABS.iter().copied().chain(solidcraft_ui_egui::workspace::SPECIALTY_TABS.iter().map(|t| t.tab)).collect()
}
