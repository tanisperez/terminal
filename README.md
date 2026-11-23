# Tanis Terminal

[![License](https://img.shields.io/github/license/tanisperez/terminal)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20Windows%20%7C%20macOS-blue)](https://github.com/tanisperez/terminal)

A fast, native terminal for Linux, Windows and macOS, built on
[libghostty](https://ghostty.org), the terminal emulation library of Ghostty. Pure black, tabs
that only show up when there is more than one, profiles for every shell (WSL, PowerShell, ssh,
an agent started in a folder), screenshots pasted or dropped as a path, and the token usage of
the coding agents at a glance. No accounts, no telemetry.

The binary is `tterm`.

## Status

Early development: the first milestone, 0.1.0, is tracked in the
[GitHub milestone](https://github.com/tanisperez/terminal/milestones).

## Building from source

Needs Rust (stable, pinned by `rust-toolchain.toml`) and, on Linux and Windows MSVC,
[Zig 0.17](https://ziglang.org), which builds libghostty-vt from source the first time
(`pacman -S zig` on Arch). On Linux, also the development packages of Wayland, X11,
xkbcommon, fontconfig and Vulkan.

```sh
make run        # debug build, then start it
make release    # optimized binary in target/release/tterm
make help       # every target
```

## Built with

- [Rust](https://www.rust-lang.org)
- [libghostty-vt](https://ghostty.org), through the
  [ghostty-vt](https://crates.io/crates/ghostty-vt) crate
- [gpui-kit](https://github.com/longbridge/gpui-kit)
- [telaio](https://github.com/tanisperez/telaio), the design system shared with Postino and ted

## License

Apache-2.0, see [LICENSE](LICENSE) and [NOTICE](NOTICE).
