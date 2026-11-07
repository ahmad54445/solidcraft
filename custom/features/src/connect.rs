//! CONNECT tab: get the structural model and results from an analysis program.
//!
//! Both commands are registered as planned: they appear in the CONNECT tab and the UI says
//! they are not supported yet. The adapters (docs/06-compatibility.md) replace these bodies.

use custom_bridge::{Ctx, Error, Result, Value};

/// MIDAS Civil NX: import model, load cases/combinations and results.
pub fn midas(_ctx: &mut Ctx, _params: &Value) -> Result<Value> {
    Err(Error::from("MIDAS Civil NX connection is not available yet"))
}

/// CSI (SAP2000 / CSiBridge): import model, load cases/combinations and results.
pub fn csi(_ctx: &mut Ctx, _params: &Value) -> Result<Value> {
    Err(Error::from("CSI connection is not available yet"))
}
