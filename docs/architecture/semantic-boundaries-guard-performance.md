# Semantic Boundaries Guard Performance Log

This document is the running log for `cargo xtask architecture boundaries`
performance work.
The goal is to capture each experiment, the measured result, and the current
optimization hypotheses so the investigation stays cumulative instead of
starting over each session.

## Benchmark Command

Use:

```bash
cargo xtask architecture boundaries --timings
```

Notes:

- The command may exit non-zero while known test-aware boundary violations
  remain. That is expected for timing probes in this refactor stage; compare
  the `[T]` timing lines, not just the exit code.
- Measurements below were taken on `semantic-architecture-guards` in the
  `mmdflux-semantic-architecture` worktree on 2026-03-17.

## Experiment Log

### 1. Phase-level timing baseline

Result from the first `--timings` pass:

- config load: `0.01s`
- rust-analyzer workspace load: `1.18s`
- top-level boundary discovery: `18.04s`
- semantic module-scope scan: `0.02s`
- qualified path scan: `51.15s`
- total: `70.40s`

Conclusion:

- The performance problem is not the module-scope semantic pass.
- The performance problem is the qualified-path pass.

### 2. Qualified-path subphase split

After breaking down the qualified-path pass:

- source files: `168`
- candidate files: `149`
- parsed files: `131`
- use-tree candidates: `1228`
- top-level path candidates: `1053`
- candidate-file filtering: `0.00s`
- file reads: `0.04s`
- edition attach: `4.09s`
- `sema.parse`: `0.00s`
- module locator setup: `0.00s`
- use-tree descendant walk: `0.14s`
- path descendant walk: `46.85s`
- segment extraction: `0.07s`
- syntactic fast-path resolution: `0.02s`
- semantic fallback resolution: `0.02s`

Conclusion:

- The qualified-path bottleneck is the broad `ast::Path` descendant walk.
- Semantic fallback resolution is negligible.

### 3. Cheap AST helper swap

Experiment:

- Replaced `path.top_path() != path` with `path.parent_path().is_some()`.
- Replaced the ancestor scan for `UseTree` membership with a direct parent
  `UseTree` cast.

Result:

- No material improvement.
- Path-walk time stayed around `47s`.

Conclusion:

- The cost is not explained by those helper calls alone.

### 4. Token-seeded path walk

Experiment:

- Swapped the broad `ast::Path` walk for a token-seeded approach that started
  from `crate` / `self` / `super` tokens via `descendants_with_tokens()`.

Result:

- No material improvement.
- Candidate counts shifted slightly, but end-to-end time stayed essentially the
  same.

Status:

- Reverted. It did not improve the hot path enough to justify keeping the more
  complex traversal.

### 5. Slowest-file reporting

Added per-file path-walk timing and reran the probe.

Representative run:

- total: `70.05s`
- qualified path scan: `50.32s`
- path descendant walk: `46.01s`

Slowest files:

- `src/format.rs`: `43.30s`
- `src/engines/graph/algorithms/layered/kernel/graph.rs`: `0.48s`
- `src/render/text/canvas.rs`: `0.39s`
- everything else: below `0.30s`

Conclusion:

- The path-walk cost is not spread evenly. It is overwhelmingly concentrated in
  `src/format.rs`.

### 6. Raw path-node and skip-check split

Added more detail inside the path loop and reran.

Latest representative run:

- total: `72.53s`
- rust-analyzer workspace load: `1.14s`
- top-level boundary discovery: `18.99s`
- qualified path scan: `52.38s`
- path descendant walk: `48.00s`
- raw path nodes: `62610`
- nested-path skips: `6877`
- use-tree-parent skips: `1858`
- parent-path checks: `0.00s`
- use-tree-parent checks: `0.00s`

Slowest files:

- `src/format.rs` [parsed `#2`]: `45.00s`
  - raw paths: `424`
  - nested skips: `129`
  - use-tree candidates: `1`
  - top-level relative paths: `8`
- `src/engines/graph/algorithms/layered/kernel/graph.rs` [parsed `#106`]: `0.51s`
- `src/render/text/canvas.rs` [parsed `#90`]: `0.42s`

Conclusions:

- `src/format.rs` is not just paying the first-file warmup penalty; it was
  parsed second.
- The expensive work is not `parent_path()` or the “is this path inside a
  use-tree?” filter.
- The remaining suspect is the full `descendants().filter_map(ast::Path::cast)`
  sweep itself.

### 7. Manual inspection of the worst file

`src/format.rs` contains:

- one relative import: `use crate::errors::RenderError;`
- six `Self::Err` references in `FromStr` impl signatures

Implication:

- The current path sweep is paying about `45s` in `src/format.rs` to discover
  only eight top-level relative-path candidates.
- At least some of those candidates are likely `Self::Err`, which is type-level
  and cannot cross a top-level module boundary.

### 8. Text-seeded non-import path lookup

Experiment:

- Kept the existing `use`-tree pass.
- Replaced the broad non-import `ast::Path` descendant walk with a text-seeded
  lookup:
  - scan the raw source text for lowercase `crate::`, `self::`, and `super::`
  - map each match offset back to the enclosing syntax token and path
  - ascend to the top-level path
  - dedupe repeated hits on the same top-level path

Latest representative run:

- total: `24.27s`
- rust-analyzer workspace load: `1.15s`
- top-level boundary discovery: `18.76s`
- semantic module-scope scan: `0.02s`
- qualified path scan: `4.33s`
- use-tree candidates: `1228`
- top-level path candidates: `201`
- text hits: `911`
- duplicate top-level path hits: `51`
- text-seeded lookup: `0.03s`
- token lookup: `0.02s`
- path ascend: `0.00s`
- semantic fallback resolution: `0.00s`

Behavior check:

- The violation set stayed the same.
- The actual top-level edge map stayed the same.
- `src/format.rs` disappeared as a hotspot.

Conclusion:

- The full-file non-import `ast::Path` sweep was the dominant bottleneck.
- Targeting only lowercase module-relative qualifier hits removes that cost
  almost entirely.

### 9. Fixed crate edition instead of `attach_first_edition`

Experiment:

- Replaced per-file `sema.attach_first_edition(file_id)` with
  `EditionedFileId::new(db, file_id, krate.edition(db))`.

Representative run:

- total: `24.69s`
- qualified path scan: `4.66s`
- edition attach: `0.00s`
- `sema.parse`: `0.27s`
- module locator setup: `4.15s`

Behavior check:

- The violation set stayed the same.
- The actual top-level edge map stayed the same.

Conclusion:

- `attach_first_edition` was a real cost.
- Removing it mainly revealed the next bottleneck: `ModuleLocator::for_file`.

### 10. Probable source-boundary prefilter

Experiment:

- Before parsing a candidate file, derive its probable top-level layer from the
  `src/`-relative path.
- Skip files whose top-level path segment or root file stem is not a declared
  source boundary.

Representative run:

- total: `24.23s`
- qualified path scan: `4.40s`
- candidate files: `140`
- parsed files: `140`
- `sema.parse`: `0.09s`
- module locator setup: `4.10s`

Behavior check:

- The violation set stayed the same.
- The actual top-level edge map stayed the same.

Conclusion:

- Skipping undeclared buckets like `internal_tests` removes some wasted work.
- It is a small win, not the next breakthrough.

### 11. Repeated boundary discovery timing

Experiment:

- Timed an immediate second `discover_top_level_boundaries(root, db)` call after the
  first one.

Representative run:

- first discovery: `18.78s`
- repeated discovery: `0.00s`

Conclusion:

- The large `top-level boundary discovery` bucket is first-query semantic/HIR
  warmup.
- The optimization target is not the discovery function itself; it is whether
  that warmup can be avoided or shifted to work we already have to do.

### 12. Lazy `ModuleLocator` for `crate::` paths

Experiment:

- Tried to build `ModuleLocator::for_file` lazily and only for `self::` /
  `super::` cases.
- `crate::...` paths used the file’s probable top-level layer directly.

Result:

- No meaningful end-to-end speedup.
- The attempt also changed the observed violation set by surfacing a different
  engine test import path (`engines -> runtime`) and by changing which sample
  files were reported for existing violations.

Status:

- Reverted. The semantics drift was not justified by the runtime result.

Conclusion:

- `ModuleLocator` is still a likely cost center, but the naive lazy split is
  not a safe optimization.

### 13. rust-analyzer `prefill_caches`

Experiment:

- Enabled `LoadCargoConfig.prefill_caches = true`.

Representative run:

- total: `85.81s`
- rust-analyzer workspace load: `85.46s`
- top-level boundary discovery: `0.00s`
- qualified path scan: `0.33s`

Behavior check:

- The violation set stayed the same.
- The actual top-level edge map stayed the same.

Status:

- Reverted.

Conclusion:

- `prefill_caches` front-loads almost all of the work and is dramatically worse
  for this command.

### 14. Path-inferred source context for ordinary files

Experiment:

- For files without an obvious inline `mod { ... }` block or `#[path = ...]`,
  infer the source boundary/module context directly from the `src/`-relative file
  path.
- Only build `ModuleLocator::for_file` for files that need semantic source
  lookup.

Result:

- No meaningful runtime win.
- The observed violation set drifted by surfacing an extra `engines ->
  runtime` dependency and by changing the sample files reported for existing
  violations.

Status:

- Reverted.

Conclusion:

- File-path ownership is not a safe substitute for semantic source-module
  lookup, even as an optimization for “ordinary” files.
- `ModuleLocator` remains a real cost center, but this shortcut is not
  reliable enough to keep.

### 15. Crate-wide module-locator index

Experiment:

- Replaced per-file `ModuleLocator::for_file(...)` calls with a crate-wide index
  built once by traversing the semantic module tree and grouping located module
  ranges by source file.

Representative run:

- total: `24.73s`
- qualified path scan: `4.46s`
- parsed files: `130` instead of `140`
- module locator index build: `0.00s`
- module locator lookup: `0.00s`
- semantic fallback resolution: `4.18s`

Behavior check:

- The reported violation set stayed the same.
- The scan shape changed: fewer files were parsed and the fallback hotspot moved
  into `src/render/graph/svg/edges/path_emit.rs`.

Status:

- Reverted.

Conclusion:

- The naive crate-wide index is not a semantic drop-in replacement for
  `sema.file_to_module_defs(file_id)`.
- It did not improve end-to-end runtime, and it changed which files
  participated in the scan, so it is not a safe optimization in this form.

### 16. Per-file `ModuleLocator` hotspot breakdown

Experiment:

- Added per-file timing for `ModuleLocator::for_file(...)`, including the number
  of located modules for each parsed file.
- Timed an immediate repeat of the first `ModuleLocator::for_file(...)` call on
  the same file.

Representative run:

- total: `24.96s`
- qualified path scan: `4.55s`
- module locator setup: `4.25s`
- repeated first module locator: `0.00s`

Slowest module-locator files:

- `src/format.rs` [parsed `#1`]: `4.23s` with `1` located module
- all remaining files: effectively `0.00s`

Conclusion:

- The apparent `ModuleLocator` cost is another first-query warmup effect, not a
  broad per-file cost.
- The expensive part is the first `sema.file_to_module_defs(...)` query, and
  immediately repeating it on the same file is free.
- This pushes the remaining investigation toward first-query rust-analyzer/HIR
  warmup, not toward micro-optimizing locator setup across all files.

## Current Optimization Opportunities

### 1. Investigate the first semantic/HIR query cost

Current measurement:

- `top-level boundary discovery` is still about `18.8s`.

Interpretation:

- This bucket is likely paying for lazy rust-analyzer/HIR initialization, not
  just `root.children(db)`.

Next question:

- Can that warmup cost be moved earlier, reduced, or reused without something
  as heavy as `prefill_caches`?

### 2. Investigate `sema.attach_first_edition`

Status:

- Resolved as a bottleneck by using the crate edition directly.

What it exposed:

- `ModuleLocator::for_file` is now the dominant cost inside the qualified-path
  pass at about `4.1s`.

### 3. Investigate `ModuleLocator::for_file`

Current measurement:

- `qualified path module locator setup` looks like about `4.2s`, but the new
  per-file breakdown shows that almost all of it is the very first
  `file_to_module_defs(...)` query on the first parsed file.

Caveat:

- Two attempts to avoid or delay semantic source-module lookup already failed:
  the naive lazy split, the path-inferred source-context shortcut, and the
  crate-wide locator index.
- Any next optimization here needs to preserve source ownership semantics more
  directly than filesystem inference does.

Refined interpretation:

- This bucket is likely more “first query warmup” than “locator setup work”.
- That makes it a sibling of the `top-level boundary discovery` cost, not a large
  independently-repeatable per-file problem.

### 4. Confirm the intended semantics around uppercase `Self::`

Current state:

- The text-seeded lookup keys off lowercase `crate::`, `self::`, and `super::`.
- The measured behavior stayed stable for the current violation set.

Remaining question:

- Do we want an explicit regression case proving that uppercase `Self::...`
  should never participate in module-boundary enforcement?

## Current Bottom Line

The text-seeded lookup prototype solved the main path-pass bottleneck. Total
runtime dropped from roughly `70s` to roughly `24s`, and the qualified-path
pass dropped from roughly `52s` to roughly `4s` without changing the observed
violation set.

The next meaningful performance targets are no longer in the non-import path
sweep. They are the first semantic/HIR warmup (`~19s`) and `ModuleLocator`
setup inside the qualified-path pass (`~4s`), but the current evidence says
both are really first-query warmup costs. The first naive attempts to avoid
them (`prefill_caches`, lazy `ModuleLocator`, path-inferred source context, and
the crate-wide locator index) were not wins.
