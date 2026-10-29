# Root entry points. Rust: one Cargo workspace. Python: only the embeddable engine (server/engine).
.PHONY: lint test test-engine imports budget check help

help:
	@echo "make lint     cargo fmt --check + clippy"
	@echo "make test     cargo test (SQLite; add LOAMZ_TEST_POSTGRES_URL and --include-ignored for Postgres)"
	@echo "make budget   release build: idle memory, start time, one binary"
	@echo "make imports  engine import contract"
	@echo "make check    lint + test + imports"

lint:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

test-engine:
	cd server/engine && uv run --package loamz-engine pytest -q -n 4 --timeout=120

imports:
	uv run lint-imports --config .importlinter

budget:
	scripts/budget.sh

check: lint test imports
