import { describe, expect, it } from "vitest";
import { mayNeedBrowserTextMetrics } from "../src/routing.js";

describe("mayNeedBrowserTextMetrics", () => {
  it("returns true when input declares font-family", () => {
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA[X]\nstyle A font-family:Verdana",
        configJson: "{}",
      }),
    ).toBe(true);
  });

  it("returns true for font-size, font-style, or font-weight directives", () => {
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA[X]\nstyle A font-size:12px",
      }),
    ).toBe(true);
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA[X]\nstyle A font-style:italic",
      }),
    ).toBe(true);
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA[X]\nstyle A font-weight:700",
      }),
    ).toBe(true);
  });

  it("returns true when configJson mentions fontFamily, fontSize, or themeVariables", () => {
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA-->B",
        configJson: '{"fontFamily":"Inter"}',
      }),
    ).toBe(true);
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA-->B",
        configJson: '{"fontSize":16}',
      }),
    ).toBe(true);
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA-->B",
        configJson: '{"themeVariables":{"primary":"#fff"}}',
      }),
    ).toBe(true);
  });

  it("returns false for plain input with empty configJson", () => {
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA-->B",
        configJson: "{}",
      }),
    ).toBe(false);
  });

  it("returns false when configJson is omitted entirely", () => {
    expect(mayNeedBrowserTextMetrics({ input: "graph TD\nA-->B" })).toBe(false);
  });

  it("matches case-insensitively in both input and configJson", () => {
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA[X]\nstyle A FONT-FAMILY:Verdana",
      }),
    ).toBe(true);
    expect(
      mayNeedBrowserTextMetrics({
        input: "graph TD\nA-->B",
        configJson: '{"FontFamily":"Inter"}',
      }),
    ).toBe(true);
  });
});
