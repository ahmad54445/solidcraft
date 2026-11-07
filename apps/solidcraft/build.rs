#![forbid(unsafe_code)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/app/windows/exports/solidcraft-executable.ico");
    // [CUSTOM] product icon and name from custom/branding when present
    let branding = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../custom/branding");
    println!("cargo:rerun-if-changed={}", branding.display());
    let icon = if branding.join("logo.ico").is_file() { branding.join("logo.ico") } else { icon };
    let product = std::fs::read_to_string(branding.join("name.txt")).map(|s| s.trim().to_string()).unwrap_or_else(|_| "SolidCraft".into());
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let copy = out.join("solidcraft.ico");
    std::fs::copy(icon, &copy)?;
    let rc = out.join("solidcraft.rc");
    let icon_line = format!("1 ICON \"{}\"\n", copy.display().to_string().replace('\\', "/"));
    let resource = version_resource(&std::env::var("CARGO_PKG_VERSION")?);
    let resource = if product == "SolidCraft" { resource } else { custom_names(&resource, &product) }; // [CUSTOM]
    std::fs::write(&rc, format!("{icon_line}{resource}"))?;
    embed_resource::compile_for(&rc, ["factory"], embed_resource::NONE).manifest_required()?; // [CUSTOM] bin name
    Ok(())
}

/// The Windows version resource (what Explorer's Details tab shows and what a launcher reads).
/// `FileVersion` and `ProductVersion` are the package version verbatim (`0.25.1`, `0.26.0-rc.1`):
/// ArtCraft's launcher parses the `ProductVersion` string as semver, so a fourth `.0` would not
/// read. The binary `FILEVERSION` fields need four numbers and take the leading `major.minor.patch`.
fn version_resource(version: &str) -> String {
    let mut numbers = version.split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse::<u16>().unwrap_or(0));
    let mut next = || numbers.next().unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    format!(
        "1 VERSIONINFO\nFILEVERSION {major},{minor},{patch},0\nPRODUCTVERSION {major},{minor},{patch},0\nFILEOS 0x40004\nFILETYPE 0x1\n{{\nBLOCK \"StringFileInfo\"\n{{\nBLOCK \"040904B0\"\n{{\nVALUE \"CompanyName\", \"SolidCraft contributors\"\nVALUE \"FileDescription\", \"SolidCraft, parametric 3D CAD\"\nVALUE \"FileVersion\", \"{version}\"\nVALUE \"InternalName\", \"solidcraft\"\nVALUE \"OriginalFilename\", \"solidcraft.exe\"\nVALUE \"ProductName\", \"SolidCraft\"\nVALUE \"ProductVersion\", \"{version}\"\n}}\n}}\nBLOCK \"VarFileInfo\"\n{{\nVALUE \"Translation\", 0x0409, 1200\n}}\n}}\n"
    )
}

/// [CUSTOM] The version resource with the product's name and executable instead of SolidCraft's.
fn custom_names(resource: &str, product: &str) -> String {
    let exe = product.to_lowercase();
    resource
        .replace("\"SolidCraft contributors\"", &format!("\"{product}\""))
        .replace("\"SolidCraft, parametric 3D CAD\"", &format!("\"{product}\""))
        .replace("\"solidcraft.exe\"", &format!("\"{exe}.exe\""))
        .replace("\"solidcraft\"", &format!("\"{exe}\""))
        .replace("\"SolidCraft\"", &format!("\"{product}\""))
}
