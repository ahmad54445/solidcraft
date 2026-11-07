//! Run after every upstream update: `cargo test -p custom-bridge`.
//! A failure names exactly which mapped SolidCraft command no longer exists.

use custom_bridge::names::COMMAND_MAP;

#[test]
fn every_mapped_command_still_exists_upstream() {
    let ids = custom_bridge::command_ids();
    let missing: Vec<String> = COMMAND_MAP
        .iter()
        .filter(|(_, id)| !ids.contains(id))
        .map(|(stable, id)| format!("`{stable}` -> `{id}` (fix custom/bridge/src/names.rs)"))
        .collect();
    assert!(missing.is_empty(), "SolidCraft no longer has these commands:\n{}", missing.join("\n"));
}

#[cfg(feature = "ui")]
#[test]
fn every_tab_name_still_exists_upstream() {
    let upstream = custom_bridge::ui::upstream_toolbar_tabs();
    let missing: Vec<&str> = custom_bridge::names::tabs::ALL.iter().copied().filter(|t| !upstream.contains(t)).collect();
    assert!(missing.is_empty(), "SolidCraft no longer has these toolbar tabs: {missing:?} (fix custom/bridge/src/names.rs)");
}
