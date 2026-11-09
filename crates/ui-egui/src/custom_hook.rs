//! [CUSTOM] Fork hook: UI settings made by the `custom/` crates at start-up.
//!
//! This is the only ui-egui file that belongs to the fork. Upstream code knows nothing about the
//! custom crates; the app calls `custom_features::install()` at start-up, which ends up here.
//! Nothing set (tests, plain SolidCraft) = upstream behaviour unchanged.
//! Set: visible toolbar tabs, product name (window, bar, all UI text), product logo, extra
//! toolbar tabs, hidden browser folders, the fork's browser rows after Origin, an extra status bar.

use std::sync::OnceLock;

static TOOLBAR_TABS: OnceLock<&'static [&'static str]> = OnceLock::new();
static APP_NAME: OnceLock<&'static str> = OnceLock::new();
static LOGO_PNG: OnceLock<&'static [u8]> = OnceLock::new();
static EXTRA_TABS: OnceLock<(&'static [(&'static str, &'static str)], &'static [crate::workspace::Section])> = OnceLock::new();
static HIDDEN_BROWSER: OnceLock<&'static [&'static str]> = OnceLock::new();

/// Extra toolbar tabs `(name, icon)` after SolidCraft's own, with their panels. First call wins.
pub fn add_toolbar_tabs(tabs: &'static [(&'static str, &'static str)], sections: &'static [crate::workspace::Section]) {
    let _ = EXTRA_TABS.set((tabs, sections));
}

/// The extra tab names, in order.
pub fn extra_tabs() -> impl Iterator<Item = &'static str> {
    EXTRA_TABS.get().map_or(&[][..], |(tabs, _)| tabs).iter().map(|(name, _)| *name)
}

/// The icon of an extra tab.
pub fn extra_tab_icon(tab: &str) -> Option<&'static str> {
    EXTRA_TABS.get()?.0.iter().find(|(name, _)| *name == tab).map(|(_, icon)| *icon)
}

/// The panels of the extra tabs.
pub fn extra_sections() -> &'static [crate::workspace::Section] {
    EXTRA_TABS.get().map_or(&[], |(_, sections)| sections)
}

/// Browser folders to leave out (e.g. `["origin"]`). First call wins.
pub fn hide_browser_items(items: &'static [&'static str]) {
    let _ = HIDDEN_BROWSER.set(items);
}

/// Whether a browser folder is left out.
pub fn browser_hidden(item: &str) -> bool {
    HIDDEN_BROWSER.get().is_some_and(|items| items.contains(&item))
}

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

static CREDITS: OnceLock<&'static str> = OnceLock::new();

/// Who made the product, shown in the About window under the description. First call wins.
pub fn set_about_credits(text: &'static str) {
    let _ = CREDITS.set(text);
}

/// The About window's credits (none while nothing is set).
pub fn about_credits() -> Option<&'static str> {
    CREDITS.get().copied().filter(|t| !t.is_empty())
}

/// The product name ("SolidCraft" while nothing is set).
pub fn app_name() -> &'static str {
    APP_NAME.get().copied().unwrap_or("SolidCraft")
}

/// A part of the window the fork draws itself: called once per frame with the area's `Ui`.
pub type DrawFn = fn(&mut egui::Ui);

/// Rows the fork adds to the browser's root after Origin, at the given depth.
pub type RowsFn = fn(&mut egui::Ui, usize);

static BROWSER_ROWS: OnceLock<RowsFn> = OnceLock::new();
static STATUS_BAR: OnceLock<DrawFn> = OnceLock::new();

/// Add the fork's rows to the browser's root, right after Origin. First call wins.
pub fn set_browser_rows(rows: RowsFn) {
    let _ = BROWSER_ROWS.set(rows);
}

/// The fork's browser rows, if set.
pub fn browser_rows() -> Option<RowsFn> {
    BROWSER_ROWS.get().copied()
}

/// An extra status bar the fork draws, always shown along the window's bottom edge (SolidCraft's
/// own bar is added above it while its preference is on). First call wins.
pub fn set_status_bar(draw: DrawFn) {
    let _ = STATUS_BAR.set(draw);
}

/// The fork's status bar, if set.
pub fn status_bar() -> Option<DrawFn> {
    STATUS_BAR.get().copied()
}

/// What a [`tree_row`] reported this frame.
pub struct TreeRow {
    /// The row was clicked (primary button).
    pub clicked: bool,
    /// The fold arrow was clicked.
    pub toggled: bool,
    /// The row was right-clicked here.
    pub context_at: Option<egui::Pos2>,
}

/// One browser row drawn exactly like SolidCraft's: `open` is `Some` for rows that fold,
/// `count` is the text at the row's end. `key` must be unique among the rows.
pub fn tree_row(ui: &mut egui::Ui, key: &str, depth: usize, open: Option<bool>, icon: &str, label: &str, count: &str) -> TreeRow {
    let row = crate::ui_kit::Row { depth, fold: open, icon, label, name: true, count, ..Default::default() };
    let out = crate::browser::draw_row(ui, ui.scope_id().with(("custom_row", key)), &row);
    TreeRow { clicked: out.clicked, toggled: out.fold, context_at: out.secondary }
}

// ---- 3D view: the fork's own elements and their overlay (docs/10-viewport-results.md) ----

static EXTRA_CLOUDS: std::sync::RwLock<Vec<solidcraft_gpu::view::GpuCloud>> = std::sync::RwLock::new(Vec::new());
static EXTRA_BOUNDS: std::sync::RwLock<Option<[[f64; 3]; 2]>> = std::sync::RwLock::new(None);
/// The view as the overlay sees it this frame.
pub struct ViewInfo<'a> {
    /// World point (m) → screen position, `None` behind the camera.
    pub project: &'a dyn Fn([f64; 3]) -> Option<egui::Pos2>,
    /// Camera position (m), for "closest to the viewer".
    pub eye: [f64; 3],
}

/// What the fork draws over the 3D view.
pub type OverlayFn = fn(&mut egui::Ui, egui::Rect, &ViewInfo<'_>);

static VIEWPORT_OVERLAY: OnceLock<OverlayFn> = OnceLock::new();

/// Clouds (segments and triangles with values per corner) drawn with the model, in the same depth,
/// section cut and colour ramp as SolidCraft's point clouds. Replaces the previous set; their
/// records are shared (`Arc`), so this and [`extra_clouds`] copy no geometry.
pub fn set_extra_clouds(clouds: Vec<solidcraft_gpu::view::GpuCloud>) {
    *EXTRA_CLOUDS.write().unwrap_or_else(std::sync::PoisonError::into_inner) = clouds;
}

/// The fork's clouds for this frame (empty while none are set).
pub fn extra_clouds() -> Vec<solidcraft_gpu::view::GpuCloud> {
    EXTRA_CLOUDS.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// The box of the fork's elements (min, max), so Fit view includes them.
pub fn set_extra_bounds(bounds: Option<[[f64; 3]; 2]>) {
    *EXTRA_BOUNDS.write().unwrap_or_else(std::sync::PoisonError::into_inner) = bounds;
}

pub fn extra_bounds() -> Option<[[f64; 3]; 2]> {
    *EXTRA_BOUNDS.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Drawn over the 3D view every frame (e.g. a result legend), with the view's free area. First call wins.
pub fn set_viewport_overlay(draw: OverlayFn) {
    let _ = VIEWPORT_OVERLAY.set(draw);
}

pub fn viewport_overlay() -> Option<OverlayFn> {
    VIEWPORT_OVERLAY.get().copied()
}
