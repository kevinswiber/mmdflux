import { mayNeedBrowserTextMetrics } from "@mmds/browser-text-metrics/routing";
import { isDynamicRenderOutputFormat } from "@mmds/browser-text-metrics/worker-protocol";
import type {
  RenderRequest,
  RenderResponse,
  RenderWorkerClient,
} from "./render-client";

export async function renderPlaygroundRequest(
  client: RenderWorkerClient,
  request: RenderRequest,
): Promise<RenderResponse> {
  if (
    !isDynamicRenderOutputFormat(request.format) ||
    !mayNeedBrowserTextMetrics(request)
  ) {
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
    format: request.format,
  });
}
