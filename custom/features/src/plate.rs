//! Example custom command: a rectangular plate, optionally with a centred through-hole.

use custom_bridge::{Ctx, Result, Value, json};

fn num(params: &Value, key: &str, default: f64) -> Result<f64> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("`{key}` must be a non-negative number").into()),
    }
}

pub fn run(ctx: &mut Ctx, params: &Value) -> Result<Value> {
    let width = num(params, "width", 80.0)?;
    let depth = num(params, "depth", 50.0)?;
    let thickness = num(params, "thickness", 10.0)?;
    let hole = num(params, "hole", 0.0)?;
    if width <= 0.0 || depth <= 0.0 || thickness <= 0.0 {
        return Err("width, depth and thickness must be greater than 0".into());
    }
    if hole >= width.min(depth) {
        return Err("the hole must be smaller than the plate".into());
    }

    ctx.exec("sketch.create", json!({"plane": "XY"}))?;
    ctx.exec("sketch.rectangle", json!({"p0": [-width / 2.0, -depth / 2.0], "p1": [width / 2.0, depth / 2.0]}))?;
    ctx.exec("sketch.finish", json!({}))?;
    let plate = ctx.exec("solid.extrude", json!({"distance": thickness}))?;

    if hole > 0.0 {
        ctx.exec("sketch.create", json!({"plane": "XY"}))?;
        ctx.exec("sketch.circle", json!({"center": [0.0, 0.0], "radius": hole / 2.0}))?;
        ctx.exec("sketch.finish", json!({}))?;
        ctx.exec("solid.extrude", json!({"distance": thickness, "operation": "cut"}))?;
    }

    Ok(json!({"width": width, "depth": depth, "thickness": thickness, "hole": hole, "extrude": plate}))
}
