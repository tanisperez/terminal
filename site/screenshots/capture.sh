#!/bin/bash
# Regenerates the website screenshots into site/img/ (WebP, light and dark), as Postino's
# site/screenshots/capture.sh does: the app opens on the desktop under XWayland at 1.5x and
# spectacle captures its window.
# Usage: capture.sh [STEP...], every step when none is given. Steps: hero, profiles, usage,
# panes, colors. THEMES="dark" limits the themes. See README.md here for the requirements.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
# TTERM_BIN picks the binary, target/debug/tterm by default.
BIN="${TTERM_BIN:-$ROOT/target/debug/tterm}"
OUT="$ROOT/site/img"
SCALE=1.5
WORK="$(mktemp -d)"
# The app runs with HOME pointing here, so the paths it shows read "~/dev/terminal".
HOME_DIR="$WORK/home"

cleanup() {
    [ -n "${APP_PID:-}" ] && kill "$APP_PID" 2>/dev/null
    rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

die() { echo "capture.sh: $*" >&2; exit 1; }

for tool in spectacle xdotool magick python3; do
    command -v "$tool" >/dev/null || die "missing required tool: $tool"
done
if [ "$(busctl --user call org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver GetActive 2>/dev/null)" = "b true" ]; then
    die "the screen is locked, unlock it first"
fi
if [ ! -x "$BIN" ]; then
    (cd "$ROOT" && cargo build) || die "cargo build failed"
fi
mkdir -p "$OUT"

# The demo home: three projects, a short prompt, a fake `claude` (a copy of sleep, so the usage
# screen finds it running in a tab) and 30 days of made up agent logs.
make_home() {
    rm -rf "$HOME_DIR"
    mkdir -p "$HOME_DIR/dev/terminal" "$HOME_DIR/dev/postino" "$HOME_DIR/dev/telaio" "$HOME_DIR/bin"
    cp "$HERE/demo/bashrc" "$HOME_DIR/.bashrc"
    # The detected bash is a login shell, which reads .bash_profile.
    echo '. ~/.bashrc' > "$HOME_DIR/.bash_profile"
    cp "$HERE/demo/show.sh" "$HOME_DIR/bin/show"
    cp "$(command -v sleep)" "$HOME_DIR/bin/claude"
    python3 "$HERE/demo/usage.py" "$HOME_DIR"
}

# settings THEME: the isolated settings.toml. Tabs run the detected bash, whose .bashrc prints the
# demo output, so they are named after their folder; two profiles fill the profiles menu.
settings() {
    local theme="$1"
    mkdir -p "$HOME_DIR/.config/tterm" "$HOME_DIR/.local/state"
    cat > "$HOME_DIR/.config/tterm/settings.toml" <<EOF
theme = "$theme"
language = "en"
font = "CaskaydiaCove Nerd Font Mono"
font_size = 12.0
check_updates = false
default_profile = "bash"

[[profiles]]
name = "claude · telaio"
cwd = "~/dev/telaio"
startup = "claude 3600"
color = "indigo"
shortcut = 3

[[profiles]]
name = "ssh · server"
startup = "show"
color = "green"
EOF
    printf 'width = 1200.0\nheight = 750.0\nmaximized = false\n' > "$HOME_DIR/.config/tterm/window.toml"
}

WID=""
APP_PID=""

# launch [VAR=value ...] -- [FOLDER ...]: starts tterm with the isolated home and waits for it.
# Coordinates of clicks are screenshot pixels, the shadow included (the bar is at y 65).
launch() {
    local vars=()
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do vars+=("$1"); shift; done
    [ "${1:-}" = "--" ] && shift
    (cd "$HOME_DIR/dev/terminal" && env WAYLAND_DISPLAY= HOME="$HOME_DIR" \
        PATH="$HOME_DIR/bin:$PATH" SHELL=/bin/bash \
        XDG_CONFIG_HOME="$HOME_DIR/.config" XDG_STATE_HOME="$HOME_DIR/.local/state" \
        XDG_CACHE_HOME="$HOME_DIR/.cache" GPUI_X11_SCALE_FACTOR=$SCALE "${vars[@]}" \
        "$BIN" "$@" >"$WORK/app.log" 2>&1) &
    local i
    for i in $(seq 1 40); do
        APP_PID=$(pgrep -nx tterm)
        [ -n "$APP_PID" ] && WID=$(xdotool search --pid "$APP_PID" 2>/dev/null | tail -1)
        [ -n "$WID" ] && break
        sleep 0.5
    done
    [ -n "$WID" ] || die "the tterm window never appeared"
    sleep 6
}

quit_app() {
    [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null
    local i
    for i in $(seq 1 20); do kill -0 "$APP_PID" 2>/dev/null || break; sleep 0.25; done
    WID=""; APP_PID=""
}

# Keystrokes and clicks must never reach another application.
ensure_active() {
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] && return 0
    xdotool windowactivate --sync "$WID"; sleep 0.4
    [ "$(xdotool getactivewindow 2>/dev/null)" = "$WID" ] || die "tterm lost the focus, aborting"
}

# XWayland runs at the largest display scale (see Postino's capture.sh): a screenshot pixel
# (PX, PY) is requested at (window origin + P) / factor.
PFACTOR=$(kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | awk '/Scale:/ {print $2}' \
    | sort -g | tail -1)
PFACTOR=${PFACTOR:-1}

pointer() {
    eval "$(xdotool getwindowgeometry --shell "$WID")"
    local rx ry
    rx=$(python3 -c "print(round(($X+$1)/$PFACTOR))")
    ry=$(python3 -c "print(round(($Y+$2)/$PFACTOR))")
    ensure_active
    xdotool mousemove $((rx-2)) $((ry-2)); sleep 0.2
    xdotool mousemove "$rx" "$ry"; sleep 0.3
    ensure_active
}

click() { pointer "$1" "$2"; xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.8; }

key() { ensure_active; xdotool key "$@"; sleep 1.2; }

# Parks the pointer in the terminal, away from the bar, so no hover state is captured.
park() { pointer 1700 1000; }

# shot NAME THEME [keep]: captures the window into site/img/NAME-THEME.webp.
shot() {
    local name="$1" theme="$2"
    [ "${3:-}" = keep ] || park
    sleep 0.8
    ensure_active
    spectacle -b -n -a -o "$WORK/$name.png" || die "spectacle failed"
    magick "$WORK/$name.png" -quality 85 -define webp:method=6 -define webp:alpha-quality=90 \
        "$OUT/$name-$theme.webp" || die "magick failed (no WebP support?)"
    echo "wrote $OUT/$name-$theme.webp"
}

step_hero() {
    settings "$1"
    launch -- "$HOME_DIR/dev/terminal" "$HOME_DIR/dev/postino" "$HOME_DIR/dev/telaio"
    shot hero "$1"
    quit_app
}

step_profiles() {
    settings "$1"
    launch -- "$HOME_DIR/dev/terminal" "$HOME_DIR/dev/postino"
    # The chevron after the + button, with two tabs, in screenshot pixels.
    click 618 65
    shot profiles "$1" keep
    quit_app
}

step_usage() {
    settings "$1"
    launch TTERM_DEMO_CLAUDE=1 -- "$HOME_DIR/dev/terminal"
    # The usage button: the screen lists the tabs running the (fake) agent when it opens.
    click 1600 65
    sleep 2
    shot usage "$1"
    quit_app
}

step_panes() {
    settings "$1"
    launch -- "$HOME_DIR/dev/terminal"
    key alt+shift+plus
    key alt+shift+minus
    shot panes "$1"
    quit_app
}

step_colors() {
    settings "$1"
    launch TTERM_OPEN=settings-colors -- "$HOME_DIR/dev/terminal"
    shot colors "$1"
    quit_app
}

STEPS=("$@")
[ ${#STEPS[@]} -eq 0 ] && STEPS=(hero profiles usage panes colors)
make_home
for theme in ${THEMES:-dark light}; do
    for step in "${STEPS[@]}"; do
        "step_$step" "$theme"
    done
done
