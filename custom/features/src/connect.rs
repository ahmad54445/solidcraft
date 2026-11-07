//! CONNECT tab: get the structural model and results from an analysis program.
//!
//! Both commands are registered as planned: they appear in the CONNECT tab and the UI says
//! they are not supported yet. The adapters (docs/06-compatibility.md) replace these bodies.

use custom_bridge::{Ctx, Error, Result, Value};

/// MIDAS Civil NX: import model, load cases/combinations and results.
pub fn midas(_ctx: &mut Ctx, _params: &Value) -> Result<Value> {
    Err(Error::from("MIDAS Civil NX connection is not available yet"))
}

/// CSiBridge v25: opens the selection dialog (load cases / combinations with check boxes).
/// Workflow and API calls: docs/08-csi-bridge.md. SAP2000 later through the same agent.
pub fn csi(_ctx: &mut Ctx, _params: &Value) -> Result<Value> {
    crate::csi::open();
    Ok(custom_bridge::json!({ "dialog": "csi" }))
}
