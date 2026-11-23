# Tanis Terminal design system

Reference for anyone (human or agent) touching the UI. It is written from the design in the
claude.ai Artifact https://claude.ai/artifact/Q6FaRuYGpdcoaJ2mkMT4nX, built on telaio's design
system, as the shared rules ask (1.3). Once the UI is implemented the code is the source of truth
and this file its readable summary: if they disagree, the code wins; fix this file.

The shared part (tokens, `Palette`, theme JSON, metrics, fonts and the standard components) lives
in [telaio](https://github.com/tanisperez/telaio). tterm uses telaio's default tokens unchanged
(its dark accent is the indigo `#5b63e6` of the design) and adds only the terminal colors and the
metrics of its own layout.

## Rules

- Colors only through `cx.palette()` and the `TerminalColors` extension trait
  (`theme/palette.rs`), never from `cx.theme()` and never as a literal in a view.
- Sizes only from telaio's `metrics` or tterm's `theme/metrics.rs`.
- Icons are Lucide, through `gpui_kit::assets::IconName`.
- Every clickable control has a hover and a pressed state (shared rules, section 19).

## Terminal colors

The ground is pure black, not telaio's `bg`: the terminal is the whole window.

| Use | Dark | Light |
| --- | --- | --- |
| Terminal background, active tab | `#0b0b0f` | `#ffffff` |
| Terminal text | `#e9eaee` | `#1f2029` |
| Cursor | `#5b63e6` | `#4f57d8` |
| Tab bar | `#111115` | `#f3f4f6` |
| Line under the bar, around the active tab | `#23252c` | `#e3e5ea` |
| Line between inactive tabs | `#1c1d23` | `#eaecf0` |

The 16 ANSI colors (`DARK_SCHEME`, `LIGHT_SCHEME` in `theme/palette.rs`) take the design's hues:
red `#f27171`, green `#4fc88f`, yellow `#e8b75a`, blue `#7a80eb` and bright blue `#a3a9ff`,
bright black `#8b8e9a`. Programs may change them for a while (OSC 4, 10, 11, 12).

## Metrics

| Constant | Value | Use |
| --- | --- | --- |
| `TAB_BAR_HEIGHT` | 37 | The tab bar, also the title bar |
| `TAB_MAX_WIDTH`, `TAB_MIN_WIDTH` | 168, 72 | A tab, shrinking when they do not fit |
| `TAB_ACCENT_HEIGHT` | 2 | The accent line on top of the active tab |
| `TAB_CLOSE_SIZE` | 20 | The X of a tab, shown on the active tab and on hover |
| `BAR_BUTTON_SIZE` | 28 | New tab, agent usage and settings buttons |
| `TERMINAL_PADDING_X`, `_Y` | 18, 14 | Space around the grid |
| `PIXELS_PER_POINT` | 96 / 72 | The terminal font size is in points, as kitty's; rows are the font's own height (ascent plus descent, rounded up) |
| `CURSOR_THICKNESS` | 2 | Bar and underline cursors |

## Screens

- **A1, tabs**: the bar shows the tabs (prompt icon, label, X), the + button with its tooltip
  ("New tab", Ctrl Shift T), and at the right the agent usage and settings buttons, then the
  window controls (Linux and Windows) or the traffic lights at the left (macOS).
- **A2, one tab**: no bar. A strip of the terminal's color keeps the buttons at the right.
- **F, Settings**: telaio's Settings dialog. General: theme (Dark, Light, System), font and size,
  cursor (Block, Bar, Underline), paste images, language. About: version, libghostty-vt
  version, privacy line.
- **M1, W1, U, profiles menu**: the chevron next to + (only with several profiles) opens every
  profile with its chip (`ProfileColor`: indigo, green, blue, yellow, gray from the palette's
  accent, success, info, warning and fg_subtle), its tag (WSL or the start program), "default"
  and its shortcut keys, then "Configure profiles…".
- **P1, Settings > Profiles**: detected profiles (eye to hide, default badge) and the user's,
  edited in a form (name, shell, start folder, start command, color, shortcut, default).
- **E, pasted image**: the success notification "Clipboard image pasted".
- **H2, agent usage**: in place of the terminals, with its own bar (back to the terminal,
  "Agents" and the period, the 7/30/90 days/All control, Esc). "Now" lists the tabs running
  Claude Code (Working while its log was written in the last 15 s); the totals compare with the
  period before (success up, danger down); the day chart draws input up in `border_hover` and
  output down in `accent`; the models share one bar (accent, info, success, warning,
  fg_subtle).
- **W2, profile picker**: Ctrl+Shift+Space (Cmd+Shift+Space on macOS) opens "New tab with…"
  over a scrim: every profile with its chip, the WSL tag, "default" and its number; arrows,
  Enter, a number, Esc, and the hints bar of telaio's modals.
- Tab labels: a user's profile is named by the profile; others by the program's title (a
  title that is a program path is ignored), or the shell and its folder.
- **G, icon**: `packaging/icons/tterm.svg`, the black square with the indigo prompt and the
  white underscore, filling the canvas with Postino's corner radius; PNGs and the Windows
  `.ico` generated from it (`packaging/icons/README.md`).
- **D, drop overlay**: while files are dragged over a terminal, the `scrim` with a dashed
  `accent` frame on `accent_subtle` at 55%, the image's thumbnail (224 x 140), its name, size
  and a PNG's dimensions, "Drop to paste the image" (or the path) and "The path is typed on the
  command line" in `accent_text`. Dropping types the paths as the tab's shell reads them: single
  quotes for POSIX shells, `/mnt/<drive>/...` in WSL, double quotes in cmd and PowerShell.

## Typography

- Interface: Geist 13 from telaio.
- Terminal: the `font` setting, Noto Sans Mono 12 pt by default, as board T (what kitty shows
  on the author's machine). Noto Sans Mono, Cascadia Code, JetBrains Mono and Fira Code are
  bundled (`theme/fonts.rs`, OFL 1.1), Geist Mono comes from
  telaio; the select lists them first, then the known monospace fonts installed. Ligatures are
  on.
