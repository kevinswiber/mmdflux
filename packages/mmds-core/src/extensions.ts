import type {
  MmdsDocument,
  MmdsNodeStyleExtension,
  MmdsTextMeasurementsExtension,
  MmdsTextMetricsExtension,
} from "./types.js";

export const MMDS_NODE_STYLE_NAMESPACE = "org.mmdflux.node-style.v1" as const;
/** Profile negotiation token for `MmdsDocument.profiles`, not a style entry ID. */
export const MMDS_NODE_STYLE_PROFILE = "mmdflux-node-style-v1" as const;
export const MMDS_TEXT_METRICS_NAMESPACE =
  "org.mmdflux.text-metrics.v1" as const;
/** Profile negotiation token for `MmdsDocument.profiles`, not `metricsProfile.id`. */
export const MMDS_TEXT_METRICS_PROFILE = "mmdflux-text-metrics-v1" as const;
export const MMDS_TEXT_MEASUREMENTS_NAMESPACE =
  "org.mmdflux.text-measurements.v1" as const;
/** Profile negotiation token for `MmdsDocument.profiles`, not `profileRef.id`. */
export const MMDS_TEXT_MEASUREMENTS_PROFILE =
  "mmdflux-text-measurements-v1" as const;

/** Read an extension namespace with a caller-supplied type and no runtime validation. */
export function getExtension<T>(
  doc: Pick<MmdsDocument, "extensions">,
  namespace: string,
): T | undefined {
  return doc.extensions?.[namespace] as T | undefined;
}

export function getNodeStyleExtension(
  doc: Pick<MmdsDocument, "extensions">,
): MmdsNodeStyleExtension | undefined {
  return getExtension<MmdsNodeStyleExtension>(doc, MMDS_NODE_STYLE_NAMESPACE);
}

export function getTextMetricsExtension(
  doc: Pick<MmdsDocument, "extensions">,
): MmdsTextMetricsExtension | undefined {
  return getExtension<MmdsTextMetricsExtension>(
    doc,
    MMDS_TEXT_METRICS_NAMESPACE,
  );
}

export function getTextMeasurementsExtension(
  doc: Pick<MmdsDocument, "extensions">,
): MmdsTextMeasurementsExtension | undefined {
  return getExtension<MmdsTextMeasurementsExtension>(
    doc,
    MMDS_TEXT_MEASUREMENTS_NAMESPACE,
  );
}
