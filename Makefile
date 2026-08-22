.DEFAULT_GOAL := test

.PHONY: architecture-check format-check check clippy test verify diff-check

PYTHON ?= python3

architecture-check:
	$(PYTHON) scripts/check_architecture.py
	$(PYTHON) scripts/check_source_growth.py

format-check:
	cargo fmt --all -- --check

check: architecture-check format-check
	cargo check --workspace --all-targets

clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test: check
	cargo test --workspace --all-features

diff-check:
	git diff --check

verify: test clippy diff-check

