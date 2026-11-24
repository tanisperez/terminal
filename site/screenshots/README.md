# Website screenshots

`capture.sh` regenerates the screenshots of the website into `site/img/` as WebP, one light and one
dark version of each: `hero`, `profiles`, `usage`, `panes` and `colors`. Run it with
`make screenshots`, or give step names to run only those (`capture.sh panes colors`), and
`THEMES=dark` to take one theme. It is Postino's `site/screenshots/capture.sh`, adapted.

## What it does

- Makes a demo home in a temporary folder: three projects under `~/dev`, a `.bashrc` with a
  short prompt (`demo/bashrc`), canned output for each project (`demo/show.sh`), a fake `claude`
  (a copy of `sleep`, so the usage screen finds an agent running) and 60 days of made up Claude
  Code logs (`demo/usage.py`, usage numbers only). The app runs with `HOME` set there, so no
  path, user or machine name of yours shows.
- Launches `target/debug/tterm` (built with `cargo build` if missing; `TTERM_BIN` picks another
  binary) under XWayland at 1.5x, 1200 x 750, with an isolated config, state and cache, so your
  real settings are never touched. The UI is in English, the font CaskaydiaCove Nerd Font Mono.
- Opens what it can with the `TTERM_OPEN` debug hook (Settings, Colors), and uses `xdotool` for
  the rest: a click on the profiles chevron and on the usage button, and the pane shortcuts.
  Every key and click first checks that the tterm window has the focus, and aborts otherwise.
- Captures the window with `spectacle` and converts it with ImageMagick.

## Requirements

- Linux, KDE Plasma on Wayland with XWayland. Click coordinates are screenshot pixels, shadow
  included; XWayland runs at the largest display scale, read from `kscreen-doctor -o`.
- `spectacle`, `xdotool`, `python3` and ImageMagick (`magick`) built with WebP support.
- An unlocked screen. Do not use the keyboard or mouse while it runs (about 2 minutes).
