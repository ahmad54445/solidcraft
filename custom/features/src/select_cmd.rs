//! The selection commands `factory.select.*` (docs/13 §1, rule R9). Called without parameters
//! (toolbar RESULTS › SELECT) a screen tool becomes the active tool and a data tool opens its
//! dialog; called with parameters (scripts, the agent tools) a data tool runs at once and opens
//! nothing. Screen tools need the view, so they are only activated here.

use std::sync::{LazyLock, PoisonError, RwLock};

use custom_bridge::{Ctx, Result, Value, json};

use crate::selection::{self, Kinds, Parallel, Scope};
use crate::{results_view, transfer};

/// The screen tool the 3D view uses (T1–T4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pointer,
    Poly,
    IntersectingPoly,
    IntersectingLine,
}

/// The data tool whose dialog is open (T5, T6, T7, T9, T10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    Coordinates,
    Parallel,
    Properties,
    Groups,
    Labels,
}

#[derive(Default)]
struct Ui {
    tool: Tool,
    dialog: Option<Dialog>,
}

static UI: LazyLock<RwLock<Ui>> = LazyLock::new(|| RwLock::new(Ui::default()));

pub fn tool() -> Tool {
    UI.read().unwrap_or_else(PoisonError::into_inner).tool
}

pub fn set_tool(t: Tool) {
    UI.write().unwrap_or_else(PoisonError::into_inner).tool = t;
}

pub fn dialog() -> Option<Dialog> {
    UI.read().unwrap_or_else(PoisonError::into_inner).dialog
}

pub fn set_dialog(d: Option<Dialog>) {
    UI.write().unwrap_or_else(PoisonError::into_inner).dialog = d;
}

/// Run `f` with the current geometry and what is shown (R1, R2).
pub fn with_scope<R>(f: impl FnOnce(Scope<'_>) -> R) -> R {
    let g = transfer::geometry();
    let d = results_view::snap_data();
    f(Scope { g: &g, shell_shown: &d.shell_shown, frame_shown: &d.frame_shown, kinds: selection::kinds() })
}

fn report() -> Result<Value> {
    let (shells, frames) = selection::current().counts();
    Ok(json!({ "selected": { "shells": shells, "frames": frames }, "message": selection::message() }))
}

fn tool_cmd(t: Tool) -> Result<Value> {
    set_tool(t);
    Ok(json!({ "tool": format!("{t:?}") }))
}

pub fn pointer(_: &mut Ctx, _: &Value) -> Result<Value> {
    tool_cmd(Tool::Pointer)
}
pub fn poly(_: &mut Ctx, _: &Value) -> Result<Value> {
    tool_cmd(Tool::Poly)
}
pub fn intersecting_poly(_: &mut Ctx, _: &Value) -> Result<Value> {
    tool_cmd(Tool::IntersectingPoly)
}
pub fn intersecting_line(_: &mut Ctx, _: &Value) -> Result<Value> {
    tool_cmd(Tool::IntersectingLine)
}

/// No parameters: open the dialog; else run.
fn dialog_or(params: &Value, d: Dialog, run: impl FnOnce(&Value) -> Result<Value>) -> Result<Value> {
    if params.as_object().is_none_or(|o| o.is_empty()) {
        set_dialog(Some(d));
        return Ok(json!({ "dialog": format!("{d:?}") }));
    }
    run(params)
}

fn range(v: &Value) -> Option<(f64, f64)> {
    let a = v.as_array()?;
    Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

/// `factory.select.coordinates {x?: [min, max], y?, z? (m), tolerance? (m)}`.
pub fn coordinates(_: &mut Ctx, params: &Value) -> Result<Value> {
    dialog_or(params, Dialog::Coordinates, |p| {
        let ranges = ["x", "y", "z"].map(|k| range(&p[k]));
        let tol = p["tolerance"].as_f64().unwrap_or(1e-6);
        selection::apply(with_scope(|s| selection::in_coordinates(s, ranges, tol)), false);
        report()
    })
}

/// `factory.select.parallel {axis: "X"|"Y"|"Z"} or {plane: "XY"|"XZ"|"YZ"}, tolerance? (degrees, 1)`.
pub fn parallel(_: &mut Ctx, params: &Value) -> Result<Value> {
    dialog_or(params, Dialog::Parallel, |p| {
        let axis = |s: &str| match s.to_ascii_uppercase().as_str() {
            "X" | "YZ" => Some([1.0, 0.0, 0.0]),
            "Y" | "XZ" => Some([0.0, 1.0, 0.0]),
            "Z" | "XY" => Some([0.0, 0.0, 1.0]),
            _ => None,
        };
        let to = match (p["axis"].as_str().and_then(axis), p["plane"].as_str().and_then(axis)) {
            (Some(a), _) => Parallel::To(a),
            (None, Some(n)) => Parallel::Plane(n),
            _ => return Err(custom_bridge::Error("give axis (X, Y, Z) or plane (XY, XZ, YZ)".into())),
        };
        selection::apply(with_scope(|s| selection::parallel(s, to, p["tolerance"].as_f64().unwrap_or(1.0))), false);
        report()
    })
}

/// `factory.select.properties {shell_sections?: [names], frame_sections?: [names], thickness?: [min, max] (m)}`.
pub fn properties(_: &mut Ctx, params: &Value) -> Result<Value> {
    dialog_or(params, Dialog::Properties, |p| {
        let g = transfer::geometry();
        let index = |list: &Vec<String>, key: &str| -> Vec<usize> {
            p[key].as_array().map(|a| a.iter().filter_map(Value::as_str).filter_map(|n| list.iter().position(|s| s == n)).collect()).unwrap_or_default()
        };
        let (shells, frames) = (index(&g.shell_sections, "shell_sections"), index(&g.frame_sections, "frame_sections"));
        selection::apply(with_scope(|s| selection::with_properties(s, &shells, &frames, range(&p["thickness"]))), false);
        report()
    })
}

/// T8 – not yet: which assignments CSiBridge gives us is open (docs/13 §7).
pub fn assignments(_: &mut Ctx, _: &Value) -> Result<Value> {
    Err(custom_bridge::Error("Select by Assignments is not built yet: the assignments are not transferred (docs/13 §7)".into()))
}

/// `factory.select.groups {names: [group names]}`.
pub fn groups(_: &mut Ctx, params: &Value) -> Result<Value> {
    dialog_or(params, Dialog::Groups, |p| {
        let wanted: Vec<&str> = p["names"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let members: Vec<String> = crate::model::read(|f| f.model.groups.iter().filter(|(n, _)| wanted.contains(&n.as_str())).flat_map(|(_, m)| m.clone()).collect());
        selection::apply(with_scope(|s| selection::in_groups(s, &members)), false);
        report()
    })
}

/// `factory.select.labels {text: "101-150, A1*"}`.
pub fn labels(_: &mut Ctx, params: &Value) -> Result<Value> {
    dialog_or(params, Dialog::Labels, |p| {
        let text = p["text"].as_str().unwrap_or_default();
        selection::apply(with_scope(|s| selection::with_labels(s, text)), false);
        report()
    })
}

pub fn all(_: &mut Ctx, _: &Value) -> Result<Value> {
    selection::apply(with_scope(selection::all), false);
    report()
}

pub fn invert(_: &mut Ctx, _: &Value) -> Result<Value> {
    with_scope(selection::invert);
    report()
}

pub fn clear(_: &mut Ctx, _: &Value) -> Result<Value> {
    selection::clear();
    report()
}

pub fn previous(_: &mut Ctx, _: &Value) -> Result<Value> {
    selection::previous();
    report()
}

/// `factory.select.deselect {on?: bool}` – toggles without a parameter.
pub fn deselect(_: &mut Ctx, params: &Value) -> Result<Value> {
    let on = params["on"].as_bool().unwrap_or(!selection::deselect_mode());
    selection::set_deselect_mode(on);
    Ok(json!({ "deselect": on }))
}

/// `factory.select.kinds {shells?: bool, frames?: bool}` (rule R1).
pub fn kinds(_: &mut Ctx, params: &Value) -> Result<Value> {
    let k = selection::kinds();
    let k = Kinds { shells: params["shells"].as_bool().unwrap_or(k.shells), frames: params["frames"].as_bool().unwrap_or(k.frames) };
    selection::set_kinds(k);
    Ok(json!({ "shells": k.shells, "frames": k.frames }))
}

/// `factory.select.save_group {name}` (C5): the selection becomes a group (Browser › Groups).
pub fn save_group(_: &mut Ctx, params: &Value) -> Result<Value> {
    let (name, count) = save_selection_as_group(params["name"].as_str().unwrap_or_default());
    Ok(json!({ "group": name, "members": count }))
}

/// Save the selection as group `name` (empty: "Selection n"); an existing group is replaced.
pub fn save_selection_as_group(name: &str) -> (String, usize) {
    let name = match name.trim() {
        "" => format!("Selection {}", crate::model::read(|f| f.model.groups.len()) + 1),
        n => n.to_owned(),
    };
    let members = selection::names(&transfer::geometry(), &selection::current());
    let count = members.len();
    crate::model::write(|f| match f.model.groups.iter_mut().find(|(n, _)| *n == name) {
        Some(g) => g.1 = members,
        None => f.model.groups.push((name.clone(), members)),
    });
    (name, count)
}
