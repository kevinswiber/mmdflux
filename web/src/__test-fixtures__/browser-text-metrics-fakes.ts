import { vi } from "vitest";
import type {
  BrowserTextMetricsEnvironment,
  MainThreadBrowserTextMetricsEnvironment,
} from "../browser-text-metrics";

export interface FakeTextMetrics {
  width: number;
}

export interface FakeCanvasContext {
  font: string;
  measureText: (text: string) => FakeTextMetrics;
}

export interface FakeFontFaceSet {
  load: (cssFont: string) => Promise<unknown[]>;
  ready: Promise<unknown>;
  check: (cssFont: string) => boolean;
}

export function fontSetFixture(
  overrides: Partial<FakeFontFaceSet> = {},
): FakeFontFaceSet {
  return {
    load: vi.fn(async () => [{}]),
    ready: Promise.resolve(),
    check: vi.fn(() => true),
    ...overrides,
  };
}

export function environmentFixture(
  fonts: FakeFontFaceSet | undefined = fontSetFixture(),
) {
  const context: FakeCanvasContext = {
    font: "",
    measureText: vi.fn((text: string) => ({ width: text.length * 3 })),
  };
  class FakeOffscreenCanvas {
    getContext(type: string): FakeCanvasContext | null {
      return type === "2d" ? context : null;
    }
  }

  const environment: BrowserTextMetricsEnvironment = {
    OffscreenCanvas: FakeOffscreenCanvas,
    fonts,
  };

  return {
    context,
    environment,
  };
}

export function mainThreadEnvironmentFixture(
  fonts: FakeFontFaceSet | undefined = fontSetFixture(),
) {
  const context: FakeCanvasContext = {
    font: "",
    measureText: vi.fn((text: string) => ({ width: text.length * 3 })),
  };
  const canvas = {
    getContext: (type: "2d"): FakeCanvasContext | null =>
      type === "2d" ? context : null,
  };
  const document = {
    fonts,
    createElement: vi.fn(function (
      this: unknown,
      tagName: "canvas",
    ): typeof canvas {
      if (this !== document) {
        throw new Error("createElement lost document receiver");
      }
      if (tagName !== "canvas") {
        throw new Error(`unexpected element: ${tagName}`);
      }
      return canvas;
    }),
  };
  const environment: MainThreadBrowserTextMetricsEnvironment = {
    document,
  };

  return {
    context,
    environment,
  };
}

export function multiStyleRequest() {
  return {
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
      {
        id: "s1",
        fontFamily: "Verdana",
        fontSize: 8,
        lineHeight: 12,
        fontStyle: "normal",
        fontWeight: "400",
      },
    ],
  };
}
