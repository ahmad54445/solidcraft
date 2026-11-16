//! Display windows (docs/14-display-windows.md): the 3D view split into up to four tiles, each
//! with its own camera, result, Visibility/Graphics and Hide / Isolate; the selection is shared.
//!
//! The active window works on Factory's ordinary state (the result in `results_view`, the
//! settings in `vv`, the hidden set in `visibility`, the drawn layers in the bridge), so every
//! tool, panel and the Browser act on it unchanged. A window that stops being active keeps that
//! state here ([`Saved`]); the one that becomes active gets its own back. The camera is swapped the
//! same way on SolidCraft's side (`custom_tiles`).

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use custom_bridge::{Ctx, Result, Value, json};

use crate::results_view::{self, Shown, SnapData};
use crate::selection::Set;
use crate::{transfer, vv, visibility};

/// The layouts of the layout picker (docs/14 §1.1), in the picker's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// L5: one view.
    #[default]
    One,
    /// L1: two equal, side by side.
    Two,
    /// L2: big left (⅔), small right (⅓).
    BigSmall,
    /// L3: one left, two stacked right.
    OneTwo,
    /// L4: 2 × 2.
    Four,
}

impl Layout {
    pub const ALL: [Layout; 5] = [Layout::One, Layout::Two, Layout::BigSmall, Layout::OneTwo, Layout::Four];

    /// How many windows it holds.
    pub fn zones(self) -> usize {
        match self {
            Layout::One => 1,
            Layout::Two | Layout::BigSmall => 2,
            Layout::OneTwo => 3,
            Layout::Four => 4,
        }
    }

    /// Its zones as fractions of the area, `[left, top, right, bottom]`, in window order.
    pub fn rects(self) -> Vec<[f32; 4]> {
        match self {
            Layout::One => vec![[0.0, 0.0, 1.0, 1.0]],
            Layout::Two => vec![[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]],
            Layout::BigSmall => vec![[0.0, 0.0, 2.0 / 3.0, 1.0], [2.0 / 3.0, 0.0, 1.0, 1.0]],
            Layout::OneTwo => vec![[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 0.5], [0.5, 0.5, 1.0, 1.0]],
            Layout::Four => vec![[0.0, 0.0, 0.5, 0.5], [0.5, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0], [0.5, 0.5, 1.0, 1.0]],
        }
    }

    /// The command's name for it (`factory.window.layout`).
    pub fn key(self) -> &'static str {
        match self {
            Layout::One => "1",
            Layout::Two => "2",
            Layout::BigSmall => "2big",
            Layout::OneTwo => "1+2",
            Layout::Four => "4",
        }
    }

    pub fn from_key(k: &str) -> Option<Layout> {
        Layout::ALL.into_iter().find(|l| l.key().eq_ignore_ascii_case(k.trim()))
    }

    /// The layout for `n` windows when one is added or closed.
    fn for_count(n: usize, now: Layout) -> Layout {
        match n {
            0 | 1 => Layout::One,
            2 if matches!(now, Layout::BigSmall) => Layout::BigSmall,
            2 => Layout::Two,
            3 => Layout::OneTwo,
            _ => Layout::Four,
        }
    }
}

/// Most windows at once (docs/14 W1).
pub const MAX_WINDOWS: usize = 4;

/// A window's own state while another is active.
#[derive(Clone)]
pub struct Saved {
    pub shown: Shown,
    pub snap: Arc<SnapData>,
    pub settings: vv::Settings,
    pub hidden: Set,
}

impl Saved {
    /// The active window's state now.
    fn current() -> Saved {
        Saved { shown: results_view::shown(), snap: results_view::snap_data(), settings: vv::settings(), hidden: visibility::hidden() }
    }
}

/// One window.
#[derive(Clone)]
pub struct Window {
    pub id: u64,
    /// `None` for the active window (its state is the ordinary one).
    pub saved: Option<Saved>,
}

/// All windows, in zone order.
#[derive(Clone)]
pub struct Windows {
    pub layout: Layout,
    pub windows: Vec<Window>,
    pub active: u64,
    /// The window filling the whole area (docs/14 W4).
    pub maximised: Option<u64>,
    next: u64,
}

impl Default for Windows {
    fn default() -> Self {
        Windows { layout: Layout::One, windows: vec![Window { id: 1, saved: None }], active: 1, maximised: None, next: 2 }
    }
}

impl Windows {
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// The window's number as shown (1-based, zone order).
    pub fn number(&self, id: u64) -> usize {
        self.index_of(id).map_or(0, |k| k + 1)
    }
}

static WINDOWS: LazyLock<RwLock<Windows>> = LazyLock::new(|| RwLock::new(Windows::default()));

/// The windows now.
pub fn current() -> Windows {
    WINDOWS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn write<T>(f: impl FnOnce(&mut Windows) -> T) -> T {
    let out = f(&mut WINDOWS.write().unwrap_or_else(PoisonError::into_inner));
    results_view::repaint();
    out
}

/// A new window, a copy of the active one (docs/14 W7).
fn copy_of_active(w: &mut Windows) -> Window {
    let id = w.next;
    w.next += 1;
    #[cfg(feature = "ui")]
    custom_bridge::view::park(id);
    Window { id, saved: Some(Saved::current()) }
}

/// Make window `id` the active one: the old one keeps its state, the new one gets its own back.
pub fn activate(id: u64) {
    write(|w| activate_in(w, id));
}

fn activate_in(w: &mut Windows, id: u64) {
    if w.active == id {
        return;
    }
    let (Some(old), Some(new)) = (w.index_of(w.active), w.index_of(id)) else { return };
    // The old window keeps what is shown now.
    let (shown, snap) = results_view::park();
    w.windows[old].saved = Some(Saved { shown, snap, settings: vv::settings(), hidden: visibility::hidden() });
    #[cfg(feature = "ui")]
    custom_bridge::view::park(w.active);
    // The new one's own state becomes the ordinary one.
    if let Some(s) = w.windows[new].saved.take() {
        vv::set_quiet(s.settings);
        visibility::set_hidden(s.hidden);
        #[cfg(feature = "ui")]
        let had_layers = custom_bridge::view::unpark(id);
        #[cfg(not(feature = "ui"))]
        let had_layers = true;
        let rebuild = !had_layers && s.shown.units.is_some();
        results_view::unpark(s.shown, s.snap);
        if rebuild {
            results_view::refresh();
        }
    }
    // The selection is shared and stays: an element hidden here but selected in another window
    // stays selected (docs/14 R2).
    w.active = id;
    if w.maximised.is_some() {
        w.maximised = Some(id);
    }
}

/// Split the view into `layout`. With `place` = (window, zone), that window goes to that zone and
/// the others fill the rest in their order (docs/14 P1); missing windows are copies of the active
/// one, windows left over are closed (never the active one).
pub fn set_layout(layout: Layout, place: Option<(u64, usize)>) {
    write(|w| set_layout_in(w, layout, place));
}

fn set_layout_in(w: &mut Windows, layout: Layout, place: Option<(u64, usize)>) {
    let n = layout.zones();
    let mut order: Vec<Window> = std::mem::take(&mut w.windows);
    // The active window and the placed one are kept first when some must close.
    let keep_first = |order: &mut Vec<Window>, id: u64| {
        if let Some(k) = order.iter().position(|x| x.id == id) {
            let x = order.remove(k);
            order.insert(0, x);
        }
    };
    let original = order.clone();
    if order.len() > n {
        if let Some((id, _)) = place {
            keep_first(&mut order, id);
        }
        keep_first(&mut order, w.active);
        for gone in order.split_off(n) {
            #[cfg(feature = "ui")]
            custom_bridge::view::forget(gone.id);
            let _ = gone;
        }
        // Back to zone order among those kept.
        order.sort_by_key(|x| original.iter().position(|o| o.id == x.id));
    }
    w.windows = order;
    while w.windows.len() < n {
        let copy = copy_of_active(w);
        w.windows.push(copy);
    }
    if let Some((id, zone)) = place
        && let Some(k) = w.index_of(id)
    {
        let x = w.windows.remove(k);
        w.windows.insert(zone.min(n - 1), x);
    }
    w.layout = layout;
    if w.maximised.is_some_and(|m| w.index_of(m).is_none()) || n == 1 {
        w.maximised = None;
    }
}

/// Add a window, a copy of the active one (docs/14 W7). `false` at the most (four).
pub fn new_window() -> bool {
    write(|w| {
        let n = w.windows.len();
        if n >= MAX_WINDOWS {
            return false;
        }
        let layout = Layout::for_count(n + 1, w.layout);
        let copy = copy_of_active(w);
        w.windows.push(copy);
        w.layout = layout;
        w.maximised = None;
        true
    })
}

/// Close window `id`; its space goes to the others. The last window cannot be closed (W6).
pub fn close(id: u64) -> bool {
    write(|w| {
        if w.windows.len() <= 1 {
            return false;
        }
        let Some(k) = w.index_of(id) else { return false };
        if w.active == id {
            let next = w.windows[if k + 1 < w.windows.len() { k + 1 } else { k - 1 }].id;
            activate_in(w, next);
        }
        w.windows.remove(k);
        #[cfg(feature = "ui")]
        custom_bridge::view::forget(id);
        w.layout = Layout::for_count(w.windows.len(), w.layout);
        if w.maximised == Some(id) {
            w.maximised = None;
        }
        true
    })
}

/// Maximise window `id` (it becomes active), or restore the layout when it is maximised (W4).
pub fn toggle_maximise(id: u64) {
    write(|w| {
        if w.index_of(id).is_none() {
            return;
        }
        activate_in(w, id);
        w.maximised = if w.maximised == Some(id) { None } else { Some(id) };
    })
}

/// Swap two windows' places (the picker's current arrangement, P2).
pub fn swap(a: u64, b: u64) {
    write(|w| {
        if let (Some(i), Some(j)) = (w.index_of(a), w.index_of(b)) {
            w.windows.swap(i, j);
        }
    })
}

/// After a transfer the element numbers are new: every inactive window forgets Hide / Isolate
/// (R4), keeps its choice (the first load where its own is gone) and is built again for the new
/// model on a worker thread; the active one is built by the transfer itself.
pub fn after_transfer() {
    let loads = results_view::loads();
    let units = crate::model::read(|f| f.units);
    let jobs: Vec<(u64, results_view::Choice, vv::Settings)> = write(|w| {
        let mut jobs = Vec::new();
        for win in &mut w.windows {
            if let Some(s) = win.saved.as_mut() {
                s.hidden = Set::default();
                if s.shown.choice.load.as_ref().is_some_and(|l| !loads.contains(l)) {
                    s.shown.choice.load = loads.first().cloned();
                }
                s.shown.units = Some(units);
                s.shown.busy = true;
                jobs.push((win.id, s.shown.choice.clone(), s.settings.clone()));
            }
        }
        jobs
    });
    for (id, choice, settings) in jobs {
        std::thread::spawn(move || {
            let empty = Set::default();
            let built = results_view::build_hidden(&transfer::geometry(), &transfer::results(), &choice, units, &settings, Some(&empty));
            let mut w = WINDOWS.write().unwrap_or_else(PoisonError::into_inner);
            let Some(s) = w.windows.iter_mut().find(|x| x.id == id).and_then(|x| x.saved.as_mut()) else { return };
            if s.shown.choice != choice || s.settings != settings {
                return; // changed meanwhile (it was activated and built there)
            }
            s.shown.busy = false;
            match built {
                Ok(b) => {
                    #[cfg(feature = "ui")]
                    {
                        let mut ranges = vec![None; b.layers.len()];
                        if let (Some(k), Some(r)) = (b.shell_layer, s.shown.manual_shell) {
                            ranges[k] = Some(r);
                        }
                        if let (Some(k), Some(r)) = (b.frame_layer, s.shown.manual_frame) {
                            ranges[k] = Some(r);
                        }
                        custom_bridge::view::set_parked(id, &b.layers, b.bounds, &ranges);
                        s.shown.layer_count = b.layers.len();
                    }
                    s.snap = Arc::new(b.snap.clone());
                    s.shown.shell = b.shell.map(|l| results_view::Legend { range: s.shown.manual_shell.unwrap_or(l.range), ..l });
                    s.shown.frame = b.frame.map(|l| results_view::Legend { range: s.shown.manual_frame.unwrap_or(l.range), ..l });
                    s.shown.shell_layer = b.shell_layer;
                    s.shown.frame_layer = b.frame_layer;
                    s.shown.envelope = b.envelope;
                    s.shown.error = None;
                }
                Err(e) => s.shown.error = Some(e),
            }
            drop(w);
            results_view::repaint();
        });
    }
}

/// What a window shows, for its title bar: number, load and components.
pub fn title(w: &Windows, id: u64) -> String {
    let shown = match w.windows.iter().find(|x| x.id == id).and_then(|x| x.saved.as_ref()) {
        Some(s) => s.shown.clone(),
        None => results_view::shown(),
    };
    let mut parts = vec![format!("{}", w.number(id))];
    if let Some(load) = &shown.choice.load {
        parts.push(load.clone());
    }
    for l in shown.shell.iter().chain(shown.frame.iter()) {
        let comp = l.title.rsplit(" – ").next().unwrap_or(&l.title);
        parts.push(format!("{comp} [{}]", l.unit));
    }
    parts.join(" · ")
}

// ---- Commands `factory.window.*` (docs/14 R3) ----

fn report() -> Result<Value> {
    let w = current();
    Ok(json!({
        "layout": w.layout.key(),
        "windows": w.windows.iter().map(|x| x.id).collect::<Vec<u64>>(),
        "active": w.number(w.active),
        "maximised": w.maximised.map(|m| w.number(m)),
    }))
}

/// A window named by its number (1-based) in `params.tile`, else the active one.
fn tile_param(params: &Value) -> Result<u64> {
    let w = current();
    match params.get("tile").and_then(Value::as_u64) {
        None => Ok(w.active),
        Some(n) => w.windows.get((n as usize).wrapping_sub(1)).map(|x| x.id).ok_or_else(|| custom_bridge::Error(format!("no window {n} (there are {})", w.windows.len()))),
    }
}

/// Open the layout picker on the next frame (toolbar button without parameters).
static PICKER_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the toolbar asked for the layout picker (read once).
pub fn take_picker_request() -> bool {
    PICKER_ASKED.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// `factory.window.layout`: with `layout` (1, 2, 2big, 1+2, 4) split the view; without, open the
/// layout picker.
pub fn layout_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    match params.get("layout").and_then(Value::as_str) {
        Some(k) => {
            let layout = Layout::from_key(k).ok_or_else(|| custom_bridge::Error(format!("unknown layout {k:?}: 1, 2, 2big, 1+2 or 4")))?;
            set_layout(layout, None);
        }
        None => {
            PICKER_ASKED.store(true, std::sync::atomic::Ordering::Relaxed);
            results_view::repaint();
        }
    }
    report()
}

/// `factory.window.new`: a new window, a copy of the active one.
pub fn new_cmd(_: &mut Ctx, _: &Value) -> Result<Value> {
    if !new_window() {
        return Err(custom_bridge::Error(format!("at most {MAX_WINDOWS} windows")));
    }
    report()
}

/// `factory.window.close`: close window `tile` (number) or the active one.
pub fn close_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    if !close(tile_param(params)?) {
        return Err(custom_bridge::Error("the last window cannot be closed".into()));
    }
    report()
}

/// `factory.window.activate`: make window `tile` (number) active.
pub fn activate_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    activate(tile_param(params)?);
    report()
}

/// `factory.window.maximise`: maximise / restore window `tile` (number) or the active one.
pub fn maximise_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    toggle_maximise(tile_param(params)?);
    report()
}

/// Back to one window (tests, a new session).
pub fn reset() {
    let mut w = WINDOWS.write().unwrap_or_else(PoisonError::into_inner);
    #[cfg(feature = "ui")]
    for x in &w.windows {
        custom_bridge::view::forget(x.id);
    }
    *w = Windows::default();
}
