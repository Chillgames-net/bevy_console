CARGO ?= cargo

.DEFAULT_GOAL := help

.PHONY: help format check package publish

help:
	@printf '%s\n' \
		'make format   Format the workspace' \
		'make check    Run the same checks as CI' \
		'make package  Verify the crates.io packages' \
		'make publish  Check and publish the workspace'

format:
	$(CARGO) fmt --all

check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets --all-features --locked -- -D warnings
	$(CARGO) test --workspace --all-features --all-targets --locked
	$(CARGO) test --workspace --no-default-features --lib --locked
	$(CARGO) test --workspace --doc --all-features --locked
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --all-features --locked

package: check
	$(CARGO) publish --workspace --dry-run --allow-dirty --locked

publish: check
	$(CARGO) publish --workspace --locked
