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

# Run clippy, architecture boundaries, and fmt check
lint: fmt-check
    cargo xtask lint

# Run clippy with auto-fix
fix *args: fmt
    cargo clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged -- -D warnings {{ args }}

# Format code
fmt *args:
    cargo +nightly fmt --all {{ args }}

fmt-check:
    cargo +nightly fmt --all -- --check

# Install git hooks (commit-msg validation via cocogitto)
setup-hooks:
    cog install-hook --all --overwrite

# Run the CLI
run *args:
    cargo run -- {{ args }}

# Generate a Mermaid dependency map for the Rust crate
module-map *args:
    ./scripts/module-deps/flowchart.mjs {{ args }}

# Generate a Mermaid C4 dependency map for the Rust crate
module-map-c4 *args:
    ./scripts/module-deps/c4.mjs {{ args }}

# Generate a Mermaid SCC-condensed dependency DAG for the Rust crate
module-map-scc *args:
    ./scripts/module-deps/scc.mjs {{ args }}

# Generate an outbound dependency tree rooted at a module
module-map-outbound module *args:
    ./scripts/module-deps/pivot.mjs --module {{ module }} --direction outbound --mode tree {{ args }}

# Generate an inbound dependency tree rooted at a module
module-map-inbound module *args:
    ./scripts/module-deps/pivot.mjs --module {{ module }} --direction inbound --mode tree {{ args }}

# Generate a pivoted SCC-condensed dependency DAG around a module
module-map-pivot-dag module *args:
    ./scripts/module-deps/pivot.mjs --module {{ module }} --direction both --mode dag --condense-scc {{ args }}

# Run MMDS conformance checks (semantic/layout/visual tiers)
conformance *args:
    cargo nextest run --test mmds_conformance --success-output immediate {{ args }}

# Check that everything compiles, passes lint, tests, and architecture policy
check: lint test

# Build wasm bindings for browser and bundler targets
wasm-build:
    wasm-pack build crates/mmdflux-wasm --target web --dev --out-dir ../../target/wasm-pkg-web
    wasm-pack build crates/mmdflux-wasm --target bundler --dev --out-dir ../../target/wasm-pkg-bundler

# Run the full repo architecture suite.
architecture:
    cargo xtask architecture

# Run the semantic boundaries check.
architecture-check:
    cargo xtask architecture check

# Watch semantic boundaries during larger refactors and host results for one-shot reuse.
architecture-host:
    cargo xtask architecture host

# Print the semantic boundary dependency graph as Mermaid.
architecture-graph:
    cargo xtask architecture graph

# Explain a specific edge or boundary in the semantic dependency graph.
architecture-explain *args:
    cargo xtask architecture explain {{ args }}

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
