## WebAssembly strategy

This note outlines what to compile to WebAssembly and what to leave in TypeScript/JavaScript so the module stays small while still accelerating the heavy parts of mmdflux in the browser.

### Goals and constraints
- Keep the Wasm binary as small as possible; avoid pulling in CLI-only pieces like `clap`.
- Maximize reuse of the proven Rust pipeline for correctness.
- Let the host own all I/O, scheduling, and DOM concerns; keep Wasm pure and deterministic.

### What belongs in Wasm
- **Core pipeline (parser → graph → layout → render)**: compile the library crate (not the CLI) to `cdylib` so we reuse pest parsing, graph construction, dagre layout, routing, and ASCII rendering exactly as in Rust. These are CPU-heavy and correctness-critical; reimplementing them in JS risks drift and duplicates complex logic.
- **Minimal entrypoints**: expose two exports via `wasm-bindgen`:
  - `render_flowchart(input: &str, opts: RenderOptions) -> String` returning the ASCII/Unicode diagram.
  - `layout_flowchart(input: &str, opts: LayoutOptions) -> JsValue` returning layout JSON (node positions, edge waypoints, subgraph bounds) so the host can render in HTML/SVG if desired without rerunning layout in JS.
- **Option shaping**: mirror existing `RenderOptions`/layout config with a thin, serializable struct; keep defaults in Rust to avoid JS duplication.

### What to keep in TypeScript/JavaScript
- **I/O and environment**: file/text acquisition, streaming, fetch, compression, clipboard, and encoding/decoding. Feed plain `string`/`Uint8Array` into Wasm and receive strings/typed arrays back.
- **CLI/UX glue**: argument parsing, feature flags, telemetry, logging, and error presentation. Map Rust errors to JS `Error` early.
- **DOM rendering**: if targeting rich outputs, build HTML/SVG/canvas in JS using the `layout_flowchart` JSON. JS can also handle theming, fonts, and responsiveness without bloating Wasm.
- **Worker orchestration**: run the Wasm module inside a Web Worker; JS owns job scheduling, caching of parsed diagrams, and incremental re-render requests.

### Size and performance tactics
- Build only the library as `crate-type = ["cdylib"]`; do not include the `bin`.
- Enable `wasm-bindgen`/`wasm-pack` with `--release`, `lto = true`, `opt-level = "z"` or `"s"`, and `panic = "abort"` to trim binary size. Consider `wee_alloc`/`dlmalloc` if it wins size after benchmarking.
- Feature-gate any future non-flowchart functionality so it can be excluded from the Wasm build.
- Keep the interface narrow (two exports) to let `wasm-bindgen` and `wasm-opt` tree-shake unused helpers.
- Return compact data: layout JSON should contain only node IDs, shapes, bounding boxes, waypoints, and direction; avoid embedding rendered text when the host will render HTML.

### Recommended API surface
- `render_flowchart(input: string, options?: RenderOptions): string`
- `layout_flowchart(input: string, options?: LayoutOptions): LayoutJson`
- Optional: `version()` for cache-busting and telemetry.
`RenderOptions` should expose ASCII vs Unicode and spacing overrides already present in Rust; `LayoutOptions` can mirror dagre knobs and padding.

### Migration approach
- Start with `render_flowchart` to validate the pipeline in Wasm with existing tests run via `wasm-bindgen-test` or JS harness.
- Add `layout_flowchart` once consumers need structured layouts for HTML/SVG; this keeps the Wasm module focused on parsing/layout while letting JS own presentation.
- Keep a single Rust codepath for both native and Wasm builds to avoid divergence; guard any host-specific functionality behind `cfg` flags if needed.
