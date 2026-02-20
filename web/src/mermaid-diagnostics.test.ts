import { describe, expect, it } from "vitest";
import { normalizeMermaidErrorToDiagnostic } from "./mermaid-diagnostics";

describe("normalizeMermaidErrorToDiagnostic", () => {
  it("uses parser location ranges when hash.loc is present", () => {
    const input = ["graph TD", "ABCDEFG", "Z"].join("\n");
    const diagnostic = normalizeMermaidErrorToDiagnostic(input, {
      message: "Parse error on line 2",
      hash: {
        loc: {
          first_line: 2,
          first_column: 2,
          last_line: 2,
          last_column: 5,
        },
        text: "CDE",
      },
    });

    expect(input.slice(diagnostic.from, diagnostic.to)).toBe("CDE");
    expect(diagnostic.severity).toBe("error");
    expect(diagnostic.source).toBe("mermaid");
  });

  it("falls back to hash.line when structured location is absent", () => {
    const input = ["graph TD", "Alpha --> Beta", "Gamma"].join("\n");
    const diagnostic = normalizeMermaidErrorToDiagnostic(input, {
      message: "Parse error on line 2",
      hash: {
        line: 1,
        text: "Alpha",
      },
    });

    expect(input.slice(diagnostic.from, diagnostic.to)).toBe("Alpha");
  });

  it("normalizes multiline parser errors to a concise message", () => {
    const input = ["graph TD", "A -? B"].join("\n");
    const diagnostic = normalizeMermaidErrorToDiagnostic(input, {
      str: "Parse error on line 2:\nA -? B\n--^\nExpecting 'LINK', got '?'",
      hash: {
        line: 1,
        text: "?",
      },
    });

    expect(diagnostic.message).toContain("Parse error on line 2");
    expect(diagnostic.message).toContain("Expecting 'LINK', got '?'");
    expect(diagnostic.message).not.toContain("--^");
  });
});
