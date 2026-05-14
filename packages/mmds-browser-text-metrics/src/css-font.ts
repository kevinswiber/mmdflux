export interface CssFontInput {
  fontFamily: string;
  fontSizePx: number;
  lineHeightPx: number;
  fontStyle?: string;
  fontWeight?: string;
}

export function buildCssFont(input: CssFontInput): string {
  const style = fontStyleFor(input);
  const weight = fontWeightFor(input);
  const family = cssFontFamilyStack(input.fontFamily);
  validatePositiveFinite("fontSizePx", input.fontSizePx);
  validatePositiveFinite("lineHeightPx", input.lineHeightPx);
  return `${style} ${weight} ${input.fontSizePx}px ${family}`;
}

export function cssFontFamilyStack(fontFamily: string): string {
  return normalizeNonEmpty("fontFamily", fontFamily)
    .split(",")
    .map((family) => familyTokenToCss(family))
    .join(", ");
}

function fontStyleFor(input: Pick<CssFontInput, "fontStyle">): string {
  return input.fontStyle?.trim() || "normal";
}

function fontWeightFor(input: Pick<CssFontInput, "fontWeight">): string {
  return input.fontWeight?.trim() || "400";
}

function normalizeNonEmpty(field: string, value: string): string {
  const normalized = value.trim();
  if (!normalized) {
    throw new Error(`${field} must not be empty.`);
  }
  return normalized;
}

function familyTokenToCss(family: string): string {
  const unquoted = stripOneQuoteLayer(family.trim());
  if (!unquoted) {
    throw new Error("fontFamily must not contain empty family names.");
  }
  if (isGenericFamily(unquoted)) {
    return unquoted.toLowerCase();
  }
  const escaped = unquoted.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return `"${escaped}"`;
}

function stripOneQuoteLayer(value: string): string {
  if (
    value.length >= 2 &&
    ((value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'")))
  ) {
    return value.slice(1, -1).trim();
  }
  return value;
}

function isGenericFamily(family: string): boolean {
  switch (family.toLowerCase()) {
    case "serif":
    case "sans-serif":
    case "monospace":
    case "cursive":
    case "fantasy":
    case "system-ui":
    case "ui-serif":
    case "ui-sans-serif":
    case "ui-monospace":
    case "ui-rounded":
    case "emoji":
    case "math":
    case "fangsong":
      return true;
    default:
      return false;
  }
}

function validatePositiveFinite(field: string, value: number): void {
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error(`${field} must be a finite positive number.`);
  }
}
