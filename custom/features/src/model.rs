//! Factory's structural model summary and status, as the UI shows them.
//!
//! The analysis-program adapters (MIDAS, CSI) fill this when they import a model. Until then
//! every count is 0. Heavy data (node coordinates, element results) will live in flat arrays
//! elsewhere; this holds only what the browser and the status bar read.

use std::sync::{LazyLock, PoisonError, RwLock};

use crate::units::Units;

/// The boundary kinds, in browser order.
pub const BOUNDARY_KINDS: [&str; 11] = [
    "Supports",
    "Point Spring Supports",
    "General Spring Supports",
    "Surface Spring Supports",
    "Elastic Link",
    "General Link",
    "Change General Link Property",
    "Beam End Release",
    "Beam End Offsets",
    "Plate End Release",
    "Rigid Link",
];

/// A load case.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadCase {
    pub name: String,
    /// e.g. "Dead Load", "Live Load", "Temperature".
    pub kind: String,
    pub description: String,
}

/// A load combination: load cases (or other combinations) with their factors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadCombination {
    pub name: String,
    /// e.g. "Add", "Envelope", "SRSS".
    pub kind: String,
    pub factors: Vec<(String, f64)>,
}

/// What the browser's Structure tree counts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub nodes: usize,
    pub elements: usize,
    pub node_local_axes: usize,
    pub materials: usize,
    pub sections: usize,
    pub thicknesses: usize,
    /// One count per [`BOUNDARY_KINDS`] entry.
    pub boundaries: [usize; 11],
    pub load_cases: Vec<LoadCase>,
    pub load_combinations: Vec<LoadCombination>,
}

impl Model {
    /// Materials + sections + thicknesses.
    pub fn properties(&self) -> usize {
        self.materials + self.sections + self.thicknesses
    }

    /// All boundary entries.
    pub fn boundaries_total(&self) -> usize {
        self.boundaries.iter().sum()
    }
}

/// Connection to the analysis program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Connection {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

impl Connection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
        }
    }
}

/// State of the backend (import, results, design).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    #[default]
    Ready,
    Busy(String),
    Error(String),
}

/// What the status bar shows besides units and memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub selected_shells: usize,
    pub selected_frames: usize,
    /// The load case or combination whose results are shown.
    pub current_load: Option<String>,
    /// The connected (or last chosen) analysis program.
    pub program: &'static str,
    pub connection: Connection,
    pub backend: Backend,
    /// Result cache in use (MB).
    pub cache_used_mb: u64,
    /// Result cache budget (MB), see docs/07-scalability.md.
    pub cache_budget_mb: u64,
}

/// Default result-cache budget until the memory budget is configurable (docs/07-scalability.md).
pub const DEFAULT_CACHE_BUDGET_MB: u64 = 4096;

impl Default for Status {
    fn default() -> Self {
        Self {
            selected_shells: 0,
            selected_frames: 0,
            current_load: None,
            program: "MIDAS",
            connection: Connection::Disconnected,
            backend: Backend::Ready,
            cache_used_mb: 0,
            cache_budget_mb: DEFAULT_CACHE_BUDGET_MB,
        }
    }
}

/// Everything Factory's UI reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Factory {
    pub model: Model,
    pub status: Status,
    pub units: Units,
}

static STATE: LazyLock<RwLock<Factory>> = LazyLock::new(|| RwLock::new(Factory::default()));

/// Read the state.
pub fn read<R>(f: impl FnOnce(&Factory) -> R) -> R {
    f(&STATE.read().unwrap_or_else(PoisonError::into_inner))
}

/// Change the state (adapters after an import, the UI for units).
pub fn write<R>(f: impl FnOnce(&mut Factory) -> R) -> R {
    f(&mut STATE.write().unwrap_or_else(PoisonError::into_inner))
}
