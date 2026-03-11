import type { MmdsDocument } from "./types.js";

export function assertValidMmdsDocument(
  doc: unknown,
): asserts doc is MmdsDocument {
  if (!doc || typeof doc !== "object") {
    throw new Error("MMDS document must be an object");
  }

  const maybe = doc as Partial<MmdsDocument>;
  if (!Array.isArray(maybe.nodes)) {
    throw new Error("MMDS document must include a nodes array");
  }
  if (!Array.isArray(maybe.edges)) {
    throw new Error("MMDS document must include an edges array");
  }
}
