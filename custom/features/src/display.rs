//! Opening the results panel and the Visibility/Graphics form (toolbar RESULTS › DISPLAY).
//! Commands set a flag; the UI draws the window while the flag is set.

use std::sync::atomic::{AtomicBool, Ordering};

use custom_bridge::{Ctx, Result, Value, json};

static RESULTS_PANEL: AtomicBool = AtomicBool::new(false);
static VISIBILITY_GRAPHICS: AtomicBool = AtomicBool::new(false);

pub fn results_panel_open() -> bool {
    RESULTS_PANEL.load(Ordering::Relaxed)
}

pub fn set_results_panel(open: bool) {
    RESULTS_PANEL.store(open, Ordering::Relaxed);
}

pub fn visibility_graphics_open() -> bool {
    VISIBILITY_GRAPHICS.load(Ordering::Relaxed)
}

pub fn set_visibility_graphics(open: bool) {
    VISIBILITY_GRAPHICS.store(open, Ordering::Relaxed);
}

/// Command `factory.view.results`: show the results panel.
pub fn results(_ctx: &mut Ctx, _params: &Value) -> Result<Value> {
    set_results_panel(true);
    Ok(json!({ "panel": "results" }))
}

/// Command `factory.view.visibility_graphics`: show the Visibility/Graphics form.
pub fn visibility_graphics(_ctx: &mut Ctx, _params: &Value) -> Result<Value> {
    set_visibility_graphics(true);
    Ok(json!({ "form": "visibility_graphics" }))
}
