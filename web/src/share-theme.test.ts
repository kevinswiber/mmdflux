import { describe, expect, it } from "vitest";
import {
  DEFAULT_SHARE_RENDER_SETTINGS,
  decodeShareState,
  encodeShareState,
} from "./share";
import { resolveTheme } from "./theme";

describe("share state", () => {
  it("encodes and decodes share state losslessly", () => {
    const original = {
      input: "graph LR\nA-->B\nB-->C",
      format: "svg" as const,
      renderSettings: {
        ...DEFAULT_SHARE_RENDER_SETTINGS,
        layoutEngine: "mermaid-layered" as const,
        edgePreset: "basis" as const,
      },
    };

    const encoded = encodeShareState(original);
    const decoded = decodeShareState(encoded);

    expect(decoded).toEqual(original);
  });

  it("decodes legacy v1 payloads with default render settings", () => {
    const legacy = {
      v: 1,
      input: "graph TD\nA-->B",
      format: "text" as const,
    };
    const hash = btoa(JSON.stringify(legacy))
      .replaceAll("+", "-")
      .replaceAll("/", "_")
      .replaceAll("=", "");
    const decoded = decodeShareState(hash);
    expect(decoded).toEqual({
      input: legacy.input,
      format: legacy.format,
      renderSettings: DEFAULT_SHARE_RENDER_SETTINGS,
    });
  });

  it("maps legacy pathDetail values to pathSimplification", () => {
    const legacy = {
      v: 2,
      input: "graph TD\nA-->B",
      format: "mmds" as const,
      renderSettings: {
        layoutEngine: "auto",
        edgePreset: "auto",
        geometryLevel: "layout",
        pathDetail: "compact",
      },
    };
    const hash = btoa(JSON.stringify(legacy))
      .replaceAll("+", "-")
      .replaceAll("/", "_")
      .replaceAll("=", "");
    const decoded = decodeShareState(hash);
    expect(decoded?.renderSettings.pathSimplification).toBe("lossless");
  });
});

describe("theme preference", () => {
  it("applies theme preference and manual override deterministically", () => {
    expect(
      resolveTheme({ preference: "system", systemPrefersDark: true }),
    ).toBe("dark");
    expect(
      resolveTheme({ preference: "system", systemPrefersDark: false }),
    ).toBe("light");
    expect(resolveTheme({ preference: "light", systemPrefersDark: true })).toBe(
      "light",
    );
    expect(resolveTheme({ preference: "dark", systemPrefersDark: false })).toBe(
      "dark",
    );
  });
});
