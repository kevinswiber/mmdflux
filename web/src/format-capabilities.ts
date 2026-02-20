export type PlaygroundFormat = "text" | "svg" | "mmds";

export type RenderControlId =
  | "layoutEngine"
  | "edgePreset"
  | "geometryLevel"
  | "pathDetail";

interface RenderControlCapability {
  description: string;
  supportedFormats: readonly PlaygroundFormat[];
  reasonWhenDisabled: string;
}

const CONTROL_CAPABILITIES: Record<RenderControlId, RenderControlCapability> = {
  layoutEngine: {
    description: "Select which layout algorithm is used during render.",
    supportedFormats: ["text", "svg", "mmds"],
    reasonWhenDisabled: "",
  },
  edgePreset: {
    description: "Pick the SVG edge style preset for routed curves.",
    supportedFormats: ["svg"],
    reasonWhenDisabled: "Edge preset applies to SVG output only.",
  },
  geometryLevel: {
    description:
      "Choose whether MMDS emits layout-only or fully routed geometry.",
    supportedFormats: ["mmds"],
    reasonWhenDisabled: "Geometry level applies to MMDS output only.",
  },
  pathDetail: {
    description: "Control how much path waypoint detail is included.",
    supportedFormats: ["svg", "mmds"],
    reasonWhenDisabled: "Path detail applies to SVG and MMDS output.",
  },
};

export function isSupported(
  format: PlaygroundFormat,
  control: RenderControlId,
): boolean {
  return CONTROL_CAPABILITIES[control].supportedFormats.includes(format);
}

export function disabledReason(
  format: PlaygroundFormat,
  control: RenderControlId,
): string | null {
  if (isSupported(format, control)) {
    return null;
  }

  return CONTROL_CAPABILITIES[control].reasonWhenDisabled;
}

export function helpText(
  format: PlaygroundFormat,
  control: RenderControlId,
): string {
  const capability = CONTROL_CAPABILITIES[control];
  const reason = disabledReason(format, control);

  return reason
    ? `${capability.description} ${reason}`
    : capability.description;
}
