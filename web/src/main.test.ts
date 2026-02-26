import { describe, expect, it } from "vitest";
import { renderApp } from "./main";

describe("renderApp", () => {
  it("renders redesigned playground shell", () => {
    try {
      history.replaceState(null, "", window.location.pathname);

      const root = document.createElement("div");
      renderApp(root, {
        renderClientFactory: () => ({
          render: async (request) => ({
            seq: request.seq,
            format: request.format,
            output: `${request.format}:${request.input}`,
          }),
          terminate: () => {},
        }),
        stateStorage: {
          getItem: () => null,
          setItem: () => {},
        },
      });
      const exampleSelect = root.querySelector<HTMLSelectElement>(
        "[data-example-select]",
      );
      const activeFormat = root.querySelector<HTMLButtonElement>(
        ".format-tabs button.is-active",
      );

      expect(root.textContent).toContain("mmdflux playground");
      expect(root.textContent).toContain("Advanced controls");
      expect(root.textContent).toContain("Syntax snippets");
      expect(activeFormat?.dataset.format).toBe("svg");
      expect(root.querySelector("[data-preview-controls]")).not.toBeNull();
      expect(root.querySelector("[data-theme-toggle]")).not.toBeNull();
      expect(exampleSelect?.value).toBe("__draft__");
    } finally {
      history.replaceState(null, "", window.location.pathname);
    }
  });
});
