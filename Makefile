.PHONY: ci fmt clippy test

# Single entry point for local validation and CI.
ci: fmt clippy test

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-features
