import { describe, expect, it } from "vitest";
import { buildCssFont, cssFontFamilyStack } from "../src/css-font.js";

describe("buildCssFont", () => {
  it("formats a single named family", () => {
    expect(
      buildCssFont({
        fontFamily: "Open Sans",
        fontSizePx: 16,
        lineHeightPx: 24,
      }),
    ).toBe('normal 400 16px "Open Sans"');
  });

  it("normalizes a comma stack with mixed quoting", () => {
    expect(
      buildCssFont({
        fontFamily: 'Arial, "Trebuchet MS", sans-serif',
        fontSizePx: 16,
        lineHeightPx: 24,
      }),
    ).toBe('normal 400 16px "Arial", "Trebuchet MS", sans-serif');
  });

  it("honors fontStyle and fontWeight", () => {
    expect(
      buildCssFont({
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
        fontStyle: "italic",
        fontWeight: "600",
      }),
    ).toBe('italic 600 16px "Inter"');
  });

  it("treats generic families as keywords (sans-serif unquoted)", () => {
    expect(
      buildCssFont({
        fontFamily: "sans-serif",
        fontSizePx: 16,
        lineHeightPx: 24,
      }),
    ).toBe("normal 400 16px sans-serif");
  });

  it("rejects empty fontFamily", () => {
    expect(() =>
      buildCssFont({ fontFamily: "", fontSizePx: 16, lineHeightPx: 24 }),
    ).toThrow();
  });

  it("rejects fontSizePx = 0", () => {
    expect(() =>
      buildCssFont({ fontFamily: "Inter", fontSizePx: 0, lineHeightPx: 24 }),
    ).toThrow(/positive/);
  });

  it("rejects lineHeightPx = NaN", () => {
    expect(() =>
      buildCssFont({
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: Number.NaN,
      }),
    ).toThrow();
  });

  it("escapes embedded quotes in family", () => {
    expect(
      buildCssFont({
        fontFamily: 'Foo"Bar',
        fontSizePx: 16,
        lineHeightPx: 24,
      }),
    ).toBe('normal 400 16px "Foo\\"Bar"');
  });
});

describe("cssFontFamilyStack", () => {
  it("quotes named families and lowercases generic keywords", () => {
    expect(cssFontFamilyStack('Inter, "Helvetica Neue", monospace')).toBe(
      '"Inter", "Helvetica Neue", monospace',
    );
  });
});
