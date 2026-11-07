//! Runs the custom command through SolidCraft's real engine (headless).
//! The test talks to SolidCraft only through the bridge, like the features do.

use custom_bridge::json;

#[test]
fn plate_command_is_registered_and_builds() {
    custom_features::install();
    assert!(custom_bridge::command_ids().contains(&"custom.plate"));
    let out = custom_bridge::testing::run_fresh("custom.plate", json!({"width": 60, "depth": 40, "thickness": 8, "hole": 10}));
    assert!(out.is_ok(), "custom.plate failed: {out:?}");
}

#[test]
fn plate_refuses_bad_input() {
    custom_features::install();
    let out = custom_bridge::testing::run_fresh("custom.plate", json!({"width": -5}));
    assert!(out.is_err());
}
