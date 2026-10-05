import { describe, expect, it, vi } from "vitest";
import {
  createFittedRenderHandler,
  FITTED_RENDER_REQUEST,
  type FittedRenderWasmModule,
  isFittedRenderRequestMessage,
} from "./fitted-worker";

function fakeModule(
  renderFitted: FittedRenderWasmModule["renderFitted"],
): FittedRenderWasmModule {
  return { default: vi.fn(async () => {}), renderFitted };
}

const fitReport = {
  outcome: "fitted",
  size: { width: 38, height: 20 },
  asAuthored: { width: 62, height: 14 },
  applied: [{ lever: "labelAwareSpacing" }],
};

describe("createFittedRenderHandler", () => {
  it("recognizes only fitted render requests", () => {
    const request = {
      version: 1,
      type: FITTED_RENDER_REQUEST,
      seq: 1,
      input: "graph LR\nA-->B",
      format: "text",
      configJson: "{}",
      fitJson: '{"maxWidth":40}',
    };

    expect(isFittedRenderRequestMessage(request)).toBe(true);
    expect(isFittedRenderRequestMessage({ ...request, type: "render" })).toBe(
      false,
    );
    expect(
      isFittedRenderRequestMessage({ ...request, fitJson: undefined }),
    ).toBe(false);
  });

  it("renders through renderFitted and posts the output with its fit report", async () => {
    const postMessage = vi.fn();
    const renderFitted = vi.fn(() =>
      JSON.stringify({ output: "fitted text", fit: fitReport }),
    );
    const module = fakeModule(renderFitted);
    const handle = createFittedRenderHandler({
      loadWasmModule: async () => module,
      postMessage,
    });

    await expect(
      handle({
        version: 1,
        type: FITTED_RENDER_REQUEST,
        seq: 7,
        input: "graph LR\nA-->B",
        format: "text",
        configJson: '{"color":"always"}',
        fitJson: '{"maxWidth":40}',
      }),
    ).resolves.toBeUndefined();

    expect(module.default).toHaveBeenCalledTimes(1);
    expect(renderFitted).toHaveBeenCalledWith(
      "graph LR\nA-->B",
      "text",
      '{"color":"always"}',
      '{"maxWidth":40}',
    );
    expect(postMessage).toHaveBeenCalledWith({
      version: 1,
      type: "fittedResult",
      seq: 7,
      format: "text",
      output: "fitted text",
      fit: fitReport,
    });
  });

  it("posts wasm errors as worker errors for the request", async () => {
    const postMessage = vi.fn();
    const handle = createFittedRenderHandler({
      loadWasmModule: async () =>
        fakeModule(() => {
          throw new Error("max width must be at least 1");
        }),
      postMessage,
    });

    await handle({
      version: 1,
      type: FITTED_RENDER_REQUEST,
      seq: 9,
      input: "graph LR\nA-->B",
      format: "text",
      configJson: "{}",
      fitJson: '{"maxWidth":0}',
    });

    expect(postMessage).toHaveBeenCalledWith({
      version: 1,
      type: "error",
      seq: 9,
      error: "max width must be at least 1",
    });
  });
});
