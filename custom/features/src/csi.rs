//! CSiBridge v25: the selection dialog's state and the data source behind it
//! (workflow and API: docs/08-csi-bridge.md in the Factory repository).
//!
//! The source is the csi-agent (a separate program using CSi's API). Until it exists,
//! [`AgentSource`] reports that; `FACTORY_CSI_DEMO=1` uses clearly marked demo data instead.

use std::sync::{LazyLock, PoisonError, RwLock};

use crate::model::{self, LoadCase, LoadCombination};

/// Run status of a load case (`Analyze.GetCaseStatus`: 1 Not run, 2 Could not start,
/// 3 Not finished, 4 Finished).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseStatus {
    NotRun,
    CouldNotStart,
    NotFinished,
    Finished,
}

impl CaseStatus {
    /// From the API's status number.
    pub fn from_api(status: i32) -> Self {
        match status {
            4 => Self::Finished,
            3 => Self::NotFinished,
            2 => Self::CouldNotStart,
            _ => Self::NotRun,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NotRun => "not run",
            Self::CouldNotStart => "could not start",
            Self::NotFinished => "not finished",
            Self::Finished => "finished",
        }
    }
}

/// What CSiBridge reports when the dialog opens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Selected area objects (`GetSelected`, ObjectType 5).
    pub shells: usize,
    /// Selected frame objects (`GetSelected`, ObjectType 2).
    pub frames: usize,
    /// Load cases: name, type, run status.
    pub cases: Vec<(String, String, CaseStatus)>,
    /// Load combinations: name, type, (load case or combination, scale factor).
    pub combos: Vec<(String, String, Vec<(String, f64)>)>,
}

/// Where the dialog's data comes from.
pub trait Source {
    /// Program and model as shown in the dialog title (e.g. "CSiBridge v25 – Bridge.bdb").
    fn title(&self) -> String;
    fn read(&self) -> Result<Snapshot, String>;
}

/// The csi-agent (not built yet).
pub struct AgentSource;

impl Source for AgentSource {
    fn title(&self) -> String {
        "CSiBridge".into()
    }
    fn read(&self) -> Result<Snapshot, String> {
        Err("The CSiBridge agent (csi-agent) is not built yet, so Factory cannot read CSiBridge. \
             See docs/08-csi-bridge.md §3. To try this dialog, start Factory with FACTORY_CSI_DEMO=1."
            .into())
    }
}

/// Demo data, marked as such everywhere it appears. Not from any real model.
pub struct DemoSource;

impl Source for DemoSource {
    fn title(&self) -> String {
        "CSiBridge – DEMO DATA".into()
    }
    fn read(&self) -> Result<Snapshot, String> {
        let case = |n: &str, t: &str, s| (n.to_string(), t.to_string(), s);
        let combo = |n: &str, f: &[(&str, f64)]| (n.to_string(), "Linear Additive".to_string(), f.iter().map(|(c, x)| (c.to_string(), *x)).collect());
        Ok(Snapshot {
            shells: 8000,
            frames: 1060,
            cases: vec![
                case("DEMO-DEAD", "Linear Static", CaseStatus::Finished),
                case("DEMO-SDL", "Linear Static", CaseStatus::Finished),
                case("DEMO-LM1", "Moving Load", CaseStatus::Finished),
                case("DEMO-TEMP", "Linear Static", CaseStatus::NotRun),
            ],
            combos: vec![
                combo("DEMO-ULS", &[("DEMO-DEAD", 1.35), ("DEMO-SDL", 1.35), ("DEMO-LM1", 1.35)]),
                combo("DEMO-SLS", &[("DEMO-DEAD", 1.0), ("DEMO-SDL", 1.0), ("DEMO-LM1", 1.0)]),
            ],
        })
    }
}

/// The source in use: demo when `FACTORY_CSI_DEMO=1`, else the agent.
pub fn source() -> Box<dyn Source> {
    if std::env::var("FACTORY_CSI_DEMO").is_ok_and(|v| v == "1") { Box::new(DemoSource) } else { Box::new(AgentSource) }
}

/// One line with a check box.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub kind: String,
    pub checked: bool,
    /// Why it cannot be ticked (e.g. "not run"); `None` = can be ticked.
    pub blocked: Option<String>,
    /// For combinations: their content.
    pub factors: Vec<(String, f64)>,
}

/// The dialog.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Closed,
    Error(String),
    Ready,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dialog {
    pub phase: Phase,
    pub title: String,
    pub shells: usize,
    pub frames: usize,
    pub cases: Vec<Item>,
    pub combos: Vec<Item>,
}

impl Dialog {
    /// The dialog for what `source` reports. Nothing is ticked at first.
    pub fn from_source(source: &dyn Source) -> Self {
        let title = source.title();
        match source.read() {
            Err(e) => Self { phase: Phase::Error(e), title, ..Default::default() },
            Ok(s) => Self {
                phase: Phase::Ready,
                title,
                shells: s.shells,
                frames: s.frames,
                cases: s
                    .cases
                    .into_iter()
                    .map(|(name, kind, status)| Item {
                        name,
                        kind,
                        checked: false,
                        blocked: (status != CaseStatus::Finished).then(|| status.label().to_string()),
                        factors: Vec::new(),
                    })
                    .collect(),
                combos: s.combos.into_iter().map(|(name, kind, factors)| Item { name, kind, checked: false, blocked: None, factors }).collect(),
            },
        }
    }

    /// Tick every item that can be ticked, or clear all.
    pub fn set_all(items: &mut [Item], on: bool) {
        for i in items {
            i.checked = on && i.blocked.is_none();
        }
    }

    pub fn ticked_cases(&self) -> usize {
        self.cases.iter().filter(|i| i.checked).count()
    }

    pub fn ticked_combos(&self) -> usize {
        self.combos.iter().filter(|i| i.checked).count()
    }

    /// Send is possible: something selected in CSiBridge and at least one case or combination ticked.
    pub fn can_send(&self) -> bool {
        self.phase == Phase::Ready && self.shells + self.frames > 0 && self.ticked_cases() + self.ticked_combos() > 0
    }

    /// Store the ticked cases and combinations in Factory's model and the selection in the status.
    /// Geometry and results follow when the csi-agent exists (docs/08-csi-bridge.md §4.4, §4.5).
    pub fn send(&self) -> bool {
        if !self.can_send() {
            return false;
        }
        let cases: Vec<LoadCase> = self
            .cases
            .iter()
            .filter(|i| i.checked)
            .map(|i| LoadCase { name: i.name.clone(), kind: i.kind.clone(), description: String::new() })
            .collect();
        let combos: Vec<LoadCombination> = self
            .combos
            .iter()
            .filter(|i| i.checked)
            .map(|i| LoadCombination { name: i.name.clone(), kind: i.kind.clone(), factors: i.factors.clone() })
            .collect();
        model::write(|f| {
            f.model.load_cases = cases;
            f.model.load_combinations = combos;
            f.status.program = "CSiBridge";
            f.status.selected_shells = self.shells;
            f.status.selected_frames = self.frames;
        });
        true
    }
}

static DIALOG: LazyLock<RwLock<Dialog>> = LazyLock::new(|| RwLock::new(Dialog::default()));

/// Open the dialog with the current source (CONNECT › CSiBridge).
pub fn open() {
    let dialog = Dialog::from_source(source().as_ref());
    model::write(|f| f.status.program = "CSiBridge");
    *DIALOG.write().unwrap_or_else(PoisonError::into_inner) = dialog;
}

/// A copy of the dialog (the UI edits it and stores it back with [`store`]).
pub fn dialog() -> Dialog {
    DIALOG.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Whether the dialog is open (cheap check for every frame).
pub fn is_open() -> bool {
    DIALOG.read().unwrap_or_else(PoisonError::into_inner).phase != Phase::Closed
}

pub fn store(dialog: Dialog) {
    *DIALOG.write().unwrap_or_else(PoisonError::into_inner) = dialog;
}

pub fn close() {
    store(Dialog::default());
}
