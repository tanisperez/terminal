# Releasing Tanis Terminal

Publishing a version means pushing a tag. The `Release` workflow
(`.github/workflows/release.yml`) builds tterm for every platform, attaches the packages to the
GitHub release of that tag and publishes it, with `latest.json`, the manifest of the in-app
updater. It is ted's workflow, plus Zig 0.17 on Linux and Windows to build libghostty-vt. Once
the release is published, `website.yml` rebuilds the site: when the `GPG_PRIVATE_KEY` and
`GPG_PASSPHRASE` secrets exist, the APT and DNF repositories get the new packages
(`packaging/linux/README.md`). To try an update between pre-releases, start a build made with
`TTERM_UPDATER=github` with `TTERM_UPDATE_URL` set to the address of another `latest.json`.

## Versions and tags

- The version lives in one place: `version` under `[workspace.package]` in `Cargo.toml`.
  Versions follow SemVer.
- Between two releases the version is the next one with the `-dev` suffix (`0.2.0-dev`). Right
  after a release, bump `version` to the next `X.Y.Z-dev`.
- A development build shows the short commit next to the version in Settings, About, in
  `tterm --version` (`tterm 0.2.0-dev (4e74e2d)`) and in the startup log line. It comes from
  `tterm-cli/build.rs` (`git rev-parse --short HEAD`, or `TTERM_COMMIT` when set at build time,
  nothing without a repository). Releases and release candidates show the bare version.
- The tag is `v` plus that version: `v0.1.0`. The workflow fails if they differ, so set the final
  version (drop `-dev`) before tagging.
- A tag with a pre-release suffix (`v0.1.0-rc.1`) is accepted when the part before the suffix
  matches the version (or when the version itself has the same suffix). It is published as a
  GitHub pre-release, never marked as the latest one, so the updater never offers it.

## Steps

The work of a version is tracked by its GitHub milestone (`gh issue list --milestone X.Y.Z`).

1. Check that every issue of the milestone is closed and `make format`, `make lint` and
   `make test` pass on `main`, with the `Build` workflow green.
2. Optionally, run the workflow by hand on `main` first (`gh workflow run release.yml`): a dry
   run builds every package and keeps them as the run's artifacts, without touching any release.
3. Finish `release-notes/X.Y.Z.md`, the release notes in English Markdown (what changes and which
   milestone issues went in, as `#N`). Set `version` in `Cargo.toml` to the release (drop
   `-dev`), run `cargo check` so `Cargo.lock` follows, and commit.
4. Tag and push (ask before pushing):

   ```
   git tag v0.1.0
   git push origin main v0.1.0
   ```

5. Create the release as a **draft**, with the notes file of step 3:

   ```
   gh release create v0.1.0 --draft --verify-tag --title v0.1.0 --notes-file release-notes/0.1.0.md
   ```

6. Wait for the `Release` workflow (`gh run watch`). It checks that every package is there,
   writes `SHA256SUMS` and `latest.json`, uploads everything to the draft and then publishes it.
   If the draft does not exist yet when the builds finish, the workflow creates one with
   placeholder notes; edit them afterwards with
   `gh release edit v0.1.0 --notes-file release-notes/0.1.0.md`. A stable release then starts
   the `Website` workflow.
7. Bump `version` to the next `X.Y.Z-dev` and commit.

## Packages

Targets: Linux x86_64 (built on Ubuntu 22.04, glibc 2.35), Windows x86_64, macOS arm64. The
scripts live in `packaging/`, and every one can run locally.

| Asset | Content |
| --- | --- |
| `tterm-X.Y.Z-linux-x86_64.tar.gz` | `bin/tterm`, the desktop entry, the icons and `install.sh` in a `/usr`-like layout |
| `tterm_X.Y.Z_amd64.deb`, `tterm-X.Y.Z-1.x86_64.rpm` | built with `nfpm` from `packaging/linux/nfpm.yaml` |
| `tterm-X.Y.Z-1-x86_64.pkg.tar.zst` | Arch package, written by `package.sh` with `bsdtar` and `zstd` |
| `tterm-X.Y.Z-x86_64.AppImage` | the same files in an AppImage; system libraries are not bundled |
| `tterm-X.Y.Z-macos-arm64.dmg`, `.app.tar.gz` | `tterm.app`, ad-hoc signed; the tarball is for the updater |
| `tterm-X.Y.Z-windows-x86_64-setup.exe` | Inno Setup, **per-user** install, no administrator rights |
| `tterm-X.Y.Z-windows-x86_64-portable.zip` | `tterm.exe`, `LICENSE` and `NOTICE`, nothing to install |
| `latest.json`, `SHA256SUMS` | the updater manifest and the checksums of every asset |

Locally, on Linux, with Zig 0.17, `nfpm`, `appimagetool` (and its type2 runtime), `bsdtar` and
`zstd` on the `PATH`:

```
make release
packaging/linux/package.sh 0.1.0 target/release/tterm dist
APPIMAGE_EXTRACT_AND_RUN=1 packaging/linux/appimage.sh 0.1.0 target/release/tterm dist
```

`packaging/macos/bundle.sh` needs a Mac (`iconutil`, `codesign`, `hdiutil`), and
`packaging/windows/tterm.iss` Inno Setup 6 (`ISCC.exe`); both are run by the workflow.

## The in-app updater

`state/update.rs` and `views/update.rs`, on top of `telaio-update`, as in ted and Postino:

- compiled in only with `TTERM_UPDATER=github` at build time; the release workflow sets it on
  every platform. A local build has no updater, and Settings shows no Updates section;
- one check 10 seconds after startup (the "Check for updates automatically" setting,
  `check_updates` in `settings.toml`), never a timer; "Check now" in Settings, About;
- macOS, Windows and a Linux copy installed by `install.sh` in a folder the user can write
  download the update in the background and verify its SHA-256; the bar then shows
  "X.Y.Z ready · Restart" (design R). The restart always asks first, since it closes every tab;
- a portable Windows copy, a Linux tarball or AppImage only announce the new version
  ("X.Y.Z available" opens the release page); a script install under `/usr/local` copies the
  install command instead; a deb, rpm or Arch copy updates with its package manager and shows
  nothing.

## Desktop integration

- **Icons:** one master, `packaging/icons/tterm.svg` (design G). The PNGs under
  `packaging/icons/png/` and `crates/tterm-app/assets/tterm.ico` are generated from it and
  committed (`packaging/icons/README.md`). The macOS `.icns` is built at bundle time.
- **Linux:** `codes.tanis.tterm.desktop` (the Wayland `app_id` and X11 `WM_CLASS` of the
  window), in the System and TerminalEmulator categories, with `Exec=tterm %F`: a folder dropped
  on the launcher opens there. The deb, rpm and Arch packages put `tterm` in `/usr/bin`.
- **Windows:** the installer adds a Start menu entry (with the AppUserModelID
  `codes.tanis.tterm`), an optional desktop icon, and two tasks checked by default: `tterm` on
  the user `PATH` (removed on uninstall) and "Open in Tanis Terminal" in the Explorer menu of a
  folder and of a folder's background. `tterm.exe` carries the icon as resource 1
  (`crates/tterm-app/build.rs`, `tterm.rc`), which gpui also uses for the window and the taskbar.
- **macOS:** `tterm.app`, shown as Tanis Terminal (`CFBundleDisplayName`). It declares no
  document types. A `.app` cannot put itself on the `PATH`.
- **Unsigned for now:** Gatekeeper and SmartScreen warn on the first launch. Signing and
  notarization are later steps, as in Postino.
