# Eigene Erweiterungen (Fork von SolidCraft)

Dieser Ordner enthält den eigenen Code. Der übrige Code ist das Original-SolidCraft
(`upstream` = https://github.com/tangency-labs/solidcraft) und wird möglichst nicht verändert.

## Aufbau

```
SolidCraft (upstream, unverändert bis auf 7 markierte Zeilen + 1 Datei)
        ▲
        │  nur hier wird SolidCraft-Intern verwendet
custom/bridge     ← Zwischenschicht (Adapter)
        ▲
        │  nur stabile API: Ctx::exec, commands!, register
custom/features   ← das eigene Programm
```

| Regel | Warum |
|---|---|
| `custom/features` hängt **nur** von `custom-bridge` ab, nie von `solidcraft-*` | Upstream-Änderungen brechen nur die Bridge |
| SolidCraft-Befehle über stabile Namen aufrufen (`custom/bridge/src/names.rs`) | Umbenennung upstream = eine Zeile ändern |
| Eingriffe in Upstream-Dateien nur mit Kommentar `[CUSTOM]` | Konflikte beim Update sofort auffindbar |

## Eingriffe in Upstream-Dateien (vollständige Liste)

| Datei | Änderung |
|---|---|
| `crates/engine/src/cmd/custom_hook.rs` | neue Datei: Laufzeit-Registrierung von Befehlen |
| `crates/engine/src/cmd/mod.rs` | `mod custom_hook;`, `v.extend(custom_hook::commands());`, `pub use …register_extension_commands;` |
| `apps/solidcraft/Cargo.toml`, `apps/solidcraft-cli/Cargo.toml` | Abhängigkeit `custom-features` |
| `apps/solidcraft/src/main.rs`, `apps/solidcraft-cli/src/main.rs` | `custom_features::install();` am Anfang von `main` |
| `apps/solidcraft/Cargo.toml` | `custom-features` mit `features = ["ui"]` |
| `apps/solidcraft/src/main.rs` | Fenstertitel aus `custom_hook::app_name()` |
| `crates/ui-egui/src/custom_hook.rs` | neue Datei: sichtbare Toolbar-Tabs und Produktname |
| `crates/ui-egui/src/lib.rs` | `pub mod custom_hook;` |
| `crates/ui-egui/src/workspace.rs` | `choosable_tabs()` filtert über `custom_hook::tab_allowed` |
| `crates/ui-egui/src/toolbar.rs` | Name in der Anwendungsleiste aus `custom_hook::app_name()` |
| `Cargo.toml` (Workspace) | `members` nennt `custom/bridge` und `custom/features` ausdrücklich (statt `custom/*`) |
| `apps/solidcraft/Cargo.toml` | `[[bin]] name = "factory"`, `default-run = "factory"` (Paketname bleibt `solidcraft`) |
| `apps/solidcraft/build.rs` | Icon und Versionsangaben der EXE aus `custom/branding` (Fallback: SolidCraft) |
| `apps/solidcraft/src/main.rs` | Fenster-/Taskleisten-Icon aus `custom_hook::logo_rgba()` (Fallback: SolidCraft-Icon) |
| `crates/ui-egui/src/toolbar.rs` | Logo in der Anwendungsleiste über `custom_hook::draw_logo` (Fallback: SolidCraft-Marke) |
| `crates/ui-egui/src/workspace.rs` | Zusatz-Tabs (`custom_hook::extra_tabs`/`extra_sections`) in `choosable_tabs`, `visible_tabs`, `tab_icon`, `panels`, `legacy_render_destination`, `layout` |
| `crates/ui-egui/src/browser.rs` | `origin_rows` bricht ab, wenn `custom_hook::browser_hidden("origin")` |
| `crates/ui-egui/src/browser.rs` | `component_rows_inner`: nach Origin `custom_hook::browser_rows()` (Structure, Linked, Design, Groups); Canvases, Point Clouds, Analysis, Bodies, Sketches, Construction, Features, Joints, Komponenten nur, wenn nicht über `custom_hook::browser_hidden` ausgeblendet |
| `crates/ui-egui/src/status_bar.rs` | `shown()`/`height()`: eigene Leiste immer sichtbar; SolidCraft-Leiste darüber, wenn ihre Einstellung an ist |
| `crates/ui-egui/src/panel_runtime/app.rs` | Höhe der Statusleiste aus `status_bar::height()` |
| `crates/ui-egui/src/viewport.rs` | `Frame.clouds` + `custom_hook::extra_clouds()`; Overlay über der Ansicht mit `ViewInfo` (Projektion, Kamera); Fit view + `custom_hook::extra_bounds()` |
| `crates/ui-egui/src/i18n.rs` | `Catalog::shown()` ersetzt „SolidCraft“ durch den Produktnamen (alle Sprachen); englischer Fallback über `shown()` |

## Oberfläche anpassen (Feature `ui`, nur Desktop-App)

In `custom/features/src/lib.rs`:

- `APP_NAME` = Produktname (Fenstertitel, Anwendungsleiste, alle Texte), gelesen aus `custom/branding/name.txt`, derzeit `Factory`
- `TOOLBAR_TABS` = einzige sichtbare Toolbar-Tabs, derzeit nur `MODEL`
  (Namen aus `custom/bridge/src/names.rs`, Modul `tabs`)
- `EXTRA_TABS` = eigene Toolbar-Tabs mit Panels und Befehlen, derzeit `CONNECT` › `PROGRAMS` (MIDAS Civil NX, CSiBridge v25; beide noch „planned“, siehe `docs/08-csi-bridge.md` im Factory-Repo)
- `HIDDEN_BROWSER_ITEMS` = ausgeblendete Browser-Ordner, derzeit Bodies, Sketches, Construction, Canvases, Point Clouds, Analysis, Features, Joints, Komponenten (Origin ist wieder sichtbar)
- Browser: `ui_browser.rs` (Structure, Linked, Design, Groups nach Origin; Expand All / Collapse All; Load Case / Load Combination › Properties); ausgeblendete SolidCraft-Zeilen: `HIDDEN_BROWSER_ITEMS`
- Statusleiste: `ui_status.rs` (Selected, LC, Units, Programm, RAM/Cache, Backend)
- Daten: `model.rs` (Zählungen, Lastfälle, Kombinationen, Status), `units.rs` (Länge, Kraft, Moment, Spannung)
- Name und Logo: `custom/branding/` (`name.txt`, `logo.png`, `logo.ico`; `make_logo.py` erzeugt das Platzhalter-Logo)

Prüfen: `git grep -n "\[CUSTOM\]" -- . ":(exclude)custom"`

## Neuen Befehl hinzufügen

1. Funktion in `custom/features/src/<name>.rs`: `pub fn run(ctx: &mut Ctx, params: &Value) -> Result<Value>`
2. Eine Zeile in `COMMANDS` in `custom/features/src/lib.rs`
3. Test in `custom/features/tests/`

Der Befehl erscheint danach in der Befehlssuche der App, in `solidcraft-cli exec`, in Skripten und über MCP.
Beispiel: `custom.plate` (`custom/features/src/plate.rs`).

## SolidCraft aktualisieren

```powershell
powershell -ExecutionPolicy Bypass -File custom\update-upstream.ps1            # neuester Stand (main)
powershell -ExecutionPolicy Bypass -File custom\update-upstream.ps1 -Ref v0.40.0 # bestimmte Version
```

Das Skript holt upstream, merged in den Branch `factory` und führt
`cargo test -p custom-bridge -p custom-features --features custom-bridge/ui` aus. Der Test `upstream_contract` nennt jeden
SolidCraft-Befehl aus `names.rs`, der nicht mehr existiert. Korrekturen erfolgen in `custom/bridge`.

## Git

- `main` = unverändertes Upstream, `factory` = Arbeitsbranch des Forks (wird täglich per `.github/workflows/factory-sync-upstream.yml` mit upstream zusammengeführt)
- Remote `upstream` = Original. Für ein eigenes Repository: `git remote add origin <URL>` und `git push -u origin factory`

## Bauen / Starten

```powershell
cargo run -p solidcraft --release                  # App mit eigenen Befehlen
cargo run -p solidcraft-cli -- exec custom.plate '{\"width\":60,\"hole\":10}'
```
