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
  printf '\033[33m478b310\033[0m (\033[36mHEAD -> \033[32mmain\033[0m) Show spend, projects and habits in the agent usage screen\n'
  printf '\033[33mf00f7b1\033[0m Show what'"'"'s new once after an update\n'
  printf '\033[33m422a227\033[0m Scroll the tab strip when the tabs do not fit\n'
  printf '\033[33m6e1c441\033[0m Restyle the active tab: a shorter accent line on a lighter bar\n'
  printf '\n\033[34m~/dev/terminal\033[0m \033[32mmain\033[0m\n\033[92m❯\033[0m cargo test -p tterm-term\n'
  printf '   \033[1;32mCompiling\033[0m tterm-term v0.3.1\n'
  printf '    \033[1;32mFinished\033[0m test profile [unoptimized + debuginfo] target(s) in 4.12s\n'
  printf '     \033[1;32mRunning\033[0m unittests src/lib.rs\n\n'
  printf 'running 39 tests\n'
  for t in emulator::a_dragged_selection_is_shown_and_copied emulator::dead_keys_compose \
           emulator::selected_cells_swap_text_and_background pty::the_shell_starts_in_its_folder \
           key::ctrl_c_interrupts integration::zsh_reports_its_folder; do
    printf 'test %s ... \033[32mok\033[0m\n' "$t"
  done
  printf '\ntest result: \033[32mok\033[0m. 39 passed; 0 failed; 0 ignored; finished in 0.25s\n'
  ;;
esac
