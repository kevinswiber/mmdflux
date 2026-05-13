import type {
  RenderRequest,
  RenderResponse,
  RenderWorkerClient,
} from "./render-client";

export async function renderPlaygroundRequest(
  client: RenderWorkerClient,
  request: RenderRequest,
): Promise<RenderResponse> {
  if (request.format !== "svg" || !mayNeedBrowserTextMetrics(request)) {
    return client.render(request);
  }

  const decision = await client.resolveBrowserTextMetricsRequest(request);
  if (!decision.required) {
    return client.render(request);
  }
  if (!decision.browserTextMetrics) {
    throw new Error(
      "browser text metrics decision required metrics but omitted the request",
    );
  }

  // Reuse the live-render seq only after the awaited resolver call has freed
  // its worker pending slot.
  return client.renderWithBrowserTextMetrics({
    seq: request.seq,
    input: request.input,
    configJson: request.configJson,
    browserTextMetrics: decision.browserTextMetrics,
  });
}

function mayNeedBrowserTextMetrics(request: RenderRequest): boolean {
  const input = request.input.toLowerCase();
  if (
    input.includes("font-family") ||
    input.includes("font-size") ||
    input.includes("font-style") ||
    input.includes("font-weight")
  ) {
    return true;
  }

  const configJson = request.configJson?.toLowerCase() ?? "";
  return (
    configJson.includes("fontfamily") ||
    configJson.includes("fontsize") ||
    configJson.includes("themevariables")
  );
}
