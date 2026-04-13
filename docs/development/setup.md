# Developer Setup

## Quick start with mise

The fastest way to get a complete dev environment is with [mise](https://mise.jdx.dev/):

```bash
# Clone the repo
git clone git@github.com:kevinswiber/mmdflux.git
cd mmdflux

# Install all tools (Rust nightly, Node, Python, cargo bins, etc.)
# Git hooks are installed automatically as part of cocogitto setup.
mise install

# Verify everything works
just check
```

mise reads `mise.toml` at the project root and installs everything automatically, including git hooks for Conventional Commits enforcement. The stable Rust toolchain is managed separately by `rust-toolchain.toml` via rustup.

## Manual setup

If you prefer to manage tools yourself, install the following:

| Tool | Purpose | Install |
|------|---------|---------|
| [Rust](https://rustup.rs/) (stable + nightly) | Build, test, format | `rustup install stable nightly` |
| [just](https://github.com/casey/just) | Task runner | `cargo install just` or `brew install just` |
| [cargo-nextest](https://nexte.st/) | Parallel test runner | `cargo install cargo-nextest` |
| [cocogitto](https://docs.cocogitto.io/) 6.5.0 | Conventional Commits enforcement | `cargo install cocogitto@6.5.0` |
| [cargo-edit](https://github.com/killercup/cargo-edit) | `cargo set-version` used by release hooks | `cargo install cargo-edit` |
| [jq](https://jqlang.github.io/jq/) | JSON processing in scripts and hooks | `brew install jq` or system package manager |

Optional (for specific workflows):

| Tool | Purpose | Install |
|------|---------|---------|
| [wasm-pack](https://rustwasm.github.io/wasm-pack/) | WebAssembly builds | `cargo install wasm-pack` |
| [Node.js](https://nodejs.org/) | MMDS packages, dagre debug scripts | via nvm or brew |
| [gh](https://cli.github.com/) | GitHub CLI for CI checks and release assets | `brew install gh` |
| [@mermaid-js/mermaid-cli](https://github.com/mermaid-js/mermaid-cli) | Mermaid parity comparison | `npm install -g @mermaid-js/mermaid-cli` |

## First-time setup

```bash
# Clone the repo
git clone git@github.com:kevinswiber/mmdflux.git
cd mmdflux

# Install git hooks (enforces Conventional Commits)
just setup-hooks

# Verify everything works
just check
```

## Commit conventions

This project uses [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/). The `commit-msg` hook validates messages automatically via cocogitto.

Format: `<type>(<optional scope>): <description>`

Types: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `ci`, `perf`, `style`, `build`

For non-trivial changes, include a body after a blank line explaining **what** changed and **why**.

## Day-to-day commands

Run `just` to see all available recipes. The most common:

```bash
just test              # Run all tests
just lint              # Clippy + fmt check
just check             # Lint + test + architecture
just fmt               # Format code
just run diagram.mmd   # Run the CLI
```

See the [Justfile](../../Justfile) for the full list.
