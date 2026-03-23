---
name: test
description: Run tests with optional nextest filter expression
---

Run project tests using cargo-nextest.

If `$ARGUMENTS` is provided, pass it as a nextest filter expression:

```bash
just test -E '$ARGUMENTS'
```

If no arguments are provided, run all tests:

```bash
just test
```

Common filter patterns:
- `test(test_name)` — match test by name
- `test(~keyword)` — fuzzy match
- `package(mmdflux)` — only the main crate
- `test(dagre_parity)` — layout parity tests against dagre.js

To run a specific test file instead of a filter expression, use:
```bash
just test-file <name>
```
where `<name>` is the filename without extension (e.g., `just test-file integration_full`).
