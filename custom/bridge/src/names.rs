//! Stable names -> SolidCraft command ids.
//!
//! Custom code calls SolidCraft by the stable names on the left. When upstream renames a
//! command, change only the right-hand side here; `cargo test -p custom-bridge` lists every
//! id below that no longer exists.

/// (stable name used by custom code, SolidCraft command id)
pub const COMMAND_MAP: &[(&str, &str)] = &[
    ("sketch.create", "sketch.create"),
    ("sketch.rectangle", "sketch.rectangle.two_point"),
    ("sketch.circle", "sketch.circle.center"),
    ("sketch.finish", "sketch.finish"),
    ("solid.extrude", "solid.extrude"),
    ("solid.fillet", "solid.fillet"),
    ("parameter.set", "parameters.change"),
];

/// The SolidCraft id for a stable name. Names not in the map pass through unchanged, so any
/// SolidCraft command can still be called directly.
pub fn resolve(name: &str) -> &str {
    COMMAND_MAP.iter().find(|(stable, _)| *stable == name).map_or(name, |(_, id)| id)
}

/// Toolbar tab names (SolidCraft's ribbon tab ids). Used with [`crate::ui::show_toolbar_tabs`].
pub mod tabs {
    pub const MODEL: &str = "MODEL";
    pub const SURFACE: &str = "SURFACE";
    pub const ASSEMBLE: &str = "ASSEMBLE";
    pub const INSERT: &str = "INSERT";
    pub const INSPECT: &str = "INSPECT";
    pub const MANAGE: &str = "MANAGE";
    pub const VIEW: &str = "VIEW";
    pub const SHEET_METAL: &str = "SHEET METAL";
    pub const WELDMENTS: &str = "WELDMENTS";

    /// Every name above, for the upstream contract test.
    pub const ALL: &[&str] = &[MODEL, SURFACE, ASSEMBLE, INSERT, INSPECT, MANAGE, VIEW, SHEET_METAL, WELDMENTS];
}
