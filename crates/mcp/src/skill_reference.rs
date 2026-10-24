//! Read-only access to the skill documents shipped with DeSurface.

use serde_json::{Map, Value};

struct Skill {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
}

// Exact names only: these are embedded documents, never filesystem paths to open.
const SKILLS: &[Skill] = &[
    Skill {
        name: "desurface-checking",
        files: &[
            ("SKILL.md", include_str!("../../../skills/desurface-checking/SKILL.md")),
            ("reference/porting.md", include_str!("../../../skills/desurface-checking/reference/porting.md")),
        ],
    },
    Skill { name: "desurface-editing", files: &[("SKILL.md", include_str!("../../../skills/desurface-editing/SKILL.md"))] },
    Skill {
        name: "desurface-mechanisms",
        files: &[
            ("SKILL.md", include_str!("../../../skills/desurface-mechanisms/SKILL.md")),
            ("reference.md", include_str!("../../../skills/desurface-mechanisms/reference.md")),
        ],
    },
    Skill {
        name: "desurface-modeling",
        files: &[
            ("SKILL.md", include_str!("../../../skills/desurface-modeling/SKILL.md")),
            ("reference.md", include_str!("../../../skills/desurface-modeling/reference.md")),
        ],
    },
];

pub(crate) fn read(args: &Map<String, Value>) -> Result<Value, String> {
    let name = args.get("skill").and_then(Value::as_str).ok_or("`skill` is required and must be a string")?;
    let skill = SKILLS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Unknown skill. Available skills: {}", SKILLS.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")))?;
    let file = match args.get("file") {
        None | Some(Value::Null) => "reference.md",
        Some(Value::String(file)) => file,
        Some(_) => return Err("`file` must be a string".into()),
    };
    skill.files.iter().find(|(name, _)| *name == file).map(|(_, text)| Value::String((*text).into())).ok_or_else(|| {
        format!("Unknown file for {}. Available files: {}", skill.name, skill.files.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", "))
    })
}
