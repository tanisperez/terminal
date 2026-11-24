#!/bin/sh
# What each demo tab shows, by its folder: a Git log and a test run, as canned output.
clear
case "$PWD" in
*postino)
  printf '\033[33m4e74e2d\033[0m (\033[36mHEAD -> \033[32mmain\033[0m) Load test report as PDF\n'
  printf '\033[33m9b1c0aa\033[0m Variables in folder scripts\n'
  printf '\033[33m31f8e52\033[0m Settings: invalid TLS certificates\n'
  ;;
*telaio)
  printf '   \033[1;32mCompiling\033[0m telaio-ui v0.32.0\n'
  printf '    \033[1;32mFinished\033[0m release profile [optimized] target(s) in 38.21s\n'
  ;;
*)
  printf '\033[33m1656f9c\033[0m (\033[36mHEAD -> \033[32mmain\033[0m) Split panes, horizontal and vertical\n'
  printf '\033[33m350fe05\033[0m Move a tab to a new window, still running\n'
  printf '\033[33m2971b2d\033[0m Configurable terminal colors, kitty palette by default\n'
  printf '\033[33m0b1b17d\033[0m Background opacity\n'
  printf '\n\033[34m~/dev/terminal\033[0m \033[32mmain\033[0m\n\033[92m❯\033[0m cargo test -p tterm-term\n'
  printf '   \033[1;32mCompiling\033[0m tterm-term v0.2.0\n'
  printf '    \033[1;32mFinished\033[0m test profile [unoptimized + debuginfo] target(s) in 4.12s\n'
  printf '     \033[1;32mRunning\033[0m unittests src/lib.rs\n\n'
  printf 'running 34 tests\n'
  for t in emulator::a_dragged_selection_is_shown_and_copied emulator::dead_keys_compose \
           emulator::selected_cells_swap_text_and_background pty::the_shell_starts_in_its_folder \
           key::ctrl_c_interrupts integration::zsh_reports_its_folder; do
    printf 'test %s ... \033[32mok\033[0m\n' "$t"
  done
  printf '\ntest result: \033[32mok\033[0m. 34 passed; 0 failed; 0 ignored; finished in 0.25s\n'
  ;;
esac
