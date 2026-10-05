import type { ShareRenderSettings } from "./share";

/** Wire shape of the fit report `renderFitted` returns (the fields the playground reads). */
export interface FitReport {
  outcome: "asAuthored" | "fitted" | "bestAttempt" | "notApplied";
  size: FitCellSize | null;
  asAuthored: FitCellSize | null;
  applied: FitLever[];
}

export interface FitCellSize {
  width: number;
  height: number;
}

export type FitLever =
  | { lever: "direction"; from: string; to: string }
  | { lever: "subgraphDirectionStrip" }
  | { lever: "labelAwareSpacing" }
  | { lever: "gridGap"; rankGap: number; nodeGap: number }
  | { lever: "edgeLabelWrap"; cells: number }
  | { lever: "nodeLabelWrap"; cells: number }
  | { lever: "memberWrap"; cells: number }
  | { lever: "labelTruncation"; cells: number }
  | { lever: "memberElision"; keep: number };

/** The `fitJson` for `renderFitted`, or `null` when no width budget is set. */
export function fitJsonFor(settings: ShareRenderSettings): string | null {
  if (settings.maxWidth === null) {
    return null;
  }

  const fit: Record<string, number | string | boolean> = {
    maxWidth: settings.maxWidth,
  };
  if (settings.fitDirection === "keep") {
    fit.fitDirection = "keep";
  }
  if (settings.fitTruncate) {
    fit.truncate = true;
  }
  return JSON.stringify(fit);
}

// Mirrors the CLI's lever wording (`impl Display for FitLever`).
function describeLever(lever: FitLever): string {
  switch (lever.lever) {
    case "direction":
      return `direction ${lever.from}→${lever.to}`;
    case "subgraphDirectionStrip":
      return "subgraph directions relaxed";
    case "labelAwareSpacing":
      return "label-aware spacing";
    case "gridGap":
      return `gaps rank ${lever.rankGap} node ${lever.nodeGap}`;
    case "edgeLabelWrap":
      return `edge labels wrapped at ${lever.cells}`;
    case "nodeLabelWrap":
      return `node labels wrapped at ${lever.cells}`;
    case "memberWrap":
      return `class members wrapped at ${lever.cells}`;
    case "labelTruncation":
      return `labels truncated to ${lever.cells}`;
    case "memberElision":
      return `class members elided to ${lever.keep}`;
    default:
      return (lever as { lever: string }).lever;
  }
}

function cells(size: FitCellSize): string {
  return `${size.width}×${size.height}`;
}

/** One line saying how the fitted drawing was chosen, or `null` when no fit ran. */
export function describeFitReport(
  report: FitReport,
  maxWidth: number,
): string | null {
  const { size, asAuthored } = report;
  if (report.outcome === "notApplied" || !size || !asAuthored) {
    return null;
  }

  const levers = report.applied.map(describeLever).join(", ");
  switch (report.outcome) {
    case "asAuthored":
      return `Fits ${maxWidth} columns as authored (${cells(size)}).`;
    case "fitted":
      return `Fitted to ${maxWidth} columns: ${levers}; ${cells(size)} (as authored ${cells(asAuthored)}).`;
    case "bestAttempt": {
      const detail = levers
        ? `${levers}; as authored ${cells(asAuthored)}`
        : "as authored";
      return `No layout fits ${maxWidth} columns; showing the narrowest, ${cells(size)} (${detail}).`;
    }
  }
}
