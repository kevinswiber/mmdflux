import { describe, expect, it } from "vitest";
import { createPreviewController } from "./preview";

describe("createPreviewController", () => {
  it("switches preview rendering mode when format tab changes", () => {
    const output = document.createElement("div");
    const error = document.createElement("p");
    const preview = createPreviewController({ output, error });

    preview.showResult({
      format: "text",
      output: "<svg data-kind='text'></svg>",
    });

    expect(output.textContent).toBe("<svg data-kind='text'></svg>");
    expect(output.querySelector("svg")).toBeNull();

    preview.showResult({
      format: "svg",
      output: "<svg data-kind='svg'></svg>",
    });

    expect(output.querySelector("svg")?.getAttribute("data-kind")).toBe("svg");
  });

  it("renders text previews as plain text, styled ansi, and escaped sequences", () => {
    const output = document.createElement("div");
    const error = document.createElement("p");
    const preview = createPreviewController({ output, error });
    const ansiOutput = "\u001b[38;2;255;0;0mAlpha\u001b[0m";

    preview.showResult({
      format: "text",
      output: ansiOutput,
    });

    expect(output.textContent).toBe("Alpha");
    expect(output.querySelector("pre")).toBeNull();

    preview.setTextMode("styled");

    const styledBlock = output.querySelector("pre");
    const styledSpan = styledBlock?.querySelector("span");
    expect(styledBlock?.textContent).toBe("Alpha");
    expect(styledSpan?.style.color).toBe("rgb(255, 0, 0)");

    preview.setTextMode("ansi");
    expect(output.textContent).toBe("\\x1b[38;2;255;0;0mAlpha\\x1b[0m");

    preview.setTextMode("plain");
    expect(output.textContent).toBe("Alpha");
  });

  it("shows human-readable error panel when worker returns error", () => {
    const output = document.createElement("div");
    const error = document.createElement("p");
    const preview = createPreviewController({ output, error });

    preview.showError("parse error: expected node id");

    expect(error.hidden).toBe(false);
    expect(error.textContent).toContain("parse error: expected node id");
  });
});
