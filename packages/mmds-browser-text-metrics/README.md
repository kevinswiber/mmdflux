# @mmds/browser-text-metrics

Browser-side adapter for `@mmds/wasm` dynamic text metrics. Provides the
`OffscreenCanvas` + `FontFaceSet` preflight, worker dispatch, and main-thread
fallback that the mmdflux playground uses to render Mermaid graphs with the
user's actual fonts.

This package is the supported boundary; consumers should not reach into the
internals of `@mmds/wasm`.

## Status

Pre-release. The package surface is still being shaped and may change
between minor versions before 1.0.0; pin exact versions in dependants.

## Subpath exports

| Subpath              | Purpose                                                            |
| -------------------- | ------------------------------------------------------------------ |
| `.`                  | Pure types + `MmdsBrowserTextMetricsCapabilityError` + predicate       |
| `./client`           | `createMmdsBrowserTextMetricsClient` orchestration                     |
| `./main-thread`      | `createMmdsMainThreadRenderer` factory                             |
| `./worker`           | `createWorkerRequestHandler` for worker hosts                      |
| `./worker-protocol`  | Worker request/response message types                              |
| `./loader`           | `loadMmdsWasm` + `MmdsWasmExports` (the only `@mmds/wasm` seam)    |
| `./routing`          | `mayNeedBrowserTextMetrics` heuristic                              |
| `./fixtures`         | Test fixtures for downstream consumers                             |

## Quickstart

Install the package and its `@mmds/wasm` peer:

```bash
npm install @mmds/browser-text-metrics @mmds/wasm
```

The worker-backed client needs two source files: a worker host that owns
wasm execution, and a main-thread caller that dispatches render requests
to it.

```ts
// worker.ts — the dedicated worker entry point
import { loadMmdsWasm } from "@mmds/browser-text-metrics/loader";
import { createWorkerRequestHandler } from "@mmds/browser-text-metrics/worker";

const handle = createWorkerRequestHandler({
  loadWasmModule: loadMmdsWasm,
  postMessage: (message) => self.postMessage(message),
});

self.onmessage = (event) => {
  void handle(event.data);
};
```

```ts
// render.ts — the main-thread caller
import {
  createDefaultMmdsWorker,
  createMmdsBrowserTextMetricsClient,
} from "@mmds/browser-text-metrics/client";

const worker = createDefaultMmdsWorker();
const client = createMmdsBrowserTextMetricsClient({ worker });

const result = await client.renderAuto({
  input: `graph TD
    A[Regular] --> B[Styled]
    style B font-family:Verdana,font-size:20px`,
  format: "svg",
  configJson: "{}",
});

document.querySelector("#diagram")!.innerHTML = result.output;
```

`renderAuto` decides per-input whether the wasm resolver needs canvas
measurements and either renders statically or routes through the dynamic
preflight; callers that want explicit control can use `renderStatic`,
`resolveBrowserTextMetricsRequest`, and `renderSvg` directly.

For environments that may not provide a `Worker` global (SSR, tests,
some hosts), `createAutoMmdsBrowserTextMetricsClient` picks between the
worker-backed client and a main-thread fallback:

```ts
import {
  createAutoMmdsBrowserTextMetricsClient,
  createDefaultMmdsWorker,
} from "@mmds/browser-text-metrics/client";
import { createMmdsMainThreadRenderer } from "@mmds/browser-text-metrics/main-thread";

const client = createAutoMmdsBrowserTextMetricsClient({
  workerFactory: createDefaultMmdsWorker,
  mainThreadFactory: () => createMmdsMainThreadRenderer(),
});
```

### Bundler coupling

`createDefaultMmdsWorker` is a one-liner convenience:

```ts
new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
```

That pattern is what Vite and webpack 5 recognise to bundle the worker
entrypoint. If your toolchain does not transform the
`new URL(..., import.meta.url)` form (esbuild without a worker plugin,
hand-rolled bundles, Comlink/partytown wrappers, electron preload
scripts, etc.), do not call `createDefaultMmdsWorker`. Construct the
worker the way your environment expects and pass it directly to
`createMmdsBrowserTextMetricsClient` — every code path that touches the
worker goes through the `MmdsWorkerLike` interface, so custom
constructors work without casts.

## Routing helper (opt-in)

`mayNeedBrowserTextMetrics({ input, configJson? })` is an **opt-in**
short-circuit. It returns `true` when the diagram input contains
`font-family` / `font-size` / `font-style` / `font-weight` directives, or
when `configJson` mentions `fontFamily`, `fontSize`, or `themeVariables`.
Consumers can use it to skip the wasm resolver round-trip when the input
clearly does not declare custom fonts. The client's own `renderAuto` already
composes this helper; importing `./routing` directly is for consumers that
roll their own orchestration. The default render path does **not** require
the helper — it is not part of the client's required routing policy.

A `false` result does not guarantee static rendering produces identical
output if the runtime config uses theme variables outside the enumerated
keys; consumers should treat the helper as a conservative skip-the-resolver
heuristic, not a definitive answer.

## License

MIT
