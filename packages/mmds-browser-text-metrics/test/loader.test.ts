import { describe, expect, expectTypeOf, it } from "vitest";
import {
  assertMmdsWasmExports,
  loadMmdsWasm,
  type MmdsWasmExports,
  type MmdsWasmModuleLoader,
} from "../src/loader.js";

describe("loadMmdsWasm", () => {
  it("is a function returning Promise<MmdsWasmExports>", () => {
    expect(typeof loadMmdsWasm).toBe("function");
    expectTypeOf(loadMmdsWasm).toEqualTypeOf<MmdsWasmModuleLoader>();
    expectTypeOf(
      loadMmdsWasm,
    ).returns.resolves.toEqualTypeOf<MmdsWasmExports>();
  });
});

describe("MmdsWasmExports surface", () => {
  it("treats default as optional (bundler target self-initializes)", () => {
    // A module without `default` must still satisfy the contract — proves the
    // bundler target (`wasm-pack --target bundler`) is supported.
    const bundlerModule: MmdsWasmExports = {
      render: () => "svg",
      renderWithBrowserTextMetrics: () => "svg",
      browserTextMetricsRequest: () => '{"required":false}',
      validate: () => '{"valid":true}',
    };
    expectTypeOf(bundlerModule).toMatchTypeOf<MmdsWasmExports>();
  });

  it("treats detect, version, and default as optional", () => {
    const withOptionals: MmdsWasmExports = {
      render: () => "svg",
      renderWithBrowserTextMetrics: () => "svg",
      browserTextMetricsRequest: () => "{}",
      validate: () => "{}",
      default: async () => undefined,
      detect: () => "flowchart",
      version: () => "2.4.2",
    };
    expectTypeOf(withOptionals.default).toMatchTypeOf<
      ((init?: unknown) => Promise<unknown>) | undefined
    >();
    expectTypeOf(withOptionals.detect).toMatchTypeOf<
      ((input: string) => string | undefined) | undefined
    >();
    expectTypeOf(withOptionals.version).toMatchTypeOf<
      (() => string) | undefined
    >();
  });
});

describe("assertMmdsWasmExports", () => {
  it("accepts a minimal bundler-target export shape", () => {
    expect(() =>
      assertMmdsWasmExports({
        render: () => "",
        renderWithBrowserTextMetrics: () => "",
        browserTextMetricsRequest: () => "",
        validate: () => "",
      }),
    ).not.toThrow();
  });

  it("rejects missing required methods", () => {
    expect(() => assertMmdsWasmExports({})).toThrow(/render/);
    expect(() =>
      assertMmdsWasmExports({
        render: () => "",
        renderWithBrowserTextMetrics: () => "",
        browserTextMetricsRequest: () => "",
        // validate missing
      }),
    ).toThrow(/validate/);
  });

  it("rejects non-function method shapes", () => {
    expect(() =>
      assertMmdsWasmExports({
        render: "not a function",
        renderWithBrowserTextMetrics: () => "",
        browserTextMetricsRequest: () => "",
        validate: () => "",
      }),
    ).toThrow(/render/);
  });

  it("rejects null and primitive values", () => {
    expect(() => assertMmdsWasmExports(null)).toThrow();
    expect(() => assertMmdsWasmExports(undefined)).toThrow();
    expect(() => assertMmdsWasmExports(42)).toThrow();
  });
});

describe.runIf(process.env.MMDS_WASM_PACKED === "1")(
  "packed @mmds/wasm smoke (gated by MMDS_WASM_PACKED=1)",
  () => {
    it("loadMmdsWasm() resolves to a usable exports object without awaiting default()", async () => {
      const exports = await loadMmdsWasm();
      expect(typeof exports.render).toBe("function");
      expect(typeof exports.renderWithBrowserTextMetrics).toBe("function");
      expect(typeof exports.browserTextMetricsRequest).toBe("function");
      expect(typeof exports.validate).toBe("function");
      // Bundler target: render() must work immediately (no default() step).
      const out = exports.render("graph TD\nA-->B", "svg", "{}");
      expect(typeof out).toBe("string");
      expect(out).toContain("<svg");
    });
  },
);
