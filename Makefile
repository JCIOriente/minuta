.PHONY: ci fmt clippy file-size test worktrees-clean

# Single entry point for local validation and CI.
ci: fmt clippy file-size test

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

# Size limit for .rs files under src/ (500 lines, ratchet via the allowlist).
# Tests the check first, then runs it.
file-size:
	./scripts/check-file-size-test.sh
	./scripts/check-file-size.sh

test:
	cargo test --all-features

# Remove worktrees whose branch has a merged or closed PR, then prune git's records.
worktrees-clean:
	./scripts/worktrees-clean.sh
