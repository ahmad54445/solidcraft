//! Visibility/Graphics settings (docs/12-visibility-graphics.md): categories, display items and
//! filters. Changing them never changes the geometry; the view is rebuilt from the same arrays
//! on a worker thread (rules G1–G7, phase 1 through SolidCraft's point-cloud path).

use std::sync::{LazyLock, PoisonError, RwLock};

use crate::transfer::Geometry;

/// A subcategory row nested under a category (docs/12 §2.1): an annotation on its objects
/// (labels, section names, symbols, local axes).
#[derive(Clone, Debug, PartialEq)]
pub struct Sub {
    pub visible: bool,
    /// Colour of the text / symbol.
    pub colour: [u8; 3],
    pub halftone: bool,
}

impl Sub {
    pub fn off(colour: [u8; 3]) -> Self {
        Self { visible: false, colour, halftone: false }
    }

    /// The colour to draw with (faded when halftone).
    pub fn ink(&self) -> [u8; 3] {
        if self.halftone { halftone(self.colour) } else { self.colour }
    }
}

/// One category row (Model Categories tab).
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub visible: bool,
    /// Line colour (frames; element edges later).
    pub line: [u8; 3],
    /// Surface colour when no contour is shown.
    pub surface: [u8; 3],
    /// Drawn faded toward the background, without contour.
    pub halftone: bool,
    /// Subcategory: element names on the elements (CSiBridge Display Options › Labels).
    pub labels: Sub,
    /// Subcategory: section / area property names (Display Options › Sections).
    pub sections: Sub,
    /// Subcategory: local-axis triads (Display Options › Local Axes); colours are fixed
    /// (1 red, 2 green, 3 blue), halftone fades them.
    pub axes: Sub,
}

/// What a filter rule matches.
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    /// Shells and frames whose section / area property has this name.
    SectionIs(String),
    /// Shells thinner than this (m).
    ThicknessBelow(f64),
    /// Shells thicker than this (m).
    ThicknessAbove(f64),
}

/// What a filter does to the elements it matches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Hide,
    Halftone,
    /// Plain colour, no contour.
    Colour([u8; 3]),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub enabled: bool,
    pub rule: Rule,
    pub action: Action,
}

/// View Type (tab View Options, docs/12 §2.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewType {
    /// Frames on their centre line, shells on their mid-surface.
    #[default]
    Standard,
    /// Shells as slabs of their thickness along local axis 3 (frames stay lines until their
    /// section dimensions are transferred).
    Extrude,
}

/// How all objects are drawn (tab View Options › View Type, General).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewOptions {
    pub view_type: ViewType,
    /// Every element slightly smaller around its centre, so element borders show.
    pub shrink: bool,
    /// Surfaces filled; off = outlines only.
    pub fill: bool,
    /// Element edges as lines (in the category's line colour).
    pub edges: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self { view_type: ViewType::Standard, shrink: false, fill: true, edges: false }
    }
}

/// Shrink factor of Shrink Objects (size kept around the element centre).
pub const SHRINK: f64 = 0.85;

/// All settings of the (current) view.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Tab View Options.
    pub view: ViewOptions,
    pub shells: Category,
    pub frames: Category,
    /// Results tab.
    pub contour: bool,
    /// Annotation tab.
    pub legend: bool,
    pub values: bool,
    pub filters: Vec<Filter>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view: ViewOptions::default(),
            shells: Category {
                visible: true,
                line: [60, 60, 70],
                surface: [190, 196, 206],
                halftone: false,
                labels: Sub::off([40, 40, 48]),
                sections: Sub::off([30, 90, 160]),
                axes: Sub::off([0, 0, 0]),
            },
            frames: Category {
                visible: true,
                line: [70, 72, 84],
                surface: [70, 72, 84],
                halftone: false,
                labels: Sub::off([40, 40, 48]),
                sections: Sub::off([30, 90, 160]),
                axes: Sub::off([0, 0, 0]),
            },
            contour: true,
            legend: true,
            values: false,
            filters: Vec::new(),
        }
    }
}

/// How one element is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Hidden,
    /// Coloured by the result (or the category colour when the contour is off).
    Normal,
    Halftone,
    Colour([u8; 3]),
}

/// The look of every shell and frame. Category first, then the filters in order (a later
/// filter wins over an earlier one); one pass over the flat arrays (rule G3).
pub fn looks(g: &Geometry, s: &Settings) -> (Vec<Look>, Vec<Look>) {
    let base = |c: &Category| match (c.visible, c.halftone) {
        (false, _) => Look::Hidden,
        (true, true) => Look::Halftone,
        (true, false) => Look::Normal,
    };
    let mut shells = vec![base(&s.shells); g.shells.len()];
    let mut frames = vec![base(&s.frames); g.frames.len()];
    for f in s.filters.iter().filter(|f| f.enabled) {
        let apply = |look: &mut Look| {
            if *look == Look::Hidden {
                return; // a hidden category stays hidden
            }
            *look = match f.action {
                Action::Hide => Look::Hidden,
                Action::Halftone => Look::Halftone,
                Action::Colour(c) => Look::Colour(c),
            };
        };
        for (e, sh) in g.shells.iter().enumerate() {
            let section = sh[4] as usize;
            let matches = match &f.rule {
                Rule::SectionIs(name) => g.shell_sections.get(section) == Some(name),
                Rule::ThicknessBelow(t) => g.shell_thickness.get(section).is_some_and(|x| x < t),
                Rule::ThicknessAbove(t) => g.shell_thickness.get(section).is_some_and(|x| x > t),
            };
            if matches {
                apply(&mut shells[e]);
            }
        }
        if let Rule::SectionIs(name) = &f.rule {
            for (e, fr) in g.frames.iter().enumerate() {
                if g.frame_sections.get(fr[2] as usize) == Some(name) {
                    apply(&mut frames[e]);
                }
            }
        }
    }
    (shells, frames)
}

/// A colour faded toward a light background (halftone).
pub fn halftone([r, g, b]: [u8; 3]) -> [u8; 3] {
    let fade = |c: u8| ((u16::from(c) + 2 * 235) / 3) as u8;
    [fade(r), fade(g), fade(b)]
}

static SETTINGS: LazyLock<RwLock<Settings>> = LazyLock::new(|| RwLock::new(Settings::default()));

/// What the overlay draws, read every frame without cloning the filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overlay {
    pub legend: bool,
    pub values: bool,
    /// Colour to draw each subcategory with; `None` = not shown (its category or itself off).
    pub shell_labels: Option<[u8; 3]>,
    pub shell_sections: Option<[u8; 3]>,
    pub frame_labels: Option<[u8; 3]>,
    pub frame_sections: Option<[u8; 3]>,
}

pub fn overlay() -> Overlay {
    let s = SETTINGS.read().unwrap_or_else(PoisonError::into_inner);
    Overlay {
        legend: s.legend,
        values: s.values,
        // A hidden category hides its subcategories; they keep their own setting (docs/12 §2.1).
        shell_labels: (s.shells.visible && s.shells.labels.visible).then(|| s.shells.labels.ink()),
        shell_sections: (s.shells.visible && s.shells.sections.visible).then(|| s.shells.sections.ink()),
        frame_labels: (s.frames.visible && s.frames.labels.visible).then(|| s.frames.labels.ink()),
        frame_sections: (s.frames.visible && s.frames.sections.visible).then(|| s.frames.sections.ink()),
    }
}

pub fn settings() -> Settings {
    SETTINGS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Commit new settings (OK / Apply, rule G7) and redraw the view once.
pub fn apply(s: Settings) {
    let changed = {
        let mut current = SETTINGS.write().unwrap_or_else(PoisonError::into_inner);
        let changed = *current != s;
        *current = s;
        changed
    };
    if changed {
        crate::results_view::refresh();
    }
}
