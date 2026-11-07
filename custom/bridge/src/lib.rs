//! [CUSTOM] The adapter layer between the fork's own code and SolidCraft.
//!
//! Rule: `custom/features` (your program) depends ONLY on this crate, never on `solidcraft-*`
//! crates. Everything that touches SolidCraft internals lives here, so an upstream change is
//! fixed in this one crate (usually `names.rs`), and the features keep compiling unchanged.
//!
//! What it offers:
//! - [`Ctx`]: what a custom command gets; [`Ctx::exec`] runs any SolidCraft command as JSON.
//! - [`commands!`]: declares custom commands without naming any SolidCraft type.
//! - [`register`]: hands the declared commands to SolidCraft (called from `install()`).

pub mod names;
#[cfg(feature = "ui")]
pub mod ui;

pub use serde_json::{Value, json};

/// Re-exports for the macros only; not for custom code.
#[doc(hidden)]
pub mod __private {
    pub use solidcraft_engine as engine;
}

/// An error from a custom command or from SolidCraft, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error(s.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The session a custom command runs in.
pub struct Ctx<'a> {
    session: &'a mut solidcraft_engine::Session,
}

impl<'a> Ctx<'a> {
    #[doc(hidden)]
    pub fn __new(session: &'a mut solidcraft_engine::Session) -> Self {
        Ctx { session }
    }

    /// Run a SolidCraft command by its stable name (see [`names`]) or its SolidCraft id.
    pub fn exec(&mut self, command: &str, params: Value) -> Result<Value> {
        self.session.execute(names::resolve(command), &params).map_err(|e| Error(e.to_string()))
    }

    /// Whether SolidCraft knows this command (stable name or id).
    pub fn has_command(&self, command: &str) -> bool {
        solidcraft_engine::cmd::find_command(names::resolve(command)).is_some()
    }

    /// The value of a user parameter, by name, if the design has one.
    pub fn parameter(&mut self, name: &str) -> Option<f64> {
        let list = self.exec("parameters.list", json!({})).ok()?;
        find_number(&list, name)
    }
}

fn find_number(v: &Value, name: &str) -> Option<f64> {
    match v {
        Value::Object(m) if m.get("name").and_then(Value::as_str) == Some(name) => m.get("value").and_then(Value::as_f64),
        Value::Object(m) => m.values().find_map(|x| find_number(x, name)),
        Value::Array(a) => a.iter().find_map(|x| find_number(x, name)),
        _ => None,
    }
}

/// The command list type `commands!` produces (opaque to custom code).
pub type CommandList = &'static [solidcraft_engine::cmd::CommandSpec];

/// Make the commands known to SolidCraft (UI, CLI, MCP, scripts). Safe to call more than once.
pub fn register(commands: CommandList) {
    solidcraft_engine::cmd::register_extension_commands(commands);
}

/// Every command id SolidCraft knows (built-in and registered).
pub fn command_ids() -> Vec<&'static str> {
    solidcraft_engine::cmd::command_specs().iter().map(|c| c.id).collect()
}

/// Helpers for tests of custom commands (headless, no window).
pub mod testing {
    use super::{Error, Result, Value, names};

    /// Run one command (stable name or id) in a new, empty design and return its result.
    pub fn run_fresh(command: &str, params: Value) -> Result<Value> {
        let mut session = solidcraft_engine::Session::default();
        session.execute(names::resolve(command), &params).map_err(|e| Error(e.to_string()))
    }
}

/// Declare custom commands. Each `run` is `fn(&mut Ctx, &Value) -> bridge::Result<Value>`.
/// `planned: "…"` marks a command that is registered but not built yet (the UI says so).
///
/// ```ignore
/// custom_bridge::commands! {
///     pub static COMMANDS = [
///         { id: "custom.plate", label: "Plate", run: plate, icon: "extrude", params: "width, depth, thickness" },
///     ];
/// }
/// ```
#[macro_export]
macro_rules! commands {
    (
        $vis:vis static $name:ident = [
            $( { id: $id:literal, label: $label:literal, run: $run:path
                 $(, icon: $icon:literal)? $(, params: $params:literal)? $(, planned: $planned:literal)? $(,)? } ),* $(,)?
        ];
    ) => {
        $vis static $name: $crate::CommandList = &[ $( {
            fn __run(
                session: &mut $crate::__private::engine::Session,
                params: &$crate::Value,
            ) -> $crate::__private::engine::Result<$crate::Value> {
                let mut ctx = $crate::Ctx::__new(session);
                $run(&mut ctx, params).map_err(|e: $crate::Error| $crate::__private::engine::EngineError::Other(e.0))
            }
            $crate::__private::engine::cmd::CommandSpec::new($id, $label, __run)
                $(.icon($icon))? $(.params($params))? $(.planned($planned))?
        } ),* ];
    };
}
