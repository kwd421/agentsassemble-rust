.DEFAULT_GOAL := test

.PHONY: architecture-check bindings frontend-check desktop-check format-check check clippy test verify diff-check

PYTHON ?= python3

architecture-check:
	$(PYTHON) scripts/check_architecture.py
	$(PYTHON) scripts/check_source_growth.py

bindings:
	cargo run -p agentsassemble-protocol --bin export_types

frontend-check: bindings
	npm --prefix frontend run build
	npm --prefix frontend test

desktop-check:
	npm --prefix desktop run check

format-check:
	cargo fmt --all -- --check

check: architecture-check format-check
	cargo check --workspace --all-targets

clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test: check frontend-check desktop-check
	cargo test --workspace --all-features

diff-check:
	git diff --check

verify: test clippy diff-check
