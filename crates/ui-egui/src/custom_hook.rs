//! [CUSTOM] Fork hook: UI settings made by the `custom/` crates at start-up.
//!
//! This is the only ui-egui file that belongs to the fork. Upstream code knows nothing about the
//! custom crates; the app calls `custom_features::install()` at start-up, which ends up here.
//! Nothing set (tests, plain SolidCraft) = upstream behaviour unchanged.
//! Set: visible toolbar tabs, product name (window, bar, all UI text), product logo.

use std::sync::OnceLock;

static TOOLBAR_TABS: OnceLock<&'static [&'static str]> = OnceLock::new();
static APP_NAME: OnceLock<&'static str> = OnceLock::new();
static LOGO_PNG: OnceLock<&'static [u8]> = OnceLock::new();

/// The product logo as PNG bytes (square, e.g. 256 x 256). First call wins.
pub fn set_logo_png(png: &'static [u8]) {
    let _ = LOGO_PNG.set(png);
}

/// The logo as straight RGBA with its width and height, if one is set and decodes.
pub fn logo_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let image = image::load_from_memory_with_format(LOGO_PNG.get()?, image::ImageFormat::Png).ok()?.into_rgba8();
    let (w, h) = image.dimensions();
    Some((image.into_raw(), w, h))
}

/// Draw the logo `height` px high centred on `c`. False (nothing drawn) while no logo is set,
/// so the caller draws SolidCraft's own mark.
pub(crate) fn draw_logo(p: &egui::Painter, c: egui::Pos2, height: f32) -> bool {
    static TEXTURE: OnceLock<Option<egui::TextureHandle>> = OnceLock::new();
    let texture = TEXTURE.get_or_init(|| {
        let (rgba, w, h) = logo_rgba()?;
        let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        Some(p.ctx().load_texture("custom_logo", image, egui::TextureOptions::LINEAR))
    });
    let Some(texture) = texture else { return false };
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    p.image(texture.id(), egui::Rect::from_center_size(c, egui::vec2(height, height)), uv, egui::Color32::WHITE);
    true
}

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

/// Whether shown text is renamed (a product name other than "SolidCraft" is set).
pub fn renames() -> bool {
    APP_NAME.get().is_some_and(|name| *name != "SolidCraft")
}

/// `text` with "SolidCraft" replaced by the product name (unchanged while nothing is set).
/// Only the capitalised product name is replaced; ids, file names and URLs in lower case stay.
pub fn rename(text: &str) -> std::borrow::Cow<'_, str> {
    match APP_NAME.get() {
        Some(name) if text.contains("SolidCraft") => std::borrow::Cow::Owned(text.replace("SolidCraft", name)),
        _ => std::borrow::Cow::Borrowed(text),
    }
}

/// The product name ("SolidCraft" while nothing is set).
pub fn app_name() -> &'static str {
    APP_NAME.get().copied().unwrap_or("SolidCraft")
}
