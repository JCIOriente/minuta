.PHONY: ci fmt clippy test worktrees-clean

# Single entry point for local validation and CI.
ci: fmt clippy test

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-features

# Remove worktrees whose branch has a merged or closed PR, then prune git's records.
worktrees-clean:
	./scripts/worktrees-clean.sh
