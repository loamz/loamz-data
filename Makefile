# Root entry points. Rust: one Cargo workspace.
.PHONY: lint test test-web budget check help

help:
	@echo "make lint     cargo fmt --check + clippy"
	@echo "make test     cargo test (SQLite; add LOAMZ_TEST_POSTGRES_URL and --include-ignored for Postgres)"
	@echo "make budget   release build: idle memory, start time, one binary"
	@echo "make test-web  web app: types and unit tests"
	@echo "make check    lint + test"

lint:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

test-web:
	cd web && pnpm exec tsc --noEmit && pnpm test

budget:
	scripts/budget.sh

check: lint test test-web
