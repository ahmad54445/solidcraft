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
| `Cargo.toml` | `"custom/*"` in `members` |
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

## Oberfläche anpassen (Feature `ui`, nur Desktop-App)

In `custom/features/src/lib.rs`:

- `APP_NAME` = Produktname (Fenstertitel, Anwendungsleiste), derzeit `"Factory"`
- `TOOLBAR_TABS` = einzige sichtbare Toolbar-Tabs, derzeit nur `MODEL`
  (Namen aus `custom/bridge/src/names.rs`, Modul `tabs`)

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
