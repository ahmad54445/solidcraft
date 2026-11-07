//! [CUSTOM] Fork hook: commands registered at runtime by the `custom/` crates.
//!
//! This is the only engine file that belongs to the fork. Upstream code knows nothing about the
//! custom crates; the apps call `custom_features::install()` at start-up, which ends up here.
//! Built-in commands are listed first in `command_specs`, so a custom command can never silently
//! replace one: an id that already exists is found as the built-in.

use std::sync::{PoisonError, RwLock};

use super::CommandSpec;

static EXTRA: RwLock<Vec<&'static CommandSpec>> = RwLock::new(Vec::new());

/// Register extra commands (idempotent: an id registered before is skipped).
pub fn register_extension_commands(commands: &'static [CommandSpec]) {
    let mut extra = EXTRA.write().unwrap_or_else(PoisonError::into_inner);
    for c in commands {
        if !extra.iter().any(|e| e.id == c.id) {
            extra.push(c);
        }
    }
}

/// The registered extra commands, in registration order.
pub(super) fn commands() -> Vec<&'static CommandSpec> {
    EXTRA.read().unwrap_or_else(PoisonError::into_inner).clone()
}
