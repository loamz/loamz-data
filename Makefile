# Root entry points. Rust: one Cargo workspace.
.PHONY: lint test test-web budget check help worker check-worker

help:
	@echo "make lint     cargo fmt --check + clippy"
	@echo "make test     cargo test (SQLite; add LOAMZ_TEST_POSTGRES_URL and --include-ignored for Postgres)"
	@echo "make budget   release build: idle memory, start time, one binary"
	@echo "make test-web  web app: types and unit tests"
	@echo "make check    lint + test (everything except the SQL worker)"
	@echo "make worker   release build of the SQL worker (compiles DuckDB from source: long the first time; keep CARGO_BUILD_JOBS the same between builds)"
	@echo "make check-worker  lint and test the SQL worker (also compiles DuckDB)"

lint:
	cargo fmt --check
	cargo clippy --workspace --exclude loamz-worker --all-targets -- -D warnings

test:
	cargo test --workspace --exclude loamz-worker

test-web:
	cd web && pnpm exec tsc --noEmit && pnpm test

budget:
	scripts/budget.sh

check: lint test test-web

# The SQL worker embeds DuckDB, which is compiled from C++ source: ten minutes or more the first time. It is kept out of `check` so
# everyday work never pays for it; CI runs `check-worker`. Changing CARGO_BUILD_JOBS between builds makes cargo compile DuckDB again.
worker:
	cargo build --release -p loamz-worker

check-worker:
	cargo clippy -p loamz-worker --all-targets -- -D warnings
	cargo test -p loamz-worker
