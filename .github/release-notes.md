DeSurface is open-source parametric 3D CAD in Rust: sketch, constrain, extrude, and change a
parameter to watch the timeline rebuild. [Documentation](https://docs.solidcraft.io/)
· [try it in the browser](https://solidcraft.io/app) ·
[contributing](https://github.com/bherbruck/solidcraft/blob/main/CONTRIBUTING.md) ·
[issues](https://github.com/bherbruck/solidcraft/issues). AI agents can model with it over MCP:
`desurface-cli mcp` ([how](https://docs.solidcraft.io/guide/mcp.html)).

**What's new** in DeSurface {version} is listed below, and the
[ROADMAP](https://github.com/bherbruck/solidcraft/blob/main/ROADMAP.md#known-limitations) lists
the known limitations. Designs from every earlier version open in this one.

**Upgrading from 0.7 or earlier.** 0.8 changed the Linux app ID to `ai.storyteller.solidcraft`:
`flatpak update` offers to replace the old app, and the `.deb`, `.rpm` and AppImage remove the old
menu entry.

**Upgrading from 0.5 or earlier.** 0.6 renamed SolveCraft to DeSurface and made the zip
container the default save format. `.solvecraft` designs, old settings folders and
`SOLVECRAFT_*` variables are still picked up, and `solvecraft-cli` still forwards to
`desurface-cli`. Want plain JSON? Use **File ▸ Save As JSON…** or `--save design.json`. On
macOS, replace `SolveCraft.app` with `DeSurface.app`. Details:
[compatibility notes](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md).

Documentation, the browser app and the Flatpak feed use the DeSurface project Pages URLs.

**Which file?**
- **Windows:** `DeSurface-{version}-windows-x64.exe` runs as is. Or install the `.msi`, or unzip the
  `-portable.zip` (x64, x86 and arm64).
- **macOS 11+** (Apple silicon and Intel): open the `.dmg` and drag `DeSurface.app` onto Applications.
  The CLI is in `desurface-cli-{version}-macos-universal.zip`.
- **Linux** (x86_64 and aarch64, glibc 2.35+): the `.AppImage` (`chmod +x`, then run it), the
  `.deb` or `.rpm`, the `.flatpak` (`flatpak install --user <file>`), the `.snap`
  (`sudo snap install --dangerous <file>`, or `sudo snap install desurface` from the Snap Store), or the `.tar.gz`.
- **FreeBSD:** `tar -xzf desurface-{version}-freebsd-x86_64.tar.gz --strip-components 1 -C /usr/local`.
- **Web:** `desurface-web-{version}.zip` is a static site to host yourself.
- `SHA256SUMS.txt`: `sha256sum -c SHA256SUMS.txt --ignore-missing`.
- Files without a version in the name (`desurface-windows-x64.msi`, `desurface-linux-x86_64.AppImage`, …)
  are the same packages under fixed names, for download links that always fetch the latest release.

For Flatpak feed installation and updates, add Flathub for the required runtime before
installing DeSurface; keep both remotes in the same user installation:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --user --if-not-exists desurface https://docs.solidcraft.io/flatpak/solidcraft.flatpakrepo
flatpak install --user desurface ai.storyteller.solidcraft
flatpak run ai.storyteller.solidcraft
```

A remote added from the old `bherbruck.github.io` address no longer finds updates: run
`flatpak remote-modify --user --url=https://docs.solidcraft.io/flatpak/repo/ desurface`, then
`flatpak update --user`.

Flatpak installs from 0.7 or earlier use the old ID `ai.storyteller.solvecraft`: `flatpak update --user`
offers to replace it with `ai.storyteller.solidcraft`, and settings move along.

Single-file `.flatpak` bundles include the runtime repository address; missing runtimes
still need an internet connection. [Linux installation details](https://docs.solidcraft.io/guide/getting-started.html#linux-flatpak).

The builds aren't signed yet. On macOS, right-click `DeSurface.app` in Applications and choose
**Open** the first time (or run `xattr -dr com.apple.quarantine /Applications/DeSurface.app`).
On Windows, SmartScreen may say "Windows protected your PC": **More info**, then **Run anyway**.
Licences are built in (Help ▸ About ▸ Licences, `desurface-cli licences`); the packages also
install `THIRD-PARTY-LICENSES.txt`.

