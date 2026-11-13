# Root entry points. Rust: one Cargo workspace.
.PHONY: lint test test-web budget check help worker check-worker

help:
	@echo "make lint     cargo fmt --check + clippy"
	@echo "make test     cargo test (SQLite; add LOAMZ_TEST_POSTGRES_URL and --include-ignored for Postgres)"
	@echo "make budget   release build: idle memory, start time, one binary"
	@echo "make test-web  web app: types and unit tests"
	@echo "make check    lint + test (everything except the SQL worker)"
	@echo "make worker   build the SQL worker against DuckDB's prebuilt library (downloaded once) and put it beside the runner"
	@echo "make check-worker  lint and test the SQL worker"

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

# The SQL worker links DuckDB's own prebuilt library (scripts/fetch-duckdb.sh downloads it once), so building it takes seconds. It is its
# own workspace so `cargo check --workspace` (an editor does this on every save) never has to look at it.
DUCKDB_LIB_DIR ?= $(HOME)/.cache/loamz/duckdb
export DUCKDB_LIB_DIR
export DUCKDB_INCLUDE_DIR ?= $(DUCKDB_LIB_DIR)

worker:
	scripts/fetch-duckdb.sh
	cd server/worker && cargo build --release
	mkdir -p target/release
	cp server/worker/target/release/loamz-worker $(DUCKDB_LIB_DIR)/libduckdb.so target/release/

check-worker:
	scripts/fetch-duckdb.sh
	cd server/worker && cargo clippy --all-targets -- -D warnings && LD_LIBRARY_PATH=$(DUCKDB_LIB_DIR) cargo test
