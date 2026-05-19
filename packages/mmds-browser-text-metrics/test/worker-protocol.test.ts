import { describe, expect, it } from "vitest";
import {
  isWorkerBrowserTextMetricsDecision,
  isWorkerRequestMessage,
  PROTOCOL_VERSION,
  type WorkerRequestMessage,
  type WorkerResponseMessage,
} from "../src/worker-protocol.js";

describe("worker-protocol", () => {
  it("pins PROTOCOL_VERSION to 1", () => {
    expect(PROTOCOL_VERSION).toBe(1);
  });

  it("round-trips the version field through JSON.stringify", () => {
    const msg: WorkerRequestMessage = {
      version: PROTOCOL_VERSION,
      type: "render",
      seq: 7,
      input: "graph TD\nA-->B",
      format: "svg",
      configJson: "{}",
    };
    const parsed = JSON.parse(JSON.stringify(msg));
    expect(parsed.version).toBe(1);
    expect(parsed.type).toBe("render");
    expect(parsed.seq).toBe(7);
  });

  it("accepts every request variant through the type guard", () => {
    const render: WorkerRequestMessage = {
      version: PROTOCOL_VERSION,
      type: "render",
      seq: 1,
      input: "x",
      format: "text",
      configJson: "{}",
    };
    const validate: WorkerRequestMessage = {
      version: PROTOCOL_VERSION,
      type: "validate",
      seq: 2,
      input: "x",
    };
    const resolve: WorkerRequestMessage = {
      version: PROTOCOL_VERSION,
      type: "resolveBrowserTextMetrics",
      seq: 3,
      input: "x",
      format: "svg",
      configJson: "{}",
    };
    const dynamic: WorkerRequestMessage = {
      version: PROTOCOL_VERSION,
      type: "renderWithBrowserTextMetrics",
      seq: 4,
      input: "x",
      format: "svg",
      configJson: "{}",
      browserTextMetrics: {
        fontFamily: "Inter",
        fontSizePx: 16,
        lineHeightPx: 24,
      },
    };
    expect(isWorkerRequestMessage(render)).toBe(true);
    expect(isWorkerRequestMessage(validate)).toBe(true);
    expect(isWorkerRequestMessage(resolve)).toBe(true);
    expect(isWorkerRequestMessage(dynamic)).toBe(true);
  });

  it("rejects render envelopes missing required fields", () => {
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "render",
        seq: 1,
        // missing input, format, configJson
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "render",
        seq: 1,
        input: "x",
        format: "not-a-format",
        configJson: "{}",
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "render",
        seq: 1,
        input: "x",
        format: "svg",
        configJson: 42,
      }),
    ).toBe(false);
  });

  it("accepts renderWithBrowserTextMetrics with mmds format", () => {
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "renderWithBrowserTextMetrics",
        seq: 1,
        input: "x",
        format: "mmds",
        configJson: "{}",
        browserTextMetrics: { fontFamily: "Inter" },
      }),
    ).toBe(true);
  });

  it("rejects renderWithBrowserTextMetrics with non-graph format or bad metrics", () => {
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "renderWithBrowserTextMetrics",
        seq: 1,
        input: "x",
        format: "text",
        configJson: "{}",
        browserTextMetrics: { fontFamily: "Inter" },
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "renderWithBrowserTextMetrics",
        seq: 1,
        input: "x",
        format: "svg",
        configJson: "{}",
        browserTextMetrics: "not-an-object",
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "renderWithBrowserTextMetrics",
        seq: 1,
        input: "x",
        format: "svg",
        configJson: "{}",
        browserTextMetrics: { fontSizePx: "16" },
      }),
    ).toBe(false);
  });

  it("accepts renderWithBrowserTextMetrics with well-shaped textStyles", () => {
    expect(
      isWorkerRequestMessage({
        version: 1,
        type: "renderWithBrowserTextMetrics",
        seq: 1,
        input: "x",
        format: "svg",
        configJson: "{}",
        browserTextMetrics: {
          textStyles: [
            {
              id: "node-default",
              fontFamily: "Inter",
              fontSize: 16,
              fontSizePx: 16,
              lineHeight: 1.5,
              lineHeightPx: 24,
              fontStyle: "normal",
              fontWeight: "400",
              cssFont: "400 16px Inter",
            },
          ],
        },
      }),
    ).toBe(true);
  });

  it("rejects textStyles entries with wrong field shapes", () => {
    const base = {
      version: 1,
      type: "renderWithBrowserTextMetrics" as const,
      seq: 1,
      input: "x",
      format: "svg" as const,
      configJson: "{}",
    };
    expect(
      isWorkerRequestMessage({
        ...base,
        browserTextMetrics: { textStyles: "not-an-array" },
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        ...base,
        browserTextMetrics: { textStyles: [null] },
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        ...base,
        browserTextMetrics: {
          textStyles: [{ fontFamily: "Inter" }], // missing id
        },
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        ...base,
        browserTextMetrics: {
          textStyles: [{ id: "x" }], // missing fontFamily
        },
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        ...base,
        browserTextMetrics: {
          textStyles: [{ id: "x", fontFamily: "Inter", fontSize: "16" }],
        },
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        ...base,
        browserTextMetrics: {
          textStyles: [{ id: "x", fontFamily: "Inter", fontWeight: 700 }],
        },
      }),
    ).toBe(false);
  });

  it("rejects unversioned, wrong-version, or wrong-type messages", () => {
    expect(
      isWorkerRequestMessage({
        type: "render",
        seq: 1,
        input: "x",
        format: "text",
        configJson: "{}",
      }),
    ).toBe(false);
    expect(
      isWorkerRequestMessage({
        version: 2,
        type: "render",
        seq: 1,
        input: "x",
        format: "text",
        configJson: "{}",
      }),
    ).toBe(false);
    expect(isWorkerRequestMessage(null)).toBe(false);
    expect(isWorkerRequestMessage(undefined)).toBe(false);
    expect(isWorkerRequestMessage("render")).toBe(false);
    expect(isWorkerRequestMessage({ version: 1, type: "wat", seq: 1 })).toBe(
      false,
    );
  });

  describe("isWorkerBrowserTextMetricsDecision", () => {
    it("accepts the minimal { required: false } shape", () => {
      expect(isWorkerBrowserTextMetricsDecision({ required: false })).toBe(
        true,
      );
    });
    it("accepts a full decision with browserTextMetrics", () => {
      expect(
        isWorkerBrowserTextMetricsDecision({
          required: true,
          browserTextMetrics: {
            fontFamily: "Inter",
            fontSizePx: 16,
            lineHeightPx: 24,
          },
        }),
      ).toBe(true);
    });
    it("rejects non-objects, missing required, wrong types", () => {
      expect(isWorkerBrowserTextMetricsDecision(null)).toBe(false);
      expect(isWorkerBrowserTextMetricsDecision("required")).toBe(false);
      expect(isWorkerBrowserTextMetricsDecision({})).toBe(false);
      expect(isWorkerBrowserTextMetricsDecision({ required: "yes" })).toBe(
        false,
      );
      expect(
        isWorkerBrowserTextMetricsDecision({
          required: true,
          browserTextMetrics: "not-an-object",
        }),
      ).toBe(false);
      expect(
        isWorkerBrowserTextMetricsDecision({
          required: true,
          browserTextMetrics: { fontSizePx: "16" },
        }),
      ).toBe(false);
    });
  });

  it("response messages also carry version: 1", () => {
    const result: WorkerResponseMessage = {
      version: PROTOCOL_VERSION,
      type: "result",
      seq: 1,
      format: "svg",
      output: "<svg/>",
    };
    expect(result.version).toBe(1);
  });
});
