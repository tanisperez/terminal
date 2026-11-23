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
| `TERMINAL_LINE_HEIGHT` | 1.6 | Row height as a multiple of the font size |
| `CURSOR_THICKNESS` | 2 | Bar and underline cursors |

## Screens

- **A1, tabs**: the bar shows the tabs (prompt icon, label, X), the + button with its tooltip
  ("New tab", Ctrl Shift T), and at the right the agent usage and settings buttons, then the
  window controls (Linux and Windows) or the traffic lights at the left (macOS).
- **A2, one tab**: no bar. A strip of the terminal's color keeps the buttons at the right.
- **F, Settings**: telaio's Settings dialog. General: theme (Dark, Light, System), font and size,
  cursor (Block, Bar, Underline), paste images, language. About: version, libghostty-vt
  version, privacy line.
- Still to build: D and E (dropping and pasting screenshots), H2 (agent usage), profiles (P1,
  W1, W2, M1, U's chevron), T (bundled fonts), G (the icon).

## Typography

- Interface: Geist 13 from telaio.
- Terminal: the `font` setting, Geist Mono 13 by default. The design's default is Cascadia
  Code, bundled with JetBrains Mono, Fira Code and Geist Mono (board T); pending.
