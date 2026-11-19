//! Display windows (docs/14-display-windows.md): the 3D view split into up to four tiles, each
//! with its own camera, result, Visibility/Graphics and Hide / Isolate; the selection is shared.
//!
//! The active window works on Factory's ordinary state (the result in `results_view`, the
//! settings in `vv`, the hidden set in `visibility`, the drawn layers in the bridge), so every
//! tool, panel and the Browser act on it unchanged. A window that stops being active keeps that
//! state here ([`Saved`]); the one that becomes active gets its own back. The camera is swapped the
//! same way on SolidCraft's side (`custom_tiles`).

use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError, RwLock};

use custom_bridge::{Ctx, Result, Value, json};

use crate::results_view::{self, Shown, SnapData};
use crate::selection::{Bits, Set};
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

    /// Its zones as fractions of the area, `[left, top, right, bottom]`, in window order, with
    /// the borders where the layout puts them.
    pub fn rects(self) -> Vec<[f32; 4]> {
        self.rects_with(self.default_splits())
    }

    /// Where its borders are at first: the vertical one (x) and the horizontal one (y), as
    /// fractions of the area (docs/14 §10.2).
    pub fn default_splits(self) -> [f32; 2] {
        match self {
            Layout::BigSmall => [2.0 / 3.0, 0.5],
            _ => [0.5, 0.5],
        }
    }

    /// Which borders it has: (vertical, horizontal over the full width, horizontal right only).
    pub fn borders(self) -> (bool, bool, bool) {
        match self {
            Layout::One => (false, false, false),
            Layout::Two | Layout::BigSmall => (true, false, false),
            Layout::OneTwo => (true, false, true),
            Layout::Four => (true, true, false),
        }
    }

    /// Its zones with the borders at `[x, y]` (each kept within 15 %–85 %).
    pub fn rects_with(self, splits: [f32; 2]) -> Vec<[f32; 4]> {
        let [x, y] = splits.map(|v| if v.is_finite() { v.clamp(MIN_SPLIT, 1.0 - MIN_SPLIT) } else { 0.5 });
        match self {
            Layout::One => vec![[0.0, 0.0, 1.0, 1.0]],
            Layout::Two | Layout::BigSmall => vec![[0.0, 0.0, x, 1.0], [x, 0.0, 1.0, 1.0]],
            Layout::OneTwo => vec![[0.0, 0.0, x, 1.0], [x, 0.0, 1.0, y], [x, y, 1.0, 1.0]],
            Layout::Four => vec![[0.0, 0.0, x, y], [x, 0.0, 1.0, y], [0.0, y, x, 1.0], [x, y, 1.0, 1.0]],
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
/// The narrowest a window gets when a border is dragged (fraction of the area, docs/14 §10.2).
pub const MIN_SPLIT: f32 = 0.15;

/// A window's own state while another is active.
#[derive(Clone)]
pub struct Saved {
    pub shown: Shown,
    pub snap: Arc<SnapData>,
    pub settings: vv::Settings,
    pub hidden: Set,
    /// Hidden elements by name, applied when a model is there (a template or the saved layout
    /// given before the transfer, docs/14 §10).
    pub hidden_names: Option<(Vec<String>, Vec<String>)>,
}

impl Saved {
    /// The active window's state now.
    fn current() -> Saved {
        Saved { shown: results_view::shown(), snap: results_view::snap_data(), settings: vv::settings(), hidden: visibility::hidden(), hidden_names: None }
    }
}

/// One window.
#[derive(Clone)]
pub struct Window {
    pub id: u64,
    /// `None` for the active window (its state is the ordinary one).
    pub saved: Option<Saved>,
    /// The named view it shows (docs/15 N1).
    pub view: String,
}

/// All windows, in zone order.
#[derive(Clone)]
pub struct Windows {
    pub layout: Layout,
    pub windows: Vec<Window>,
    pub active: u64,
    /// The window filling the whole area (docs/14 W4).
    pub maximised: Option<u64>,
    /// The borders: vertical (x) and horizontal (y), fractions of the area (§10.2).
    pub splits: [f32; 2],
    /// Every named view, shown in a window or not (docs/15).
    pub views: Vec<NamedView>,
    /// SolidCraft's named views (standard Top / Front / Right / Home and the design's saved views)
    /// with the template each has: (view name, template) (docs/15 §10).
    pub named_templates: Vec<(String, String)>,
    next: u64,
}

impl Default for Windows {
    fn default() -> Self {
        Windows {
            layout: Layout::One,
            windows: vec![Window { id: 1, saved: None, view: "View 1".into() }],
            active: 1,
            maximised: None,
            splits: [0.5, 0.5],
            views: vec![NamedView { name: "View 1".into(), ..NamedView::default() }],
            named_templates: Vec::new(),
            next: 2,
        }
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

    /// The named view window `id` shows.
    pub fn view_of(&self, id: u64) -> Option<&NamedView> {
        let name = &self.windows.iter().find(|x| x.id == id)?.view;
        self.views.iter().find(|v| &v.name == name)
    }

    /// The window showing view `name`, if any (one window at a time, docs/15 R6).
    pub fn window_of(&self, name: &str) -> Option<u64> {
        self.windows.iter().find(|x| x.view == name).map(|x| x.id)
    }

    /// A view name not used yet: `View n`.
    fn free_view_name(&self, base: &str) -> String {
        (1..).map(|n| if base == "View" { format!("View {n}") } else if n == 1 { base.to_string() } else { format!("{base} ({n})") }).find(|c| !self.views.iter().any(|v| &v.name == c)).unwrap_or_default()
    }

    /// Keep what window `id` shows as its view's own state (before the window closes).
    fn store_own(&mut self, id: u64) {
        let Some(state) = capture_in(self, id) else { return };
        let Some(name) = self.windows.iter().find(|x| x.id == id).map(|x| x.view.clone()) else { return };
        if let Some(v) = self.views.iter_mut().find(|v| v.name == name) {
            v.own = state;
        }
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

/// A new window, a copy of the active one (docs/14 W7), showing a new named view `View n` with
/// the active view's template (docs/15 N1).
fn copy_of_active(w: &mut Windows) -> Window {
    let id = w.next;
    w.next += 1;
    #[cfg(feature = "ui")]
    custom_bridge::view::park(id);
    let name = w.free_view_name("View");
    let template = w.view_of(w.active).and_then(|v| v.template.clone());
    let own = capture_in(w, w.active).unwrap_or_default();
    w.views.push(NamedView { name: name.clone(), template, own });
    Window { id, saved: Some(Saved::current()), view: name }
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
    w.windows[old].saved = Some(Saved { shown, snap, settings: vv::settings(), hidden: visibility::hidden(), hidden_names: None });
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
    if w.windows.len() > n {
        let ids: Vec<u64> = w.windows.iter().map(|x| x.id).collect();
        for id in ids {
            w.store_own(id); // a closed window's view keeps what it showed (docs/15 R6)
        }
    }
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
    if w.layout != layout {
        w.splits = layout.default_splits();
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
        w.splits = layout.default_splits();
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
        w.store_own(id); // its view keeps what it showed (docs/15 R6)
        if w.active == id {
            let next = w.windows[if k + 1 < w.windows.len() { k + 1 } else { k - 1 }].id;
            activate_in(w, next);
        }
        w.windows.remove(k);
        #[cfg(feature = "ui")]
        custom_bridge::view::forget(id);
        w.layout = Layout::for_count(w.windows.len(), w.layout);
        w.splits = w.layout.default_splits();
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
/// (R4) – except elements given by name (a template or the saved layout, §10) –, keeps its choice
/// (the first load where its own is gone) and is built again for the new model on a worker
/// thread; the active one is built by the transfer itself.
pub fn after_transfer() {
    // The active window's hidden elements given by name.
    let pending = ACTIVE_HIDDEN_NAMES.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(names) = pending {
        visibility::set_hidden(set_of(&names));
        results_view::refresh();
    }
    let ids: Vec<u64> = write(|w| {
        let mut ids = Vec::new();
        for win in &mut w.windows {
            if let Some(s) = win.saved.as_mut() {
                s.hidden = s.hidden_names.take().map(|n| set_of(&n)).unwrap_or_default();
                ids.push(win.id);
            }
        }
        ids
    });
    for id in ids {
        rebuild_parked(id);
    }
}

/// Build inactive window `id`'s result again for the model there now (worker thread).
fn rebuild_parked(id: u64) {
    let loads = results_view::loads();
    if loads.is_empty() {
        return; // no model yet: built after the transfer
    }
    let units = crate::model::read(|f| f.units);
    let job = write(|w| {
        let s = w.windows.iter_mut().find(|x| x.id == id).and_then(|x| x.saved.as_mut())?;
        if s.shown.choice.load.as_ref().is_none_or(|l| !loads.contains(l)) {
            s.shown.choice.load = loads.first().cloned();
        }
        s.shown.units = Some(units);
        s.shown.busy = true;
        Some((s.shown.choice.clone(), s.settings.clone(), s.hidden.clone()))
    });
    let Some((choice, settings, hidden)) = job else { return };
    std::thread::spawn(move || {
        let built = results_view::build_hidden(&transfer::geometry(), &transfer::results(), &choice, units, &settings, Some(&hidden));
        let mut w = WINDOWS.write().unwrap_or_else(PoisonError::into_inner);
        let Some(s) = w.windows.iter_mut().find(|x| x.id == id).and_then(|x| x.saved.as_mut()) else { return };
        if s.shown.choice != choice || s.settings != settings || s.hidden != hidden {
            return; // changed meanwhile (built again for that, or activated and built there)
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

/// What a window shows, for its title bar: number, load and components.
pub fn title(w: &Windows, id: u64) -> String {
    let shown = match w.windows.iter().find(|x| x.id == id).and_then(|x| x.saved.as_ref()) {
        Some(s) => s.shown.clone(),
        None => results_view::shown(),
    };
    let view = w.view_of(id);
    let mut parts = vec![match view {
        Some(v) => match &v.template {
            Some(t) => format!("{} [{t}]", v.name),
            None => v.name.clone(),
        },
        None => format!("{}", w.number(id)),
    }];
    if let Some(load) = &shown.choice.load {
        parts.push(load.clone());
    }
    // A template's load the model has not (docs/15 R4).
    if let Some(t) = view.and_then(|v| v.template.as_ref()).and_then(|t| templates().into_iter().find(|x| &x.name == t))
        && t.include.result
        && let Some(l) = t.state.choice.load.as_ref()
        && !results_view::loads().is_empty()
        && !results_view::loads().contains(l)
    {
        parts.push(format!("template load \"{l}\" not in this model"));
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

// ---- Borders (docs/14 §10.2) ----

/// Move the borders: vertical (x) and horizontal (y) as fractions of the area (kept 15 %–85 %).
pub fn set_splits(splits: [f32; 2]) {
    let splits = splits.map(|v| if v.is_finite() { v.clamp(MIN_SPLIT, 1.0 - MIN_SPLIT) } else { 0.5 });
    WINDOWS.write().unwrap_or_else(PoisonError::into_inner).splits = splits;
    results_view::repaint();
}

// ---- Elements by name (Hide / Isolate kept across transfers, §10.1) ----

/// Most hidden elements kept by name in a template or the saved layout; a larger hidden set (an
/// Isolate on a very large model) is not kept (docs/14 §10.1).
pub const MAX_HIDDEN_NAMES: usize = 100_000;

/// The model's names of the elements in `set` (none when there are more than [`MAX_HIDDEN_NAMES`]).
fn names_of(set: &Set) -> (Vec<String>, Vec<String>) {
    let (s, f) = set.counts();
    if s + f > MAX_HIDDEN_NAMES {
        return (Vec::new(), Vec::new());
    }
    let g = transfer::geometry();
    let pick = |names: &Vec<String>, bits: &Bits| bits.ones().filter_map(|e| names.get(e).cloned()).collect();
    (pick(&g.shell_names, &set.shells), pick(&g.frame_names, &set.frames))
}

/// The elements of the model with these names (names it has not are skipped).
fn set_of((shells, frames): &(Vec<String>, Vec<String>)) -> Set {
    let g = transfer::geometry();
    let bits = |all: &Vec<String>, wanted: &Vec<String>| {
        let wanted: std::collections::HashSet<&str> = wanted.iter().map(String::as_str).collect();
        let mut b = Bits::with_len(all.len());
        for (e, n) in all.iter().enumerate() {
            if wanted.contains(n.as_str()) {
                b.set(e, true);
            }
        }
        b
    };
    if shells.is_empty() && frames.is_empty() {
        return Set::default();
    }
    Set { shells: bits(&g.shell_names, shells), frames: bits(&g.frame_names, frames) }
}

/// The active window's hidden elements by name, applied after the next transfer (no model yet).
static ACTIVE_HIDDEN_NAMES: Mutex<Option<(Vec<String>, Vec<String>)>> = Mutex::new(None);

fn model_there() -> bool {
    !transfer::geometry().joints.is_empty()
}

// ---- View templates (docs/14 §10.1) ----

/// Everything one window shows: camera, result, Visibility/Graphics with View Options, and the
/// elements hidden by Hide / Isolate (by name).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ViewState {
    /// SolidCraft's camera as text (kept as given).
    pub camera: Option<String>,
    pub choice: results_view::Choice,
    pub manual_shell: Option<[f32; 2]>,
    pub manual_frame: Option<[f32; 2]>,
    pub settings: vv::Settings,
    pub hidden_shells: Vec<String>,
    pub hidden_frames: Vec<String>,
}

/// A named view state: a view template (docs/15). `include` says which parts it rules.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Template {
    pub name: String,
    pub state: ViewState,
    pub include: Include,
}

static TEMPLATES: RwLock<Vec<Template>> = RwLock::new(Vec::new());

/// The view templates, in the order saved.
pub fn templates() -> Vec<Template> {
    TEMPLATES.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// What window `id` shows now.
pub fn capture(id: u64) -> Option<ViewState> {
    capture_in(&current(), id)
}

fn capture_in(w: &Windows, id: u64) -> Option<ViewState> {
    let win = w.windows.iter().find(|x| x.id == id)?;
    #[cfg(feature = "ui")]
    let camera = custom_bridge::view::camera_of(id);
    #[cfg(not(feature = "ui"))]
    let camera = None;
    let (shown, settings, hidden, names) = match &win.saved {
        Some(s) => (s.shown.clone(), s.settings.clone(), s.hidden.clone(), s.hidden_names.clone()),
        None => (results_view::shown(), vv::settings(), visibility::hidden(), ACTIVE_HIDDEN_NAMES.lock().unwrap_or_else(PoisonError::into_inner).clone()),
    };
    // Names not applied yet (no model) are kept as they are.
    let (hidden_shells, hidden_frames) = names.unwrap_or_else(|| names_of(&hidden));
    Some(ViewState { camera, choice: shown.choice, manual_shell: shown.manual_shell, manual_frame: shown.manual_frame, settings, hidden_shells, hidden_frames })
}

/// Give window `id` everything in `st` (VT2): its result is built again on a worker thread.
pub fn apply_state(id: u64, st: &ViewState) {
    apply_parts(id, st, Include::all());
}

/// Whether two result choices are the same for a template: a template's load the model has not
/// counts as the first load (docs/15 R4), so it is never taken for a change.
fn same_result(a: &ViewState, b: &ViewState) -> bool {
    let loads = results_view::loads();
    let load = |c: &results_view::Choice| match &c.load {
        Some(l) if !loads.is_empty() && !loads.contains(l) => loads.first().cloned(),
        None if !loads.is_empty() => loads.first().cloned(),
        other => other.clone(),
    };
    let strip = |c: &results_view::Choice| results_view::Choice { load: None, ..c.clone() };
    load(&a.choice) == load(&b.choice) && strip(&a.choice) == strip(&b.choice) && a.manual_shell == b.manual_shell && a.manual_frame == b.manual_frame
}

/// Whether the elements hidden in `a` (a window, by its set) are those `b` names.
fn same_hidden(window: &Set, b: &ViewState) -> bool {
    *window == set_of(&(b.hidden_shells.clone(), b.hidden_frames.clone()))
}

/// Give window `id` the parts of `st` that `inc` names, and only those that differ from what it
/// shows (a template ruling a view, docs/15 R1): nothing is built again for an unchanged result.
pub fn apply_parts(id: u64, st: &ViewState, inc: Include) {
    #[cfg(feature = "ui")]
    if inc.camera
        && let Some(cam) = &st.camera
        && custom_bridge::view::camera_of(id).as_ref() != Some(cam)
    {
        custom_bridge::view::set_camera_of(id, cam);
    }
    let names = (st.hidden_shells.clone(), st.hidden_frames.clone());
    let there = model_there();
    let Some(now) = capture(id) else { return };
    let active = current().active == id;
    let vg = inc.vg && now.settings != st.settings;
    let result = inc.result && !same_result(&now, st);
    let hidden = inc.hidden && if there { !same_hidden(&if active { visibility::hidden() } else { saved_hidden(id) }, st) } else { now.hidden_shells != st.hidden_shells || now.hidden_frames != st.hidden_frames };
    if !(vg || result || hidden) {
        return;
    }
    if active {
        if vg {
            vv::set_quiet(st.settings.clone());
        }
        if hidden {
            if there {
                visibility::set_hidden(set_of(&names));
            } else {
                *ACTIVE_HIDDEN_NAMES.lock().unwrap_or_else(PoisonError::into_inner) = Some(names);
            }
        }
        let shown = results_view::shown();
        let (choice, ms, mf) = if result { (st.choice.clone(), st.manual_shell, st.manual_frame) } else { (shown.choice, shown.manual_shell, shown.manual_frame) };
        if there {
            results_view::select_with_ranges(choice, ms, mf, crate::model::read(|f| f.units));
        } else {
            results_view::set_choice_quiet(choice, ms, mf);
        }
        results_view::repaint();
        return;
    }
    write(|w| {
        let Some(s) = w.windows.iter_mut().find(|x| x.id == id).and_then(|x| x.saved.as_mut()) else { return };
        if vg {
            s.settings = st.settings.clone();
        }
        if result {
            s.shown.choice = st.choice.clone();
            s.shown.manual_shell = st.manual_shell;
            s.shown.manual_frame = st.manual_frame;
        }
        if hidden {
            if there {
                s.hidden = set_of(&names);
                s.hidden_names = None;
            } else {
                s.hidden = Set::default();
                s.hidden_names = Some(names);
            }
        }
    });
    if there {
        rebuild_parked(id);
    }
}

/// An inactive window's hidden set.
fn saved_hidden(id: u64) -> Set {
    current().windows.iter().find(|x| x.id == id).and_then(|x| x.saved.as_ref()).map(|s| s.hidden.clone()).unwrap_or_default()
}

/// Keep what window `id` shows as template `name` (a template of that name is replaced, VT1).
pub fn save_template(name: &str, id: u64) -> bool {
    let name = name.trim();
    let Some(state) = capture(id).filter(|_| !name.is_empty()) else { return false };
    {
        let mut t = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
        match t.iter_mut().find(|x| x.name == name) {
            Some(x) => x.state = state,
            None => t.push(Template { name: name.to_string(), state, include: Include::new_template() }),
        }
    }
    propagate(name, None);
    true
}

/// Give window `id`'s view template `name` (linked, docs/15 N3). `false` when there is no
/// such template.
pub fn apply_template(name: &str, id: u64) -> bool {
    let Some(view) = current().windows.iter().find(|x| x.id == id).map(|x| x.view.clone()) else { return false };
    set_view_template(&view, Some(name))
}

/// Delete template `name`; its views keep what it gave them as their own (docs/15 R3, R7).
pub fn delete_template(name: &str) -> bool {
    let linked: Vec<String> = current().views.iter().filter(|v| v.template.as_deref() == Some(name)).map(|v| v.name.clone()).collect();
    for v in &linked {
        set_view_template(v, None);
    }
    write(|w| w.named_templates.retain(|(_, t)| t != name));
    let mut t = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
    let before = t.len();
    t.retain(|x| x.name != name);
    t.len() != before
}

// ---- Kept between sessions (docs/14 §10.4) ----

/// What `windows.json` holds.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Stored {
    pub layout: String,
    pub splits: [f32; 2],
    /// Index of the active and the maximised window.
    pub active: usize,
    pub maximised: Option<usize>,
    pub windows: Vec<ViewState>,
    pub templates: Vec<Template>,
    /// Every named view (docs/15) and the view each window shows.
    pub views: Vec<NamedView>,
    pub shown: Vec<String>,
    /// The starting templates were given once (docs/15 §3): deleted ones do not come back.
    pub seeded: bool,
    /// SolidCraft's named views and their templates (docs/15 §10).
    pub named_templates: Vec<(String, String)>,
    /// The Cut Diagrams of every model (docs/16 §4).
    pub cuts: Vec<crate::cut::Cut>,
}

/// The windows and templates as they would be stored now.
pub fn stored() -> Stored {
    let mut w = current();
    let ids: Vec<u64> = w.windows.iter().map(|x| x.id).collect();
    for id in ids {
        w.store_own(id);
    }
    Stored {
        views: w.views.clone(),
        shown: w.windows.iter().map(|x| x.view.clone()).collect(),
        named_templates: w.named_templates.clone(),
        cuts: crate::cut::all(),
        seeded: SEEDED.load(std::sync::atomic::Ordering::Relaxed),
        layout: w.layout.key().to_string(),
        splits: w.splits,
        active: w.index_of(w.active).unwrap_or(0),
        maximised: w.maximised.and_then(|m| w.index_of(m)),
        windows: w.windows.iter().filter_map(|x| capture(x.id)).collect(),
        templates: templates(),
    }
}

/// Put back windows and templates as stored (on start, before any model is transferred).
pub fn restore(st: &Stored) {
    let layout = Layout::from_key(&st.layout).unwrap_or_default();
    let n = st.windows.len().max(st.shown.len()).clamp(1, MAX_WINDOWS).min(layout.zones().max(1));
    *TEMPLATES.write().unwrap_or_else(PoisonError::into_inner) = st.templates.clone();
    crate::cut::set_all(st.cuts.clone());
    SEEDED.store(st.seeded, std::sync::atomic::Ordering::Relaxed);
    reset();
    // The views; a file of §14 (no views) gets one view per window from its states.
    let mut views = st.views.clone();
    views.retain(|v| !v.name.trim().is_empty());
    let shown: Vec<String> = (0..n)
        .map(|k| {
            st.shown.get(k).filter(|name| views.iter().any(|v| &v.name == *name)).cloned().unwrap_or_else(|| {
                let name = (1..).map(|i| format!("View {i}")).find(|c| !views.iter().any(|v| &v.name == c)).unwrap_or_default();
                views.push(NamedView { name: name.clone(), template: None, own: st.windows.get(k).cloned().unwrap_or_default() });
                name
            })
        })
        .collect();
    {
        let mut w = WINDOWS.write().unwrap_or_else(PoisonError::into_inner);
        w.views = views;
        w.named_templates = st.named_templates.clone();
    }
    if st.windows.is_empty() && st.shown.is_empty() {
        WINDOWS.write().unwrap_or_else(PoisonError::into_inner).windows[0].view = shown[0].clone();
        let state = effective_of(&current(), &shown[0]);
        apply_state(1, &state);
        return;
    }
    let active_index = st.active.min(n - 1);
    {
        let mut w = WINDOWS.write().unwrap_or_else(PoisonError::into_inner);
        w.windows = (0..n as u64).map(|k| Window { id: k + 1, saved: None, view: shown[k as usize].clone() }).collect();
        w.next = n as u64 + 1;
        w.active = active_index as u64 + 1;
        w.layout = if layout.zones() == n { layout } else { Layout::for_count(n, layout) };
        w.splits = st.splits;
        w.maximised = st.maximised.filter(|m| *m < n).map(|m| m as u64 + 1);
        for (k, win) in w.windows.iter_mut().enumerate() {
            if k != active_index {
                win.saved = Some(Saved {
                    shown: Shown::default(),
                    snap: Arc::new(SnapData::default()),
                    settings: vv::Settings::default(),
                    hidden: Set::default(),
                    hidden_names: None,
                });
            }
        }
    }
    let w = current();
    for (k, name) in shown.iter().enumerate() {
        let state = effective_of(&w, name);
        apply_state(k as u64 + 1, &state);
    }
}

/// Where the windows are kept; set by [`keep_between_sessions`] (the app, not the tests).
static FILE: OnceLock<PathBuf> = OnceLock::new();
/// What was last written, and when it was last looked at.
static LAST: Mutex<(String, Option<std::time::Instant>)> = Mutex::new((String::new(), None));

/// Read `%LOCALAPPDATA%\Factory\windows.json` (if any) and keep it up to date from now on (§10.4).
/// A broken file is ignored.
pub fn keep_between_sessions() {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let path = base.join("Factory").join("windows.json");
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(st) = serde_json::from_str::<Stored>(&text)
    {
        restore(&st);
        *LAST.lock().unwrap_or_else(PoisonError::into_inner) = (text, None);
    }
    seed_templates(); // the starting templates, once (docs/15 §3)
    let _ = FILE.set(path);
}

/// Write the windows when they changed – at most once a second (called every frame).
pub fn keep_up() {
    let Some(path) = FILE.get() else { return };
    {
        let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
        if last.1.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        last.1 = Some(std::time::Instant::now());
    }
    // A cheap look first: the file is written only when something changed.
    let key = fingerprint();
    if *KEY.lock().unwrap_or_else(PoisonError::into_inner) == Some(key) {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(&stored()) else { return };
    *KEY.lock().unwrap_or_else(PoisonError::into_inner) = Some(key);
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    if last.0 == text {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Written beside and renamed, so a crash never leaves half a file.
    let part = path.with_extension("json.part");
    if std::fs::write(&part, &text).is_ok() && std::fs::rename(&part, path).is_ok() {
        last.0 = text;
    }
}

static KEY: Mutex<Option<u64>> = Mutex::new(None);

/// A hash of what [`stored`] holds, without naming any element.
fn fingerprint() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let w = current();
    (w.layout.key(), w.splits.map(f32::to_bits), w.index_of(w.active), w.maximised.and_then(|m| w.index_of(m))).hash(&mut h);
    visibility::version().hash(&mut h);
    for x in &w.windows {
        #[cfg(feature = "ui")]
        custom_bridge::view::camera_of(x.id).hash(&mut h);
        match &x.saved {
            Some(s) => format!("{:?}{:?}{:?}{:?}{:?}{:?}", s.shown.choice, s.shown.manual_shell, s.shown.manual_frame, s.settings, s.hidden.counts(), s.hidden_names.as_ref().map(|n| (n.0.len(), n.1.len()))).hash(&mut h),
            None => {
                let s = results_view::shown();
                format!("{:?}{:?}{:?}{:?}", s.choice, s.manual_shell, s.manual_frame, vv::settings()).hash(&mut h);
            }
        }
    }
    format!("{:?}", templates()).hash(&mut h);
    for v in &w.views {
        (&v.name, &v.template).hash(&mut h);
    }
    w.named_templates.hash(&mut h);
    crate::cut::version().hash(&mut h);
    h.finish()
}

// ---- Template commands (VT3) ----

/// The template dialog is asked for (toolbar button), read once by the UI.
static TEMPLATES_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn take_templates_request() -> bool {
    TEMPLATES_ASKED.swap(false, std::sync::atomic::Ordering::Relaxed)
}

fn name_param(params: &Value) -> Result<String> {
    params.get("name").and_then(Value::as_str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).ok_or_else(|| custom_bridge::Error("give the template's name".into()))
}

fn template_report() -> Result<Value> {
    Ok(json!({ "templates": templates().iter().map(|t| t.name.clone()).collect::<Vec<_>>() }))
}

/// `factory.window.templates`: open the View Templates dialog; lists the templates.
pub fn templates_cmd(_: &mut Ctx, _: &Value) -> Result<Value> {
    TEMPLATES_ASKED.store(true, std::sync::atomic::Ordering::Relaxed);
    results_view::repaint();
    template_report()
}

/// `factory.window.save_template`: keep window `tile` (default the active one) as template `name`.
pub fn save_template_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let name = name_param(params)?;
    if !save_template(&name, tile_param(params)?) {
        return Err(custom_bridge::Error("the window could not be read".into()));
    }
    template_report()
}

/// `factory.window.apply_template`: apply template `name` to window `tile` (default the active one).
pub fn apply_template_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let name = name_param(params)?;
    if !apply_template(&name, tile_param(params)?) {
        return Err(custom_bridge::Error(format!("no template {name:?}")));
    }
    report()
}

/// `factory.window.delete_template`: delete template `name`.
pub fn delete_template_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let name = name_param(params)?;
    if !delete_template(&name) {
        return Err(custom_bridge::Error(format!("no template {name:?}")));
    }
    template_report()
}

// ---- Named views (docs/15) ----

/// Which parts of a view a template rules (docs/15 R1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Include {
    /// Visibility/Graphic Overrides with View Options.
    pub vg: bool,
    /// The result: load, plate force, beam force, envelope, averaging, legend ranges.
    pub result: bool,
    pub camera: bool,
    /// Hide / Isolate.
    pub hidden: bool,
}

impl Default for Include {
    /// Everything – a template kept by §14 (before the switches) rules all of a view.
    fn default() -> Self {
        Include::all()
    }
}

impl Include {
    pub fn all() -> Include {
        Include { vg: true, result: true, camera: true, hidden: true }
    }

    /// A new template: Visibility/Graphics and the result; camera and Hide / Isolate stay the
    /// view's own (docs/15 N5).
    pub fn new_template() -> Include {
        Include { vg: true, result: true, camera: false, hidden: false }
    }

    /// The template's parts of `tpl` over `own`.
    pub fn merge(self, own: &ViewState, tpl: &ViewState) -> ViewState {
        let mut v = own.clone();
        if self.vg {
            v.settings = tpl.settings.clone();
        }
        if self.result {
            v.choice = tpl.choice.clone();
            v.manual_shell = tpl.manual_shell;
            v.manual_frame = tpl.manual_frame;
        }
        if self.camera && tpl.camera.is_some() {
            v.camera = tpl.camera.clone();
        }
        if self.hidden {
            v.hidden_shells = tpl.hidden_shells.clone();
            v.hidden_frames = tpl.hidden_frames.clone();
        }
        v
    }

    /// Short text for the Browser and the dialogs, e.g. `V/G · Result`.
    pub fn summary(self) -> String {
        let parts: Vec<&str> = [(self.vg, "V/G"), (self.result, "Result"), (self.camera, "Camera"), (self.hidden, "Hide")].into_iter().filter(|(on, _)| *on).map(|(_, n)| n).collect();
        if parts.is_empty() { "nothing".into() } else { parts.join(" · ") }
    }
}

/// A named view: its name, its template (or none) and its own state (the parts the template does
/// not rule, and everything without a template).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct NamedView {
    pub name: String,
    pub template: Option<String>,
    pub own: ViewState,
}

/// The views, in the order made.
pub fn views() -> Vec<NamedView> {
    current().views
}

/// What view `name` shows: its own state with its template's parts over it (docs/15 N4, N6).
fn effective_of(w: &Windows, name: &str) -> ViewState {
    let Some(v) = w.views.iter().find(|v| v.name == name) else { return ViewState::default() };
    match v.template.as_ref().and_then(|t| templates().into_iter().find(|x| &x.name == t)) {
        Some(t) => t.include.merge(&v.own, &t.state),
        None => v.own.clone(),
    }
}

/// Open view `name` in window `id` (docs/15 N2, R6): the window's view keeps what it showed; a
/// view shown in another window makes that window active instead.
pub fn open_view(name: &str, id: u64) -> bool {
    let w = current();
    if !w.views.iter().any(|v| v.name == name) || w.index_of(id).is_none() {
        return false;
    }
    if let Some(other) = w.window_of(name) {
        activate(other);
        return true;
    }
    write(|w| {
        w.store_own(id);
        if let Some(x) = w.windows.iter_mut().find(|x| x.id == id) {
            x.view = name.to_string();
        }
    });
    let state = effective_of(&current(), name);
    apply_state(id, &state);
    true
}

/// Rename view `old` (names unique, docs/15 R7). `false` when the name is taken or empty.
pub fn rename_view(old: &str, new: &str) -> bool {
    let new = new.trim();
    write(|w| {
        if new.is_empty() || w.views.iter().any(|v| v.name == new) {
            return false;
        }
        let Some(v) = w.views.iter_mut().find(|v| v.name == old) else { return false };
        v.name = new.to_string();
        for x in w.windows.iter_mut().filter(|x| x.view == old) {
            x.view = new.to_string();
        }
        true
    })
}

/// A copy of view `name` (its state and template), not shown; the copy's name.
pub fn duplicate_view(name: &str) -> Option<String> {
    write(|w| {
        let ids: Vec<u64> = w.windows.iter().filter(|x| x.view == name).map(|x| x.id).collect();
        for id in ids {
            w.store_own(id);
        }
        let v = w.views.iter().find(|v| v.name == name)?.clone();
        let copy = w.free_view_name(&format!("{name} copy"));
        w.views.push(NamedView { name: copy.clone(), ..v });
        Some(copy)
    })
}

/// A new view, a copy of window `id`'s, not shown (`factory.view.new`); its name.
pub fn new_view(id: u64) -> Option<String> {
    let name = current().view_of(id)?.name.clone();
    duplicate_view(&name)
}

/// Delete view `name`. A view shown in a window is not deleted (open another one there first).
pub fn delete_view(name: &str) -> bool {
    write(|w| {
        if w.window_of(name).is_some() {
            return false;
        }
        let before = w.views.len();
        w.views.retain(|v| v.name != name);
        w.views.len() != before
    })
}

/// Give view `view` template `template` (`None` detaches, keeping what the template gave as the
/// view's own, docs/15 R3). A view shown in a window shows the template's parts at once.
pub fn set_view_template(view: &str, template: Option<&str>) -> bool {
    if template.is_some_and(|t| !templates().iter().any(|x| x.name == t)) {
        return false;
    }
    let shown_in = current().window_of(view);
    let ok = write(|w| {
        // Detaching keeps the shown state (nothing jumps).
        if let Some(id) = shown_in {
            w.store_own(id);
        } else if template.is_none() {
            let state = effective_of(w, view);
            if let Some(v) = w.views.iter_mut().find(|v| v.name == view) {
                v.own = state;
            }
        }
        let Some(v) = w.views.iter_mut().find(|v| v.name == view) else { return false };
        v.template = template.map(str::to_string);
        true
    });
    if ok
        && let (Some(id), Some(t)) = (shown_in, template.and_then(|t| templates().into_iter().find(|x| x.name == t)))
    {
        apply_parts(id, &t.state, t.include);
    }
    ok
}

/// Template `name`, if there is one.
pub fn template(name: &str) -> Option<Template> {
    templates().into_iter().find(|x| x.name == name)
}

/// Give template `name` this state and these switches (the View Templates editor, docs/15 §9):
/// every window whose view uses it shows it at once (N6).
pub fn set_template(name: &str, state: ViewState, include: Include) -> bool {
    {
        let mut t = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
        let Some(x) = t.iter_mut().find(|x| x.name == name) else { return false };
        x.state = state;
        x.include = include;
    }
    propagate(name, None);
    true
}

/// A copy of template `name` (`name copy`); the copy's name.
pub fn duplicate_template(name: &str) -> Option<String> {
    let mut t = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
    let src = t.iter().find(|x| x.name == name)?.clone();
    let copy = (1..).map(|n| if n == 1 { format!("{name} copy") } else { format!("{name} copy ({n})") }).find(|c| !t.iter().any(|x| &x.name == c))?;
    t.push(Template { name: copy.clone(), ..src });
    Some(copy)
}

/// Rename template `old`; its views stay linked (R7).
pub fn rename_template(old: &str, new: &str) -> bool {
    let new = new.trim();
    {
        let mut t = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
        if new.is_empty() || t.iter().any(|x| x.name == new) {
            return false;
        }
        let Some(x) = t.iter_mut().find(|x| x.name == old) else { return false };
        x.name = new.to_string();
    }
    write(|w| {
        for v in w.views.iter_mut().filter(|v| v.template.as_deref() == Some(old)) {
            v.template = Some(new.to_string());
        }
        for (_, t) in w.named_templates.iter_mut().filter(|(_, t)| t == old) {
            *t = new.to_string();
        }
    });
    true
}

/// Set which parts template `name` rules (R1); its shown views follow at once.
pub fn set_include(name: &str, inc: Include) -> bool {
    {
        let mut t = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
        let Some(x) = t.iter_mut().find(|x| x.name == name) else { return false };
        x.include = inc;
    }
    propagate(name, None);
    true
}

/// Template `name` changed: every window whose view uses it shows its parts (N6), except `skip`
/// (the window it was changed in).
fn propagate(name: &str, skip: Option<u64>) {
    let Some(t) = templates().into_iter().find(|x| x.name == name) else { return };
    let w = current();
    for x in &w.windows {
        if Some(x.id) != skip && w.views.iter().any(|v| v.name == x.view && v.template.as_deref() == Some(name)) {
            apply_parts(x.id, &t.state, t.include);
        }
    }
}

/// The template ruling the active window's view and its parts, if any (for the locks, R2).
pub fn active_template() -> Option<(String, Include)> {
    let w = current();
    let name = w.view_of(w.active)?.template.clone()?;
    let t = templates().into_iter().find(|x| x.name == name)?;
    Some((t.name, t.include))
}

/// The forms are unlocked to edit the active view's template (R2): changes go to the template.
static EDITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn editing_template() -> bool {
    EDITING.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_editing_template(on: bool) {
    EDITING.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Keep the link live (N6): what changed in the active window's ruled parts goes into its
/// template and to every other window using it. Called every frame; looks at most 4 times a second.
pub fn sync() {
    static LAST_LOOK: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    {
        let mut last = LAST_LOOK.lock().unwrap_or_else(PoisonError::into_inner);
        if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250)) {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    let w = current();
    let Some(name) = w.view_of(w.active).and_then(|v| v.template.clone()) else { return };
    let Some(t) = templates().into_iter().find(|x| x.name == name) else { return };
    let Some(now) = capture_in(&w, w.active) else { return };
    let inc = t.include;
    let vg = inc.vg && now.settings != t.state.settings;
    let result = inc.result && !same_result(&now, &t.state);
    let camera = inc.camera && now.camera.is_some() && now.camera != t.state.camera;
    let hidden = inc.hidden && model_there() && !same_hidden(&visibility::hidden(), &t.state);
    if !(vg || result || camera || hidden) {
        return;
    }
    {
        let mut all = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
        let Some(x) = all.iter_mut().find(|x| x.name == name) else { return };
        let only = Include { vg, result, camera, hidden };
        x.state = only.merge(&x.state, &now);
    }
    propagate(&name, Some(w.active));
}

/// The starting templates were given (docs/15 §3).
static SEEDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The starting templates (docs/15 §3): plate forces, plate membrane, beam forces, beam axial,
/// geometry.
pub fn starting_templates() -> Vec<Template> {
    let plain = vv::Settings::default();
    let mut plates = plain.clone();
    plates.frames.halftone = true;
    plates.legend = true;
    let mut beams = plain.clone();
    beams.shells.halftone = true;
    beams.legend = true;
    let mut geometry = plain.clone();
    geometry.contour = false;
    geometry.legend = false;
    geometry.shells.labels.visible = true;
    geometry.shells.sections.visible = true;
    geometry.frames.labels.visible = true;
    geometry.frames.sections.visible = true;
    let choice = |shell: usize, frame: usize| results_view::Choice { shell, frame, ..Default::default() };
    let t = |name: &str, settings: &vv::Settings, choice: results_view::Choice| Template {
        name: name.into(),
        state: ViewState { settings: settings.clone(), choice, ..ViewState::default() },
        include: Include::new_template(),
    };
    vec![
        t("Plate forces", &plates, choice(3, 0)),  // M11
        t("Plate membrane", &plates, choice(0, 0)), // F11
        t("Beam forces", &beams, choice(0, 5)),     // M3
        t("Beam axial", &beams, choice(0, 0)),      // P
        t("Geometry", &geometry, choice(0, 0)),
    ]
}

/// Add the starting templates once (a name already taken is left as it is).
fn seed_templates() {
    if SEEDED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let mut t = TEMPLATES.write().unwrap_or_else(PoisonError::into_inner);
    for s in starting_templates() {
        if !t.iter().any(|x| x.name == s.name) {
            t.push(s);
        }
    }
}

// ---- Commands `factory.view.*` and template switches (docs/15 R8) ----

fn view_param(params: &Value) -> Result<String> {
    match params.get("view").and_then(Value::as_str) {
        Some(v) => Ok(v.to_string()),
        None => {
            let w = current();
            w.view_of(w.active).map(|v| v.name.clone()).ok_or_else(|| custom_bridge::Error("no view".into()))
        }
    }
}

fn views_report() -> Result<Value> {
    let w = current();
    Ok(json!({
        "views": w.views.iter().map(|v| json!({ "name": v.name, "template": v.template, "window": w.window_of(&v.name).map(|id| w.number(id)) })).collect::<Vec<_>>(),
    }))
}

/// `factory.view.new`: a new named view, a copy of the active window's.
pub fn view_new_cmd(_: &mut Ctx, _: &Value) -> Result<Value> {
    new_view(current().active).ok_or_else(|| custom_bridge::Error("no view to copy".into()))?;
    views_report()
}

/// `factory.view.open`: open view `view` in window `tile` (default the active one).
pub fn view_open_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let name = view_param(params)?;
    if !open_view(&name, tile_param(params)?) {
        return Err(custom_bridge::Error(format!("no view {name:?}")));
    }
    views_report()
}

/// `factory.view.rename`: rename view `view` to `name`.
pub fn view_rename_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let old = view_param(params)?;
    let new = name_param(params)?;
    if !rename_view(&old, &new) {
        return Err(custom_bridge::Error(format!("cannot rename {old:?} to {new:?} (name taken or no such view)")));
    }
    views_report()
}

/// `factory.view.duplicate`: a copy of view `view`.
pub fn view_duplicate_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let name = view_param(params)?;
    duplicate_view(&name).ok_or_else(|| custom_bridge::Error(format!("no view {name:?}")))?;
    views_report()
}

/// `factory.view.delete`: delete view `view` (not while a window shows it).
pub fn view_delete_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let name = view_param(params)?;
    if !delete_view(&name) {
        return Err(custom_bridge::Error(format!("cannot delete {name:?}: no such view, or a window shows it")));
    }
    views_report()
}

/// `factory.view.set_template`: give view `view` template `template` (empty or missing: none).
pub fn view_set_template_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let view = view_param(params)?;
    let template = params.get("template").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty() && *t != "<None>");
    if !set_view_template(&view, template) {
        return Err(custom_bridge::Error(format!("no view {view:?} or no template {template:?}")));
    }
    views_report()
}

/// `factory.template.rename`: rename template `template` to `name`.
pub fn template_rename_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let old = params.get("template").and_then(Value::as_str).unwrap_or_default().to_string();
    let new = name_param(params)?;
    if !rename_template(&old, &new) {
        return Err(custom_bridge::Error(format!("cannot rename template {old:?} to {new:?}")));
    }
    template_report()
}

/// `factory.template.include`: which parts template `template` rules (vg, result, camera, hidden).
pub fn template_include_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let name = params.get("template").and_then(Value::as_str).unwrap_or_default().to_string();
    let now = templates().into_iter().find(|x| x.name == name).ok_or_else(|| custom_bridge::Error(format!("no template {name:?}")))?.include;
    let flag = |k: &str, v: bool| params.get(k).and_then(Value::as_bool).unwrap_or(v);
    let inc = Include { vg: flag("vg", now.vg), result: flag("result", now.result), camera: flag("camera", now.camera), hidden: flag("hidden", now.hidden) };
    set_include(&name, inc);
    Ok(json!({ "template": name, "include": inc.summary() }))
}

/// `factory.template.edit`: unlock the forms to edit the active view's template (`on`, default toggles).
pub fn template_edit_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let on = params.get("on").and_then(Value::as_bool).unwrap_or(!editing_template());
    set_editing_template(on);
    results_view::repaint();
    Ok(json!({ "editing": on, "template": active_template().map(|t| t.0) }))
}

// ---- SolidCraft's named views with a template (docs/15 §10) ----

/// The template of SolidCraft's named view `view` (Top, Front, Right, Home or a saved view).
pub fn named_template(view: &str) -> Option<String> {
    current().named_templates.iter().find(|(v, _)| v == view).map(|(_, t)| t.clone())
}

/// Give SolidCraft's named view `view` template `template` (`None`: no template). `false` when
/// there is no such template.
pub fn set_named_template(view: &str, template: Option<&str>) -> bool {
    if template.is_some_and(|t| !template_exists(t)) {
        return false;
    }
    write(|w| {
        w.named_templates.retain(|(v, _)| v != view);
        if let Some(t) = template {
            w.named_templates.push((view.to_string(), t.to_string()));
        }
    });
    true
}

fn template_exists(name: &str) -> bool {
    templates().iter().any(|t| t.name == name)
}


/// SolidCraft's named view `view` was opened (its camera is set by SolidCraft): with a template,
/// the active window's view takes that template (linked, N3) – its V/G and result show at once.
/// Without one nothing changes.
pub fn named_view_opened(view: &str) {
    let Some(t) = named_template(view) else { return };
    let w = current();
    let Some(active_view) = w.view_of(w.active).map(|v| v.name.clone()) else { return };
    set_view_template(&active_view, Some(&t));
}

/// `factory.view.set_named_template`: give SolidCraft's named view `view` (Top, Front, Right,
/// Home or a saved view) template `template` (missing or empty: none).
pub fn view_set_named_template_cmd(_: &mut Ctx, params: &Value) -> Result<Value> {
    let view = params.get("view").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).ok_or_else(|| custom_bridge::Error("give the named view".into()))?;
    let template = params.get("template").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty() && *t != "<None>");
    if !set_named_template(view, template) {
        return Err(custom_bridge::Error(format!("no template {template:?}")));
    }
    Ok(json!({ "view": view, "template": template }))
}
