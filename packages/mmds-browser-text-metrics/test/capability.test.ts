import { describe, expect, it } from "vitest";
import {
  isMmdsBrowserTextMetricsCapabilityError,
  MmdsBrowserTextMetricsCapabilityError,
} from "../src/capability.js";

describe("MmdsBrowserTextMetricsCapabilityError", () => {
  it("defaults worker preflight codes to fallbackEligible: true", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "worker-offscreen-canvas-unavailable",
      message: "no canvas",
    });
    expect(err.fallbackEligible).toBe(true);
  });

  it("defaults main-thread codes to fallbackEligible: false", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "main-thread-canvas-2d-context-unavailable",
      message: "no 2d",
    });
    expect(err.fallbackEligible).toBe(false);
  });

  it("defaults measurement failures to fallbackEligible: false", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "font-load-check-failed",
      message: "Inter unloadable",
    });
    expect(err.fallbackEligible).toBe(false);
  });

  it("sets name to MmdsBrowserTextMetricsCapabilityError", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "unsupported-format",
      message: "x",
    });
    expect(err.name).toBe("MmdsBrowserTextMetricsCapabilityError");
  });

  it("subclasses Error so single-realm instanceof works", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "unsupported-format",
      message: "x",
    });
    expect(err).toBeInstanceOf(Error);
    expect(err).toBeInstanceOf(MmdsBrowserTextMetricsCapabilityError);
  });

  it("predicate accepts real instances and rejects unrelated values", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "unsupported-format",
      message: "x",
    });
    expect(isMmdsBrowserTextMetricsCapabilityError(err)).toBe(true);
    expect(isMmdsBrowserTextMetricsCapabilityError(new Error("plain"))).toBe(
      false,
    );
    expect(isMmdsBrowserTextMetricsCapabilityError(null)).toBe(false);
    expect(isMmdsBrowserTextMetricsCapabilityError(undefined)).toBe(false);
    expect(
      isMmdsBrowserTextMetricsCapabilityError({
        name: "MmdsBrowserTextMetricsCapabilityError",
        message: "x",
        // missing code, fallbackEligible
      }),
    ).toBe(false);
  });

  it("predicate accepts a structuredClone round-trip across realms", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "worker-offscreen-canvas-unavailable",
      message: "no canvas",
    });
    const clone = structuredClone({
      name: err.name,
      code: err.code,
      message: err.message,
      fallbackEligible: err.fallbackEligible,
    });
    expect(isMmdsBrowserTextMetricsCapabilityError(clone)).toBe(true);
  });

  it("predicate rejects an Error with the right name but missing fields", () => {
    const e = new Error("masquerade");
    e.name = "MmdsBrowserTextMetricsCapabilityError";
    expect(isMmdsBrowserTextMetricsCapabilityError(e)).toBe(false);
  });

  it("preserves cssFont when provided", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "font-load-check-failed",
      message: "Inter unloadable",
      cssFont: "16px 'Inter'",
    });
    expect(err.cssFont).toBe("16px 'Inter'");
  });

  it("honors explicit fallbackEligible override on main-thread codes", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "main-thread-canvas-2d-context-unavailable",
      message: "x",
      fallbackEligible: true,
    });
    expect(err.fallbackEligible).toBe(true);
  });

  it("toJSON exposes structural fields without prototype chain", () => {
    const err = new MmdsBrowserTextMetricsCapabilityError({
      code: "worker-font-face-set-unavailable",
      message: "no fonts",
      cssFont: "16px Inter",
    });
    const json = JSON.parse(JSON.stringify(err));
    expect(json).toEqual({
      name: "MmdsBrowserTextMetricsCapabilityError",
      code: "worker-font-face-set-unavailable",
      message: "no fonts",
      fallbackEligible: true,
      cssFont: "16px Inter",
    });
  });
});
