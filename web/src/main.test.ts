import { describe, expect, it } from "vitest";
import { renderApp } from "./main";

describe("renderApp", () => {
  it("renders redesigned playground shell", () => {
    const root = document.createElement("div");
    renderApp(root);

    expect(root.textContent).toContain("mmdflux playground");
    expect(root.textContent).toContain("Advanced controls");
    expect(root.textContent).toContain("Syntax snippets");
    expect(root.querySelector("[data-preview-controls]")).not.toBeNull();
  });
});
