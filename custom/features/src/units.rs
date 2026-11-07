//! Display units: length, force, moment, stress. Values are stored in SI (m, N, N·m, N/m²);
//! these units only convert for display and input.

/// Length unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    Mm,
    Cm,
    M,
}

/// Force unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Force {
    N,
    KN,
    MN,
}

/// Moment unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    NMm,
    NM,
    KNM,
    MNM,
}

/// Stress unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stress {
    /// N/mm² (= MPa)
    NMm2,
    /// kN/m² (= kPa)
    KNM2,
    /// N/m² (= Pa)
    NM2,
}

impl Length {
    pub const ALL: [Self; 3] = [Self::Mm, Self::Cm, Self::M];
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::M => "m",
        }
    }
    /// Metres per unit.
    pub fn to_si(self) -> f64 {
        match self {
            Self::Mm => 1e-3,
            Self::Cm => 1e-2,
            Self::M => 1.0,
        }
    }
}

impl Force {
    pub const ALL: [Self; 3] = [Self::N, Self::KN, Self::MN];
    pub fn symbol(self) -> &'static str {
        match self {
            Self::N => "N",
            Self::KN => "kN",
            Self::MN => "MN",
        }
    }
    /// Newtons per unit.
    pub fn to_si(self) -> f64 {
        match self {
            Self::N => 1.0,
            Self::KN => 1e3,
            Self::MN => 1e6,
        }
    }
}

impl Moment {
    pub const ALL: [Self; 4] = [Self::NMm, Self::NM, Self::KNM, Self::MNM];
    pub fn symbol(self) -> &'static str {
        match self {
            Self::NMm => "N·mm",
            Self::NM => "N·m",
            Self::KNM => "kN·m",
            Self::MNM => "MN·m",
        }
    }
    /// Newton-metres per unit.
    pub fn to_si(self) -> f64 {
        match self {
            Self::NMm => 1e-3,
            Self::NM => 1.0,
            Self::KNM => 1e3,
            Self::MNM => 1e6,
        }
    }
}

impl Stress {
    pub const ALL: [Self; 3] = [Self::NMm2, Self::KNM2, Self::NM2];
    pub fn symbol(self) -> &'static str {
        match self {
            Self::NMm2 => "N/mm²",
            Self::KNM2 => "kN/m²",
            Self::NM2 => "N/m²",
        }
    }
    /// Pascals (N/m²) per unit.
    pub fn to_si(self) -> f64 {
        match self {
            Self::NMm2 => 1e6,
            Self::KNM2 => 1e3,
            Self::NM2 => 1.0,
        }
    }
}

/// The four display units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Units {
    pub length: Length,
    pub force: Force,
    pub moment: Moment,
    pub stress: Stress,
}

impl Default for Units {
    /// kN, m (moment kN·m, stress kN/m²).
    fn default() -> Self {
        Self { length: Length::M, force: Force::KN, moment: Moment::KNM, stress: Stress::KNM2 }
    }
}

impl Units {
    /// The status bar's short form: force and length, e.g. "kN, m".
    pub fn short(&self) -> String {
        format!("{}, {}", self.force.symbol(), self.length.symbol())
    }
}
