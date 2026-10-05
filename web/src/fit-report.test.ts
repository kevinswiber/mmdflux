import { describe, expect, it } from "vitest";
import { describeFitReport, type FitReport, fitJsonFor } from "./fit-report";
import { DEFAULT_SHARE_RENDER_SETTINGS } from "./share";

function report(overrides: Partial<FitReport>): FitReport {
  return {
    outcome: "asAuthored",
    size: { width: 30, height: 12 },
    asAuthored: { width: 30, height: 12 },
    applied: [],
    ...overrides,
  };
}

describe("fitJsonFor", () => {
  it("returns null when no width budget is set", () => {
    expect(fitJsonFor(DEFAULT_SHARE_RENDER_SETTINGS)).toBeNull();
  });

  it("sends only the options that differ from the defaults", () => {
    expect(
      JSON.parse(
        fitJsonFor({ ...DEFAULT_SHARE_RENDER_SETTINGS, maxWidth: 40 }) ?? "",
      ),
    ).toEqual({ maxWidth: 40 });

    expect(
      JSON.parse(
        fitJsonFor({
          ...DEFAULT_SHARE_RENDER_SETTINGS,
          maxWidth: 40,
          fitDirection: "keep",
          fitTruncate: true,
        }) ?? "",
      ),
    ).toEqual({ maxWidth: 40, fitDirection: "keep", truncate: true });
  });
});

describe("describeFitReport", () => {
  it("says the as-authored drawing fits", () => {
    expect(describeFitReport(report({}), 80)).toBe(
      "Fits 80 columns as authored (30×12).",
    );
  });

  it("lists the levers a fitted drawing applied", () => {
    expect(
      describeFitReport(
        report({
          outcome: "fitted",
          size: { width: 38, height: 20 },
          asAuthored: { width: 62, height: 14 },
          applied: [
            { lever: "direction", from: "LR", to: "TD" },
            { lever: "labelAwareSpacing" },
            { lever: "gridGap", rankGap: 2, nodeGap: 1 },
            { lever: "edgeLabelWrap", cells: 12 },
          ],
        }),
        40,
      ),
    ).toBe(
      "Fitted to 40 columns: direction LR→TD, label-aware spacing, gaps rank 2 node 1, edge labels wrapped at 12; 38×20 (as authored 62×14).",
    );
  });

  it("names the narrowest drawing when nothing fits", () => {
    expect(
      describeFitReport(
        report({
          outcome: "bestAttempt",
          size: { width: 34, height: 30 },
          asAuthored: { width: 62, height: 14 },
          applied: [
            { lever: "nodeLabelWrap", cells: 8 },
            { lever: "labelTruncation", cells: 6 },
          ],
        }),
        20,
      ),
    ).toBe(
      "No layout fits 20 columns; showing the narrowest, 34×30 (node labels wrapped at 8, labels truncated to 6; as authored 62×14).",
    );
  });

  it("returns null when the fit was not applied", () => {
    expect(
      describeFitReport(
        report({ outcome: "notApplied", size: null, asAuthored: null }),
        40,
      ),
    ).toBeNull();
  });
});
