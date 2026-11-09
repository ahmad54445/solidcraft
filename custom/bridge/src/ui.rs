//! [CUSTOM] Desktop UI settings (feature `ui`): what the app shows and what it is called.

/// egui, re-exported so Factory's UI code always uses the version SolidCraft is built with.
pub use egui;

pub use solidcraft_ui_egui::custom_hook::{DrawFn, RowsFn, TreeRow};

/// Add Factory's rows to the browser's root right after Origin (`rows(ui, depth)`). Call once.
pub fn set_browser_rows(rows: RowsFn) {
    solidcraft_ui_egui::custom_hook::set_browser_rows(rows);
}

/// Factory's status bar along the window's bottom edge, always shown. Call once.
pub fn set_status_bar(draw: DrawFn) {
    solidcraft_ui_egui::custom_hook::set_status_bar(draw);
}

/// One browser row drawn like SolidCraft's (`open: Some(..)` for rows that fold, `count` at the
/// row's end). `key` must be unique among the rows. Icons: SolidCraft's UI icon names.
pub fn tree_row(ui: &mut egui::Ui, key: &str, depth: usize, open: Option<bool>, icon: &str, label: &str, count: &str) -> TreeRow {
    solidcraft_ui_egui::custom_hook::tree_row(ui, key, depth, open, icon, label, count)
}

/// Show only these toolbar tabs (names from [`crate::names::tabs`]); every other tab is hidden
/// from the ribbon and from the toolbar menu. Call once at start-up, before the window opens.
pub fn show_toolbar_tabs(tabs: &'static [&'static str]) {
    solidcraft_ui_egui::custom_hook::set_toolbar_tabs(tabs);
}

/// Who made the product, shown in Help › About. Call once at start-up.
pub fn set_about_credits(text: &'static str) {
    solidcraft_ui_egui::custom_hook::set_about_credits(text);
}

/// The product name shown in the window title and the application bar.
pub fn set_app_name(name: &'static str) {
    solidcraft_ui_egui::custom_hook::set_app_name(name);
}

/// The product logo (PNG bytes, square) in the application bar and as the window icon.
pub fn set_logo_png(png: &'static [u8]) {
    solidcraft_ui_egui::custom_hook::set_logo_png(png);
}

/// A panel of an extra toolbar tab: its name and the command ids on it, in order.
pub struct Panel {
    pub name: &'static str,
    pub commands: &'static [&'static str],
}

/// An extra toolbar tab: its name (upper case, like SolidCraft's: "CONNECT"), icon and panels.
pub struct Tab {
    pub name: &'static str,
    pub icon: &'static str,
    pub panels: &'static [Panel],
}

/// Add toolbar tabs after SolidCraft's own. To show them, list them in `show_toolbar_tabs` too.
/// Call once at start-up.
pub fn add_toolbar_tabs(tabs: &'static [Tab]) {
    use solidcraft_ui_egui::workspace::Section;
    let names: Vec<(&'static str, &'static str)> = tabs.iter().map(|t| (t.name, t.icon)).collect();
    let sections: Vec<Section> = tabs
        .iter()
        .flat_map(|t| t.panels.iter().map(move |p| Section { tab: t.name, panel: p.name, promoted: p.commands.len(), ids: p.commands }))
        .collect();
    // Start-up data that lives as long as the app.
    solidcraft_ui_egui::custom_hook::add_toolbar_tabs(Box::leak(names.into_boxed_slice()), Box::leak(sections.into_boxed_slice()));
}

/// Browser folders to leave out (names from [`crate::names::browser`]). Call once at start-up.
pub fn hide_browser_items(items: &'static [&'static str]) {
    solidcraft_ui_egui::custom_hook::hide_browser_items(items);
}

/// The toolbar tabs SolidCraft has (before any filter), for the upstream contract test.
pub fn upstream_toolbar_tabs() -> Vec<&'static str> {
    solidcraft_ui_egui::workspace::TABS.iter().copied().chain(solidcraft_ui_egui::workspace::SPECIALTY_TABS.iter().map(|t| t.tab)).collect()
}
