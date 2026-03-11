This directory holds locked MMDS contract fixtures shared across Rust and TypeScript tests.

- `*.json` files are the expected MMDS contract payloads consumed by Rust and TS packages.
- Core fields live in the MMDS envelope directly.
- Profile and extension behavior remains explicit in `profiles/` and extension-specific fixtures.
- Source Mermaid diagrams continue to live under the regular diagram fixture trees such as `tests/fixtures/flowchart/`.
