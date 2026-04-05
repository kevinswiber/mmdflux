# Architecture Checker

A semantic module dependency guard that enforces architecture rules at the
Rust module level using rust-analyzer's semantic analysis engine.

## Overview

The checker reads a policy file (`boundaries.toml`) that declares the
project's top-level modules and the rules governing their dependencies. It
then loads the crate through rust-analyzer, discovers the actual dependency
edges between modules, and evaluates every rule against the real graph.
Violations are reported with source locations, underlined code snippets, and
actionable diagnostics.

The design separates three concerns:

1. **Edge collection** — semantic analysis via rust-analyzer produces a
   `BoundaryGraph` with provenance-tagged edges
2. **Rule evaluation** — typed rules are checked against the graph
   independently from how edges were collected
3. **Reporting** — violations are rendered as compiler-style diagnostics,
   JSON for IDE integration, or human-readable explain output

This separation is the central architectural lesson from tools like
[ArchUnit](https://www.archunit.org/),
[Import Linter](https://import-linter.readthedocs.io/),
[dependency-cruiser](https://github.com/nicedoc/dependency-cruiser),
Nx [`enforce-module-boundaries`](https://nx.dev/features/enforce-module-boundaries),
and [JS Boundaries](https://github.com/nicolo-ribaudo/eslint-plugin-boundaries).
Unlike those tools, this checker operates on Rust's semantic module tree
rather than file paths, catching re-exports and qualified paths that
path-based checkers miss.

## Policy File

The policy file is `boundaries.toml` at the repository root (override with
the `SEMANTIC_BOUNDARIES_CONFIG` environment variable).

### Structure

```toml
version = 1

[modules]
# Declare governed modules and optional tags
errors = {}
format = {}
graph = {}
engines = { tags = { role = "runtime-facade" } }
render = { tags = { role = "runtime-facade" } }
runtime = {}

[[rules]]
id = "allow-graph"
type = "allow"
[rules.config]
source = "graph"
allowed = ["errors", "format"]

[[exceptions]]
id = "legacy-coupling"
rule_id = "allow-graph"
source = "graph"
target = "render"
reason = "historical coupling being unwound"
owner = "alice"
```

Three top-level sections:

- **`[modules]`** — declares the set of governed module boundaries. Each key
  is a top-level module name. Values are tables with optional `tags`.
  Modules not listed here are not governed (but imports _into_ them from
  governed modules are still checked).

- **`[[rules]]`** — an array of typed architecture rules. Each rule has an
  `id`, a `type`, and a `[rules.config]` table with type-specific fields.

- **`[[exceptions]]`** — named suppressions for known violations. Each
  exception targets a specific rule, source, and target boundary.

### Tags

Modules can carry freeform key-value tags:

```toml
[modules.engines]
tags = { role = "runtime-facade", layer = "layout" }
```

Tags are used for **tag-based rule targeting** — rule fields that accept
boundary lists can alternatively reference boundaries by tag:

```toml
# Explicit list:
targets = ["engines", "render", "builtins", "frontends"]

# Equivalent via tag:
targets = { tag = "role", value = "runtime-facade" }
```

Tag references are resolved at parse time. The rule's internal model always
contains resolved boundary names. If a tag matches zero boundaries, parsing
fails with an error.

Tag targeting is supported on list-typed fields where order doesn't matter:
`allow.allowed`, `protected.targets`, `protected.allowed_importers`,
`independence.members`, `acyclic.members`. It is not supported on
`allow.source` (single selector) or `layers.order` (order matters).

When a rule field accepts an explicit boundary name, it also accepts an exact
module-path selector such as `render::text`. Module-path selectors match that
module and its descendants. Tags still expand only to top-level boundaries,
because tags are declared on `modules.<name>` entries.

## Rule Types

### allow

Controls which modules a given source boundary may depend on.

```toml
[[rules]]
id = "allow-graph"
type = "allow"
[rules.config]
source = "graph"
allowed = ["errors", "format"]
```

Any edge from `graph` to a module not in `allowed` is a violation. This is
the most common rule type — one per governed boundary.

**Fields:**

- `source` — the selector being governed (single explicit boundary or
  module-path selector)
- `allowed` — selectors that `source` may depend on (list or tag reference)

### layers

Enforces a strict layer ordering. Lower layers must not depend on higher
layers.

```toml
[[rules]]
id = "pipeline-layers"
type = "layers"
[rules.config]
order = ["errors", "format", "graph", "engines", "render", "runtime"]
```

A boundary at position _i_ may only depend on boundaries at positions _j_
where _j < i_. An edge from a lower-position boundary to a same-or-higher-
position boundary is a violation. Edges involving boundaries not in the
`order` list are ignored.

**Fields:**

- `order` — selectors from lowest to highest (always explicit, no tag
  support — order matters)

**Example violation:** If `graph` (position 2) depends on `runtime`
(position 5), the violation detail says:
`graph (layer 2) must not depend on runtime (layer 5)`

The same `layers` rule also accepts exact module-path selectors. This is for
intra-boundary structure such as `render::text` versus `render::graph`.

```toml
[[rules]]
id = "render-submodule-layers"
type = "layers"
[rules.config]
order = ["render::text", "render::svg", "render::graph", "render::timeline"]
```

Each `order` entry may be either a top-level boundary name like `graph` or an
exact module-path selector like `render::text`. A module-path selector matches
that module and its descendants. When `layers.order` contains any `::`
selectors, the checker evaluates the rule against the exact module graph and
normalizes each edge to the longest matching selector on both sides.

**Example violation:** If `render::text::canvas` depends on `render::graph`,
the violation detail says:
`render::text (layer 0) must not depend on render::graph (layer 2)`

### protected

Restricts which modules may import a protected boundary. This is the
complement of `allow` — where `allow` controls what a module _can reach_,
`protected` controls what _can reach a module_.

```toml
[[rules]]
id = "protect-runtime-facades"
type = "protected"
[rules.config]
targets = { tag = "role", value = "runtime-facade" }
allowed_importers = ["runtime"]
```

Any edge _to_ a protected target from a source not in `allowed_importers`
is a violation. This catches violations from new modules that haven't been
added to the allow list yet.

`targets` and `allowed_importers` may be top-level boundaries, exact
module-path selectors, or tag expansions.

**Fields:**

- `targets` — selectors whose access is restricted (list or tag reference)
- `allowed_importers` — only these selectors may import the targets (list or
  tag reference)

### independence

Declares a group of peer modules that must not depend on each other.

```toml
[[rules]]
id = "parser-independence"
type = "independence"
[rules.config]
members = ["mermaid", "mmds"]
```

Any direct edge between group members (in either direction) is a violation.
Edges to/from modules outside the group are ignored. `members` may be
top-level boundaries, exact module-path selectors, or tag expansions.

**Fields:**

- `members` — boundaries that must be independent peers (list or tag
  reference)

### acyclic

Enforces that a set of boundaries forms a directed acyclic graph (DAG).

```toml
[[rules]]
id = "no-boundary-cycles"
type = "acyclic"
[rules.config]
members = [
    "builtins", "diagrams", "engines", "errors", "format",
    "frontends", "graph", "mermaid", "mmds", "payload",
    "registry", "render", "runtime", "simplification", "timeline",
]
```

Uses DFS-based cycle detection restricted to the member set. `members` may be
top-level boundaries, exact module-path selectors, or tag expansions. If a
cycle is found, the violation detail includes the deterministic cycle path:
`cycle: a -> b -> c -> a`

Cycles are normalized (rotated so the lexicographically smallest element is
first) and deduplicated for stable output.

**Fields:**

- `members` — selectors that must form a DAG (list or tag reference)

## Exceptions

Exceptions suppress known violations with explicit justification:

```toml
[[exceptions]]
id = "legacy-render-graph-cycle"
rule_id = "pipeline-layers"
source = "render"
target = "graph"
reason = "historical coupling being unwound in plan 0118"
owner = "kevin"
```

Matching is exact: the exception must match the violation's `rule_id`,
`source`, and `target`. No wildcards or fuzzy matching.

**Suppression tracking:** The evaluator reports both suppressed violations
and unused exceptions (suppression debt). Unused exceptions indicate stale
suppressions that should be removed. In `--verbose` mode, both are logged.

**Fields:**

- `id` — unique identifier for this exception
- `rule_id` — which rule this exception applies to
- `source` — source boundary of the suppressed edge
- `target` — target boundary of the suppressed edge
- `reason` — human-readable justification
- `owner` — who owns this exception

## Semantic Edge Collection

The checker uses rust-analyzer to discover actual module dependencies. This
catches imports that path-based tools miss:

- Re-exported symbols resolved through `pub use` chains
- Qualified paths like `crate::graph::Node` used inline
- Trait implementations that pull in cross-module dependencies

Edge collection happens in two passes:

1. **Module-scope pass** — walks the module tree via rust-analyzer's scope
   API, collecting direct imports from each module's scope
2. **Qualified-path pass** — parses source files for `crate::`, `self::`,
   and `super::` path expressions, resolving them semantically

Each edge is tagged with its **provenance**: `ModuleScope` (found by pass
1), `QualifiedPath` (found by pass 2), or `Mixed` (found by both). This
provenance is visible in `explain` output.

The result is a `BoundaryGraph` — a set of boundary nodes and typed edges
with representative source samples. Rules evaluate against this graph, not
against the raw import data.

## CLI

```
cargo xtask architecture [subcommand] [options]
```

### Subcommands

**`check`** (default) — one-shot architecture check. Tries to reuse a warm
host if one is running; falls back to local analysis.

```bash
cargo xtask architecture check
cargo xtask architecture check --fresh    # bypass host, run locally
cargo xtask architecture check --json     # cargo-compatible JSON diagnostics
cargo xtask architecture check --timings  # phase timing breakdown
cargo xtask architecture check --verbose  # detailed diagnostics
```

**`host`** — run the check, then watch for file changes and host results
for one-shot reuse. This keeps a warmed rust-analyzer context in memory so
subsequent `check` commands complete in milliseconds instead of seconds.

```bash
cargo xtask architecture host
cargo xtask architecture host --timings --verbose
```

**`graph`** — print the boundary dependency graph as Mermaid.

```bash
cargo xtask architecture graph
```

Output:

```
graph LR
    errors["errors"]
    format["format"]
    graph["graph"]
    ...
    graph --> errors
    graph --> format
    ...
```

**`explain`** — inspect specific edges or boundaries. Local-only (does not
use host reuse).

```bash
# Why does this edge exist? Which rules govern it?
cargo xtask architecture explain --edge render graph

# What does this boundary depend on? What depends on it?
cargo xtask architecture explain --boundary graph
```

### Check Options

| Flag             | Description                                          |
| ---------------- | ---------------------------------------------------- |
| `--watch, -w`    | Rerun on file changes (interactive, requires TTY)    |
| `--status`       | Print warm host status for this worktree             |
| `--fresh`        | Bypass host reuse, run local analysis                |
| `--fast-exit`    | Don't wait for a warming host; fall back immediately |
| `--json`         | Output cargo-compatible JSON diagnostics             |
| `--notify-dirty` | Tell the host to mark itself dirty (for hooks)       |
| `--timings, -t`  | Print phase timing breakdown                         |
| `--verbose, -v`  | Print verbose diagnostics and debug context          |

## Host Reuse

The architecture check is expensive (~5-30s) because it loads the crate
through rust-analyzer for semantic analysis. The host reuse system
eliminates this cost for repeated checks:

1. `cargo xtask architecture host` starts a long-running process that keeps
   a warmed rust-analyzer context in memory
2. It watches for file changes and incrementally refreshes the context
3. Subsequent `cargo xtask architecture check` commands connect to the host
   via Unix socket (macOS/Linux) or named pipe (Windows) and get results in
   milliseconds
4. If no host is running or the protocol is incompatible, `check` falls
   back to a local analysis

The host uses a cluster model with leader election:

- One leader owns the socket and serves requests
- Zero or more standbys stay warm for failover
- Dead processes are detected via PID validation
- Protocol versioning ensures incompatible hosts trigger local fallback

Host isolation is per-worktree (based on the repository path). The policy
file is re-read from disk on every check, so branch switches that change
`boundaries.toml` are picked up automatically without restarting the host.

## JSON Diagnostics

The `--json` flag outputs cargo-compatible diagnostics suitable for IDE
integration. Each violation is a JSON object with `reason:
"compiler-message"` containing spans, labels, and children notes. The
format matches `cargo check --message-format=json`.

Rule metadata (`rule_id`, `rule_type`, `detail`) appears as additional
children notes when present.

## Influences

The design draws from several architecture enforcement tools:

- **[ArchUnit](https://www.archunit.org/)** (Java) — the concept of typed
  architecture rules evaluated against a dependency graph. ArchUnit's
  `layeredArchitecture()` and `classes().should().onlyBeAccessed()` directly
  inspired the `layers` and `protected` rule types.

- **[Import Linter](https://import-linter.readthedocs.io/)** (Python) —
  contract-based dependency checking with explicit exception management.
  The separation of graph collection from rule evaluation follows Import
  Linter's architecture.

- **[dependency-cruiser](https://github.com/nicedoc/dependency-cruiser)**
  (JavaScript) — rule-based dependency validation with rich reporting. The
  diagnostic output style and the `explain` inspection command draw from
  dependency-cruiser's `cruise --reaches` and `--output-type err`.

- **[Nx `enforce-module-boundaries`](https://nx.dev/features/enforce-module-boundaries)**
  (TypeScript) — tag-based boundary grouping. The tag system for grouping
  modules and targeting rules by tag is directly inspired by Nx's tag
  constraints.

- **[JS Boundaries](https://github.com/nicolo-ribaudo/eslint-plugin-boundaries)**
  (JavaScript) — ESLint-based boundary enforcement. The independence
  contract idea draws from boundaries' element type isolation.

The key difference from all these tools is that this checker operates on
Rust's semantic module tree via rust-analyzer rather than on file paths or
import strings. This catches re-exports, qualified paths, and trait-mediated
dependencies that path-based tools would miss.
