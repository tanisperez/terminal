.DEFAULT_GOAL := help

.PHONY: help run build release check lint lint-fix format format-check test clean docs screenshots

help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "  run           Build and run the app (debug)"
	@echo "  build         Build in debug mode"
	@echo "  release       Build an optimized release binary"
	@echo "  check         Fast type-check without producing a binary"
	@echo "  lint          Run Clippy (warnings are errors)"
	@echo "  lint-fix      Run Clippy and apply automatic fixes"
	@echo "  format        Format all Rust code with rustfmt"
	@echo "  format-check  Check formatting without modifying files"
	@echo "  test          Run all tests"
	@echo "  clean         Remove build artifacts"
	@echo "  docs          Build the user documentation of the website into site/docs/"
	@echo "  screenshots   Regenerate the website screenshots (site/img)"
	@echo ""
	@echo "libghostty-vt is built from source with Zig 0.17 on Linux (pacman -S zig)."

run:
	cargo run

build:
	cargo build

release:
	cargo build --release

check:
	cargo check --all-targets

lint:
	cargo clippy --all-targets -- -D warnings

lint-fix:
	cargo clippy --all-targets --fix --allow-dirty --allow-staged

format:
	cargo fmt --all

format-check:
	cargo fmt --all -- --check

test:
	cargo test --all-targets

clean:
	cargo clean

docs:
	python3 site/docs-src/build.py

screenshots:
	site/screenshots/capture.sh
