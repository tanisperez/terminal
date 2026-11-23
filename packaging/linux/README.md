# Linux packaging

| File | Purpose |
|---|---|
| `codes.tanis.tterm.desktop` | Desktop entry (`Exec=tterm %F`: folders dropped on the launcher open there) |
| `package.sh` | Builds the tarball, with `site/install.sh` in it |
| `test-install.sh` | Tests the install script (`site/install.sh`) against fake releases, no network |

Linux gets the tarball and the install script only (decided on 2026-11-23): no deb, rpm, Arch
package, AppImage or AUR. The install script puts `tterm` in `~/.local/bin` and the in-app updater
keeps it up to date.
