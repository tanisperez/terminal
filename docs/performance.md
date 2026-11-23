# Performance

Measured on 2026-11-23 against kitty 0.51.1, before the 0.1.0 release (#18), on a ThinkPad L14
(Intel Iris Xe, CachyOS, KDE Plasma). Both terminals used Noto Sans Mono 12 pt, 10 px of padding
and a window of about 1400 x 900. tterm was a release build; kitty ran with `--config NONE`.

## Method

- **Throughput:** a script inside the terminal times `cat` of three files and `seq 1 5000000`,
  three runs each, and the table shows the mean. The time is what the terminal takes to drain
  the pseudo terminal, which includes parsing and the frames it paints meanwhile. Files:
  100 MB of base64 lines (plain), 50 MB of lines full of 256-color SGR codes (color), 20 MB of
  CJK, Greek, box drawing, blocks and emoji (unicode).
- **Memory:** peak RSS of the terminal process during the throughput run, and RSS of an idle
  window with one shell. kitty kept its default 2000 lines of scrollback; tterm keeps 10 MB.
- **Startup:** from the launch to the window being mapped (X11, polled with `xdotool`), five
  runs.
- **Idle:** CPU time used in 30 s by a window with one shell, 20 s after it opened.
- X11 runs used a private rootful Xwayland; Wayland runs used KDE's compositor directly.

## Results

Throughput, seconds (lower is better):

| Test | tterm, Wayland | kitty, Wayland | tterm, X11 | kitty, X11 |
| --- | --- | --- | --- | --- |
| plain, 100 MB | 0.65 | 0.82 | 0.58 | 1.65 |
| color, 50 MB | 0.47 | 0.65 | 0.49 | 0.84 |
| unicode, 20 MB | 0.19 | 0.22 | 0.20 | 0.33 |
| `seq 1 5000000` | 1.21 | 1.46 | 1.20 | 1.52 |

| | tterm | kitty |
| --- | --- | --- |
| Peak RSS during the throughput run | 166 MB | 183 MB |
| RSS, idle window | 143 MB | 154 MB |
| Window mapped after launch (X11) | 69 to 85 ms | 126 to 150 ms |
| CPU while idle, 30 s, Wayland | 0.00 s | 0.00 s |
| CPU while idle, 30 s, X11 | 0.19 s | 0.00 s |

## Reading the numbers

- tterm drains output faster than kitty in every test, by 1.2 to 1.4 times on Wayland and up
  to 2.8 times on X11. libghostty-vt parses fast, and output is joined in batches of up to 1 MB
  before each update, so a burst costs few frames.
- Memory is close. Most of tterm's idle RSS is gpui, the bundled fonts and the GPU context.
- On Wayland neither terminal does any work while idle. On X11, gpui redraws on a 16 ms timer
  (`gpui_linux::x11::client: Refreshing every 16ms` in the log), so an idle tterm still uses a
  little CPU there. That comes from gpui, not from tterm.
- Not measured: input latency (it needs a camera or a tool like Typometer) and frame times
  while scrolling.
