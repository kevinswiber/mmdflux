# List available recipes
default:
    @just --list

# Run all tests
test *args:
    cargo nextest run {{ args }}

# Run all tests (CI mode: no fail-fast, verbose)
test-ci *args:
    cargo nextest run --profile ci {{ args }}

# Run a specific test file (e.g. just test-file integration)
test-file name *args:
    cargo nextest run --test {{ name }} {{ args }}

# Build (debug)
build *args:
    cargo build {{ args }}

# Build (release)
release *args:
    cargo build --release {{ args }}

# Run clippy and fmt check
lint:
    cargo +nightly fmt -- --check
    cargo clippy --locked --all-targets --all-features -- -D warnings

# Run clippy with auto-fix
fix *args: fmt
    cargo clippy --fix --all-targets --all-features --allow-dirty --allow-staged -- -D warnings {{ args }}

# Format code
fmt:
    cargo +nightly fmt

# Run the CLI
run *args:
    cargo run -- {{ args }}

# Generate a Mermaid dependency map for the Rust crate
module-map *args:
    ./scripts/generate-rust-module-deps.py {{ args }}

# Generate a Mermaid C4 dependency map for the Rust crate
module-map-c4 *args:
    ./scripts/generate-rust-module-deps-c4.py {{ args }}

# Generate a Mermaid SCC-condensed dependency DAG for the Rust crate
module-map-scc *args:
    ./scripts/generate-rust-module-deps-scc.py {{ args }}

# Generate an outbound dependency tree rooted at a module
module-map-outbound module *args:
    python3 ./scripts/generate-rust-module-deps-pivot.py --module {{ module }} --direction outbound --mode tree {{ args }}

# Generate an inbound dependency tree rooted at a module
module-map-inbound module *args:
    python3 ./scripts/generate-rust-module-deps-pivot.py --module {{ module }} --direction inbound --mode tree {{ args }}

# Generate a pivoted SCC-condensed dependency DAG around a module
module-map-pivot-dag module *args:
    python3 ./scripts/generate-rust-module-deps-pivot.py --module {{ module }} --direction both --mode dag --condense-scc {{ args }}

# Run MMDS conformance checks (semantic/layout/visual tiers)
conformance *args:
    cargo nextest run --test mmds_conformance --success-output immediate {{ args }}

# Check that everything compiles, passes lint, tests, and architecture policy
check: lint test architecture

# Build wasm bindings for browser and bundler targets
wasm-build:
    wasm-pack build crates/mmdflux-wasm --target web --dev --out-dir ../../target/wasm-pkg-web
    wasm-pack build crates/mmdflux-wasm --target bundler --dev --out-dir ../../target/wasm-pkg-bundler

# Run the full repo architecture suite.
architecture:
    cargo xtask architecture

# Run the semantic boundaries suite.
boundaries:
    cargo xtask architecture boundaries

# Watch semantic boundaries during larger refactors.
boundaries-watch:
    cargo xtask architecture boundaries --watch

# Build size-optimized release wasm bindings for browser and bundler targets
wasm-build-release:
    CARGO_PROFILE_RELEASE_OPT_LEVEL=z CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_PANIC=abort wasm-pack build crates/mmdflux-wasm --target web --release --out-dir ../../target/wasm-pkg-web
    CARGO_PROFILE_RELEASE_OPT_LEVEL=z CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_PANIC=abort wasm-pack build crates/mmdflux-wasm --target bundler --release --out-dir ../../target/wasm-pkg-bundler

# Run browser-executed wasm-bindgen contract tests
wasm-test:
    just wasm-build
    ./scripts/run-wasm-browser-tests.sh

# Build release wasm artifacts and enforce CI-equivalent size budgets
wasm-size *args:
    ./scripts/check-wasm-size.sh {{ args }}
