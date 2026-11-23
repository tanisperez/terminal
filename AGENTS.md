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
  prefix `TTERM_`, website `tterm.tanis.codes`, repository `tanisperez/terminal` (decided on
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
- Programs see `TERM=xterm-ghostty` with Ghostty's terminfo (written to a cache folder by
  `ghostty_vt::terminfo::dir`), `COLORTERM=truecolor` and `TERM_PROGRAM=tterm`.
- Languages of the UI: **English** (source and fallback) and **Spanish**, as ted.
- Design: the prototype lives in a claude.ai Artifact,
  https://claude.ai/artifact/Q6FaRuYGpdcoaJ2mkMT4nX (canvas "Tanis Terminal", built on telaio's
  design system). UI changes are designed there first; read its boards with the Artifact tool,
  never a local copy. Boards: A1 tabs, A2 one tab without a bar, D dropping a screenshot, E a
  screenshot pasted into Claude Code, F settings, H2 agent usage, W1/W2 Windows profiles, M1 macOS
  profiles, P1 profile settings, U the bar on the three platforms, T fonts, G the icon.
- The tab bar is the title bar and only shows from the second tab (A2, U). With one tab, a strip
  of the terminal's color keeps the usage and settings buttons, the window controls and the drag
  area.
- Settings apply at once and are saved, as every app on telaio (shared rules, section 10), not
  with the Cancel and Save buttons of board F.
- The default theme is Dark, the pure black of the design.
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
- Profiles (#5, decided on 2026-11-23) live in `settings.toml`: detected profiles are not
  written; the user's own ones are `[[profiles]]` tables (`name`, `shell`, `cwd`, `startup`,
  `color`, `shortcut`), plus `default_profile` and `hidden_profiles`. `startup` is typed into the
  shell instead of replacing it, so the tab stays open when the program exits.

## Pending decisions (ask before choosing)

- Fonts: board T bundles JetBrains Mono, Cascadia Code (the default), Fira Code and Geist Mono.
  Only Geist Mono (from telaio) is bundled today; the others are offered when installed.
- Agents other than Claude Code in the usage screen.
- Environment variables per profile.
- Splits: not in the design; ask before designing them.

## Architecture

A Cargo workspace of small crates, UI kept thin. Dependency direction, no cycles, no crate
depends on `tterm-app`:

```
tterm-app ──> tterm-term ──> ghostty-vt, portable-pty
   ├────────> telaio-ui, telaio-app
   └────────> tterm-cli (standalone, depends on no other tterm crate)
```

- `tterm-term`: the terminal without a window. `pty` starts the shell in a pseudo terminal;
  `Emulator` wraps libghostty-vt's terminal, turns the shell's output into a plain `Frame`
  (lines of styled runs, background spans, the cursor, the scroll position) and encodes keys,
  pastes and focus reports. No gpui, tested with `cargo test`.
- `tterm-cli`: `-h/--help`, `-V/--version` and the folder arguments, `IDENTITY` (the
  `telaio_app::AppIdentity` of tterm), `build.rs` with the commit, as `ted-cli`.
- `tterm-app`: the gpui binary (`tterm`). `views/terminal.rs` paints a `Frame` and forwards
  input; `views/root.rs` holds the tabs and the bar; `views/settings.rs` the Settings dialog.
  UI state that can be tested without a window lives in `state/`.

How output reaches the screen: a thread per tab reads the pseudo terminal and sends chunks
through a bounded channel to a task on the window's thread, which joins what is waiting (up to
1 MB), feeds the emulator once, writes the answers back to the shell and repaints. A program that
writes faster than the window paints is slowed down by the pseudo terminal, not queued.

## Performance

The shared rules (section 8) apply in full. On top of that, for tterm:

- An idle terminal does no work: no cursor blink, no timer, no polling. The reader thread sleeps
  in `read`, the window's task sleeps on the channel.
- `Emulator::frame` only rebuilds the rows libghostty-vt marks dirty; lines are `Arc`s, so a
  frame with one changed row costs one new line.
- Text is shaped per run with gpui's line cache and a forced cell width, so a frame that did not
  change costs no shaping.

## Testing and debug hooks

- Unit tests in every crate. The emulator is tested by feeding escape sequences and reading the
  frame; the pseudo terminal by running `/bin/sh -c ...`.
- Visual checks in the real app as in the shared rules (section 17): XWayland, `xdotool`,
  `spectacle`, isolated `XDG_CONFIG_HOME` and `XDG_STATE_HOME`, killing only our own PID.
- Debug hooks, read once at startup: `TTERM_LOG=<level>`.

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
