import { describe, expect, it } from "vitest";
import {
  assertMmdsWasmExports,
  loadMmdsWasm,
  type MmdsWasmExports,
} from "../src/loader.js";

const enabled = process.env.MMDS_WASM_PACKED === "1";

describe.runIf(enabled)(
  "@mmds/wasm packed-bundler integration (MMDS_WASM_PACKED=1)",
  () => {
    it("loadMmdsWasm resolves to the published export shape without awaiting default()", async () => {
      const wasmExports = await loadMmdsWasm();
      expect(typeof wasmExports.render).toBe("function");
      expect(typeof wasmExports.renderWithBrowserTextMetrics).toBe("function");
      expect(typeof wasmExports.browserTextMetricsRequest).toBe("function");
      expect(typeof wasmExports.validate).toBe("function");
      // The bundler target self-initializes via __wbindgen_start at import time.
      // Calling render() immediately must succeed — no `await default()` step.
      const svg = wasmExports.render("graph TD\nA-->B", "svg", "{}");
      expect(typeof svg).toBe("string");
      expect(svg).toContain("<svg");
    });

    it("assertMmdsWasmExports accepts the packed module shape", async () => {
      const wasmExports = await loadMmdsWasm();
      expect(() => assertMmdsWasmExports(wasmExports)).not.toThrow();
    });

    it("optional detect and version exports are present as functions", async () => {
      const wasmExports: MmdsWasmExports = await loadMmdsWasm();
      expect(typeof wasmExports.detect).toBe("function");
      expect(typeof wasmExports.version).toBe("function");
      // Bonus: detect should classify a trivial flowchart input.
      const kind = wasmExports.detect?.("graph TD\nA-->B");
      expect(typeof kind).toBe("string");
    });

    it("validate returns a JSON-shaped string", async () => {
      const wasmExports = await loadMmdsWasm();
      const out = wasmExports.validate("graph TD\nA-->B");
      const parsed = JSON.parse(out) as { valid: unknown };
      expect(typeof parsed.valid).toBe("boolean");
    });
  },
);
