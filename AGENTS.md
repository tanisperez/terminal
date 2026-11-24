# AGENTS.md

Guidance for AI agents working on Tanis Terminal (`tterm`).

The rules shared by every Rust desktop app (design first, stack, telaio, performance,
translations, logging, CLI, docs, releases, packaging, CI, testing) are imported below. This file
only adds what is specific to tterm, and wins where the two differ.

@~/.claude/rust/RULES.md

## Project

Tanis Terminal is a native terminal emulator for Linux, Windows and macOS, written in Rust with
`gpui` (through `gpui-kit`) and telaio. Postino (`~/dev/postino`) is the reference
implementation and ted (`~/dev/ted`) the closest sibling: copy their skeleton, conventions and
patterns, and rename.

The terminal emulation is **libghostty-vt**, Ghostty's VT parser and screen state, through the
`ghostty-vt` crate (a safe Rust API over the official release, linked statically). tterm owns
everything around it: the pseudo terminal, the rendering, the input, the tabs and the settings.

It is a desktop project, unrelated to the Expo mobile apps in `~/dev`. Do not reuse the
Expo/React Native conventions here.

## Decisions taken

- Name: **Tanis Terminal**, binary `tterm`, crates `tterm-*`, app id `codes.tanis.tterm`, env
  prefix `TTERM_`, website `terminal.tanis.codes`, repository `tanisperez/terminal` (decided on
  2026-11-23).
- License: Apache-2.0, as Postino. Ghostty (MIT) is credited in `NOTICE`.
- Stack: Rust (edition 2024, stable toolchain) + `gpui-kit` 0.10.0 + telaio (`telaio-ui`,
  `telaio-app`) pinned to one tag in `[workspace.dependencies]` (`v0.32.0` today).
  `cargo tree -d --depth 0 | grep gpui` must stay empty.
- Emulator: `ghostty-vt` 0.9 (libghostty-vt 1.0.1). On Linux and Windows MSVC its build script
  compiles libghostty-vt from source with **Zig 0.17** (`pacman -S zig`); macOS and Windows GNU
  download a prebuilt static library. The handles are `!Send`: the emulator lives on the window's
  thread.
- Pseudo terminal: `portable-pty` (WezTerm's, MIT): Unix pty and Windows ConPTY.
- Shell integration (#10): bash, zsh and fish load Ghostty's scripts (shipped by `ghostty-vt`,
  written to a cache folder), injected as Ghostty does: zsh through `ZDOTDIR`, bash with
  `--posix` and `ENV`, fish through `XDG_DATA_DIRS` (`tterm-term/src/integration.rs`). They
  report the folder (OSC 7, `kitty-shell-cwd://`) and the prompts (OSC 133).
  `GHOSTTY_SHELL_FEATURES` is empty: tterm names its tabs and draws its cursor itself. A new
  tab starts in the active tab's folder, from OSC 7 or, on Linux, the foreground program's
  folder in `/proc`. Not on Windows, where WSL runs its own shells.
- Programs see `TERM=xterm-ghostty` with Ghostty's terminfo (written to a cache folder by
  `ghostty_vt::terminfo::dir`), `COLORTERM=truecolor` and `TERM_PROGRAM=tterm`.
- Languages of the UI: **English** (source and fallback) and **Spanish**, as ted.
- Design: the prototype lives in a claude.ai Artifact,
  https://claude.ai/artifact/Q6FaRuYGpdcoaJ2mkMT4nX (canvas "Tanis Terminal", built on telaio's
  design system). UI changes are designed there first; read its boards with the Artifact tool,
  never a local copy. Boards: A1 tabs, A4 one tab (the bar always shows), C resizing, D dropping
  a screenshot, E a screenshot pasted into Claude Code, F settings, H2 agent usage, W1/W2
  Windows profiles, M1 macOS profiles, P1 profile settings, U the bar on the three platforms,
  T fonts, G the icon, R the update ready in the bar and its restart dialog; for 0.2.0, F2
  general settings (start folder, opacity), F3 colors, P2 profile order, S1 split panes, X1
  moving a tab to a new window, A5 the tab tooltip, T2 Nerd Font icons and the font select.
- The tab bar is the title bar and always shows, with one tab too (A4, U, decided on
  2026-11-23). A single tab has no close button: closing it is closing the window. The grid is
  centered across and starts at the top, under the bar (A4, C).
- Settings apply at once and are saved, as every app on telaio (shared rules, section 10), not
  with the Cancel and Save buttons of board F.
- The default theme is Dark, the pure black of the design. Its terminal colors are the author's
  kitty palette (#30, `state/colors.rs`), changed in Settings, Colors and saved as `[colors]`.
- Pasted clipboard images (#3, decided on 2026-11-23) are saved as PNG in the OS cache folder
  (`~/.cache/tterm/images/`, `~/Library/Caches/tterm/images/`,
  `%LOCALAPPDATA%\tterm\cache\images\`), never in Downloads or `/tmp`, so a resumed agent
  session still finds them. Images older than `pasted_images_days` (14 by default) are deleted at
  startup.
- Agent usage (#6, decided on 2026-11-23) comes from Claude Code only, for now: the `usage` of
  each assistant message in `~/.claude/projects/*/*.jsonl`, read in the background when the
  screen opens. Only the usage lines are read, never the content. Input tokens include the cache
  (creation and reads), with the breakdown in the tooltip. Open sessions are the tabs running
  `claude`.
- Packaging and updates (#11, decided on 2026-11-23): copied from ted (`docs/releasing.md`).
  Linux gets the tarball and the self-updating install script only: no deb, rpm, Arch package,
  AppImage or AUR (decided on 2026-11-23). The macOS bundle is `tterm.app`, shown as
  Tanis Terminal, and `telaio-update`'s `app_name` is `tterm`, so its folders match the
  binary's (`tterm/updates` in the cache). The Windows installer is per user in
  `%LOCALAPPDATA%\Programs\tterm`, with "Open in Tanis Terminal" on folders. No document types
  or MIME types: a terminal opens folders, not files. The update button sits in the bar before
  the usage button (design R), and restarting always asks first, since it closes every tab.
- Copy and paste (decided on 2026-11-23), as the author's kitty: Ctrl+C copies the selection
  and clears it, or else goes to the program; Ctrl+V pastes. Ctrl+Shift+C and Ctrl+Shift+V stay.
  Ctrl+click (Cmd+click) opens the URL under the mouse (`state/links.rs`).
- Profiles (#5, decided on 2026-11-23) live in `settings.toml`: detected profiles are not
  written; the user's own ones are `[[profiles]]` tables (`name`, `shell`, `cwd`, `startup`,
  `color`, `shortcut`), plus `default_profile` and `hidden_profiles`. `startup` is typed into the
  shell instead of replacing it, so the tab stays open when the program exits.
- Milestone 0.2.0 (decided on 2026-11-24): split panes, horizontal and vertical (#33); moving a
  tab to its own window (#34); a configurable color palette whose default is the author's kitty
  one (#30); background transparency, off by default (#31); a start folder for the first tab,
  with new tabs inheriting the active tab's folder unless Settings says otherwise (#28); Nerd
  Font icons in the bundled fonts, CaskaydiaCove Nerd Font Mono by default (#35). Each one is
  designed in the Artifact first.

## Pending decisions (ask before choosing)

- Agents other than Claude Code in the usage screen.
- Environment variables per profile.

## Architecture

A Cargo workspace of small crates, UI kept thin. Dependency direction, no cycles, no crate
depends on `tterm-app`:

```
tterm-app ──> tterm-term ──> ghostty-vt, portable-pty
   ├────────> tterm-usage (agent logs, standalone)
   ├────────> telaio-ui, telaio-app, telaio-update
   └────────> tterm-cli (standalone, depends on no other tterm crate)
```

- `tterm-term`: the terminal without a window. `pty` starts the shell in a pseudo terminal;
  `Emulator` wraps libghostty-vt's terminal, turns the shell's output into a plain `Frame`
  (lines of styled runs, background spans, the cursor, the scroll position) and encodes keys,
  pastes and focus reports. No gpui, tested with `cargo test`.
- `tterm-usage`: the token usage of the agents, read from Claude Code's logs
  (`~/.claude/projects/**/*.jsonl`, one entry per message id, prompts as tasks) and summed per
  period and local day for the usage screen. Only usage numbers, never message contents. No gpui.
  `cargo run --release -p tterm-usage --example scan` prints the totals of this machine.
- `tterm-cli`: `-h/--help`, `-V/--version` and the folder arguments, `IDENTITY` (the
  `telaio_app::AppIdentity` of tterm), `build.rs` with the commit, as `ted-cli`.
- `tterm-app`: the gpui binary (`tterm`). `views/terminal.rs` paints a `Frame` and forwards
  input; `views/root.rs` holds the tabs and the bar; `views/settings.rs` the Settings dialog.
  UI state that can be tested without a window lives in `state/`.

How keys reach the shell: gpui's key bindings first (the app's shortcuts), then
`TerminalView::on_key_down` for keys without text (Enter, arrows, Ctrl and Alt combinations),
encoded by libghostty-vt. Text goes through the view's input handler (`EntityInputHandler`,
`replace_text_in_range`), as the platform composes it: dead keys, AltGr characters, input
methods (whose preedit is drawn at the cursor). On Windows a key handled in `on_key_down` skips
`TranslateMessage`, so handling a text key there would leave a pending dead key to spoil the
next characters (seen in the Windows 11 VM: `´` then `a` gave `ã`). Only a program that asks the
Kitty protocol to report every key gets text keys encoded.

How output reaches the screen: a thread per tab reads the pseudo terminal and sends chunks
through a bounded channel to a task on the window's thread, which joins what is waiting (up to
1 MB), feeds the emulator once, writes the answers back to the shell and repaints. A program that
writes faster than the window paints is slowed down by the pseudo terminal, not queued.

## Performance

The shared rules (section 8) apply in full. On top of that, for tterm:

- An idle terminal does no work: no timer, no polling. The cursor blinks (`cursor_blink`,
  on by default, decided on 2026-11-23) only for 15 s after a key or focus, as kitty, then
  stays shown. The reader thread sleeps in `read`, the window's task sleeps on the channel.
- `Emulator::frame` only rebuilds the rows libghostty-vt marks dirty; lines are `Arc`s, so a
  frame with one changed row costs one new line.
- Text is shaped per run with gpui's line cache and a forced cell width, so a frame that did not
  change costs no shaping.
- Block elements (U+2580 to U+259F) and box drawing lines (U+2500 to U+257F, but the dashed,
  double and diagonal ones) are painted as quads snapped to device pixels, not with the font
  (`state/box_drawing.rs`), as kitty and Ghostty do: logos and frames meet across cells.

## Testing and debug hooks

- Unit tests in every crate. The emulator is tested by feeding escape sequences and reading the
  frame; the pseudo terminal by running `/bin/sh -c ...`.
- Visual checks in the real app as in the shared rules (section 17): XWayland, `xdotool`,
  `spectacle`, isolated `XDG_CONFIG_HOME` and `XDG_STATE_HOME`, killing only our own PID.
- Windows: cross-build with cargo-xwin and `GHOSTTY_VT_PREBUILT_MSVC=1` (the prebuilt
  libghostty-vt links with lld-link), and test in the Windows 11 VM (see the user's memory).
  Cross-build in debug: a release build needs Windows' shader compiler (`fxc`, gpui's
  build script). Windows debug builds embed the icons (rust-embed's `debug-embed`, in
  `tterm-app`'s Cargo.toml); without it they are read from the build machine's disk and every
  icon is empty in the VM.
- Debug hooks, read once at startup: `TTERM_LOG=<level>`, `TTERM_OPEN=<state>` (`settings`,
  `settings-profiles`, `settings-about`, `usage`), `TTERM_UPDATE_URL=<latest.json>` (in a build
  made with `TTERM_UPDATER=github`, checks this manifest instead of GitHub's).

## Working rules

- Git remote `origin` is `tanisperez/terminal`. Commits are fine; never `git push` unless the
  user says so explicitly. Commit messages, issues and pull requests in English.
- Work is tracked in the GitHub milestone of each version (`gh issue list --milestone 0.1.0`).
  Complex issues go on a branch.
- Keep changes small and traceable to the request. Do not refactor unrelated code.
- Run `make format`, `make lint` and `make test` before considering a task done.
- GPUI evolves quickly. Read the pinned `gpui-kit` source in `~/.cargo/registry/src/` instead of
  relying on memory of its API; the same for `ghostty-vt`.
- Writing style: never use the em dash in code, comments or docs. Use a comma or a period.

## Commands

See `make help`. Main targets: `run`, `build`, `release`, `check`, `lint`, `format`, `test`.
