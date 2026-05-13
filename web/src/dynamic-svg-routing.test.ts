import { describe, expect, it, vi } from "vitest";
import { renderPlaygroundRequest } from "./services/dynamic-svg-routing";
import type {
  BrowserTextMetricsRenderRequest,
  RenderRequest,
  RenderResponse,
  RenderWorkerClient,
} from "./services/render-client";

function responseFor(
  request: Pick<RenderRequest, "seq" | "format">,
): RenderResponse {
  return {
    seq: request.seq,
    format: request.format,
    output: `${request.format}-output`,
  };
}

function renderClientFixture(
  overrides: Partial<RenderWorkerClient> = {},
): RenderWorkerClient {
  return {
    render: vi.fn(async (request: RenderRequest) => responseFor(request)),
    renderWithBrowserTextMetrics: vi.fn(
      async (
        request: BrowserTextMetricsRenderRequest,
      ): Promise<RenderResponse> => ({
        seq: request.seq,
        format: "svg",
        output: "dynamic-svg-output",
      }),
    ),
    resolveBrowserTextMetricsRequest: vi.fn(async () => ({
      required: false,
    })),
    validate: vi.fn(async () => '{"valid":true}'),
    terminate: vi.fn(),
    ...overrides,
  };
}

describe("renderPlaygroundRequest", () => {
  it("routes required browser metrics through dynamic render", async () => {
    const client = renderClientFixture({
      resolveBrowserTextMetricsRequest: vi.fn(async () => ({
        required: true,
        browserTextMetrics: {
          defaultStyle: "s0",
          textStyles: [
            {
              id: "s0",
              fontFamily: "Inter",
              fontSize: 16,
              lineHeight: 24,
              fontStyle: "normal",
              fontWeight: "400",
            },
          ],
        },
      })),
    });

    await expect(
      renderPlaygroundRequest(client, {
        seq: 1,
        input: "graph TD\nA[Regular]-->B\nstyle A font-family:Verdana",
        format: "svg",
        configJson: "{}",
      }),
    ).resolves.toEqual({
      seq: 1,
      format: "svg",
      output: "dynamic-svg-output",
    });

    expect(client.resolveBrowserTextMetricsRequest).toHaveBeenCalledWith({
      seq: 1,
      input: "graph TD\nA[Regular]-->B\nstyle A font-family:Verdana",
      format: "svg",
      configJson: "{}",
    });
    expect(client.renderWithBrowserTextMetrics).toHaveBeenCalledWith({
      seq: 1,
      input: "graph TD\nA[Regular]-->B\nstyle A font-family:Verdana",
      configJson: "{}",
      browserTextMetrics: expect.objectContaining({ defaultStyle: "s0" }),
    });
    expect(client.render).not.toHaveBeenCalled();
  });

  it("routes not-required SVG through static render", async () => {
    const client = renderClientFixture();

    await expect(
      renderPlaygroundRequest(client, {
        seq: 2,
        input: "graph TD\nA-->B",
        format: "svg",
        configJson: "{}",
      }),
    ).resolves.toEqual({
      seq: 2,
      format: "svg",
      output: "svg-output",
    });

    expect(client.resolveBrowserTextMetricsRequest).not.toHaveBeenCalled();
    expect(client.render).toHaveBeenCalledWith({
      seq: 2,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
    });
    expect(client.renderWithBrowserTextMetrics).not.toHaveBeenCalled();
  });

  it("routes non-font SVG styles through static render without resolving", async () => {
    const client = renderClientFixture();

    await expect(
      renderPlaygroundRequest(client, {
        seq: 6,
        input: "graph TD\nA-->B\nstyle A fill:#f00",
        format: "svg",
        configJson: "{}",
      }),
    ).resolves.toEqual({
      seq: 6,
      format: "svg",
      output: "svg-output",
    });

    expect(client.resolveBrowserTextMetricsRequest).not.toHaveBeenCalled();
    expect(client.render).toHaveBeenCalledWith({
      seq: 6,
      input: "graph TD\nA-->B\nstyle A fill:#f00",
      format: "svg",
      configJson: "{}",
    });
    expect(client.renderWithBrowserTextMetrics).not.toHaveBeenCalled();
  });

  it("resolves SVG requests when config JSON carries graph font fields", async () => {
    const client = renderClientFixture();

    await renderPlaygroundRequest(client, {
      seq: 7,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: '{"fontFamily":"Inter"}',
    });

    expect(client.resolveBrowserTextMetricsRequest).toHaveBeenCalledWith({
      seq: 7,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: '{"fontFamily":"Inter"}',
    });
    expect(client.render).toHaveBeenCalledWith({
      seq: 7,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: '{"fontFamily":"Inter"}',
    });
  });

  it.each([
    "text",
    "mmds",
  ] as const)("routes %s without resolving browser metrics", async (format) => {
    const client = renderClientFixture();

    await renderPlaygroundRequest(client, {
      seq: 3,
      input: "graph TD\nA-->B",
      format,
      configJson: "{}",
    });

    expect(client.resolveBrowserTextMetricsRequest).not.toHaveBeenCalled();
    expect(client.render).toHaveBeenCalledWith({
      seq: 3,
      input: "graph TD\nA-->B",
      format,
      configJson: "{}",
    });
    expect(client.renderWithBrowserTextMetrics).not.toHaveBeenCalled();
  });

  it("does not fall back to static render when resolver fails", async () => {
    const client = renderClientFixture({
      resolveBrowserTextMetricsRequest: vi.fn(async () => {
        throw new Error("resolver failed");
      }),
    });

    await expect(
      renderPlaygroundRequest(client, {
        seq: 4,
        input: "graph TD\nA[Regular]-->B\nstyle A font-family:Verdana",
        format: "svg",
        configJson: "{}",
      }),
    ).rejects.toThrow("resolver failed");

    expect(client.render).not.toHaveBeenCalled();
    expect(client.renderWithBrowserTextMetrics).not.toHaveBeenCalled();
  });

  it("propagates dynamic render errors", async () => {
    const client = renderClientFixture({
      resolveBrowserTextMetricsRequest: vi.fn(async () => ({
        required: true,
        browserTextMetrics: {
          fontFamily: "Inter",
          fontSizePx: 16,
          lineHeightPx: 24,
        },
      })),
      renderWithBrowserTextMetrics: vi.fn(async () => {
        throw new Error("dynamic render failed");
      }),
    });

    await expect(
      renderPlaygroundRequest(client, {
        seq: 5,
        input: "graph TD\nA[Regular]-->B\nstyle A font-family:Verdana",
        format: "svg",
        configJson: "{}",
      }),
    ).rejects.toThrow("dynamic render failed");

    expect(client.render).not.toHaveBeenCalled();
  });
});
