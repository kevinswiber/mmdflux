# Developer Setup

## Prerequisites

Install the following tools before working on mmdflux:

| Tool | Purpose | Install |
|------|---------|---------|
| [Rust](https://rustup.rs/) (stable + nightly) | Build, test, format | `rustup install stable nightly` |
| [just](https://github.com/casey/just) | Task runner | `cargo install just` or `brew install just` |
| [cargo-nextest](https://nexte.st/) | Parallel test runner | `cargo install cargo-nextest` |
| [cocogitto](https://docs.cocogitto.io/) | Conventional Commits enforcement | `cargo install cocogitto` or `brew install cocogitto` |

Optional (for specific workflows):

| Tool | Purpose | Install |
|------|---------|---------|
| [wasm-pack](https://rustwasm.github.io/wasm-pack/) | WebAssembly builds | `cargo install wasm-pack` |
| [Node.js](https://nodejs.org/) | Dagre debug scripts, web playground | via nvm or brew |

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
