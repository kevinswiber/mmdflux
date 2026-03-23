---
name: verify
description: Run full project verification (lint + test + architecture boundaries)
---

Run the full project verification suite:

```bash
just check
```

This runs three stages in sequence:
1. **Lint** — `cargo +nightly fmt -- --check` and clippy
2. **Test** — `cargo nextest run` (all tests, parallel)
3. **Architecture** — `cargo xtask architecture` (semantic boundary enforcement)

If any stage fails, report the failure clearly and fix it before re-running. Do not skip stages.

If only a specific stage needs re-checking after a fix, run it individually:
- `just lint` — format + clippy only
- `just test` — tests only
- `just boundaries` — architecture boundaries only
