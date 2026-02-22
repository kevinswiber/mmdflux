import { describe, expect, it } from "vitest";
import { normalizeValidateResultToDiagnostics } from "./wasm-diagnostics";

describe("normalizeValidateResultToDiagnostics", () => {
  it("returns empty array for valid result", () => {
    const result = normalizeValidateResultToDiagnostics(
      "graph TD\nA-->B",
      '{"valid":true}',
    );
    expect(result).toEqual([]);
  });

  it("returns empty array when diagnostics array is empty", () => {
    const result = normalizeValidateResultToDiagnostics(
      "graph TD\nA-->B",
      '{"valid":false,"diagnostics":[]}',
    );
    expect(result).toEqual([]);
  });

  it("returns diagnostic with correct offsets for syntax error on line 2", () => {
    // Input: "graph TD\n!!!"
    //         0123456789...
    // Line 2 starts at offset 9, column 1 → offset 9
    const input = "graph TD\n!!!";
    const validateResult = JSON.stringify({
      valid: false,
      diagnostics: [
        {
          line: 2,
          column: 1,
          message: "expected node identifier",
        },
      ],
    });
    const result = normalizeValidateResultToDiagnostics(input, validateResult);
    expect(result).toHaveLength(1);
    expect(result[0].severity).toBe("error");
    expect(result[0].source).toBe("mmdflux");
    expect(result[0].from).toBe(9); // start of line 2
    expect(result[0].to).toBe(10); // default 1-char highlight
    expect(result[0].message).toBe("expected node identifier");
  });

  it("uses end_line and end_column for span range", () => {
    // Input: "graph TD\nABCDEFG"
    //         012345678 9...
    // Line 2, col 3 → offset 11 (9 + 2)
    // Line 2, col 6 → offset 14 (9 + 5)
    const input = "graph TD\nABCDEFG";
    const validateResult = JSON.stringify({
      valid: false,
      diagnostics: [
        {
          line: 2,
          column: 3,
          end_line: 2,
          end_column: 6,
          message: "unexpected token",
        },
      ],
    });
    const result = normalizeValidateResultToDiagnostics(input, validateResult);
    expect(result).toHaveLength(1);
    expect(input.slice(result[0].from, result[0].to)).toBe("CDE");
  });

  it("handles diagnostic without position info", () => {
    const input = "not a diagram";
    const validateResult = JSON.stringify({
      valid: false,
      diagnostics: [{ message: "unknown diagram type" }],
    });
    const result = normalizeValidateResultToDiagnostics(input, validateResult);
    expect(result).toHaveLength(1);
    expect(result[0].from).toBe(0);
    expect(result[0].to).toBe(1);
    expect(result[0].message).toBe("unknown diagram type");
  });

  it("handles diagnostic without position on empty input", () => {
    const result = normalizeValidateResultToDiagnostics(
      "",
      JSON.stringify({
        valid: false,
        diagnostics: [{ message: "unknown diagram type" }],
      }),
    );
    expect(result).toHaveLength(1);
    expect(result[0].from).toBe(0);
    expect(result[0].to).toBe(0);
  });

  it("clamps positions to input bounds", () => {
    const input = "graph TD\nA";
    const validateResult = JSON.stringify({
      valid: false,
      diagnostics: [
        {
          line: 99,
          column: 99,
          message: "out of bounds",
        },
      ],
    });
    const result = normalizeValidateResultToDiagnostics(input, validateResult);
    expect(result).toHaveLength(1);
    expect(result[0].from).toBeLessThanOrEqual(input.length);
    expect(result[0].to).toBeLessThanOrEqual(input.length);
  });

  it("defaults to error severity when severity field is absent", () => {
    const input = "graph TD\n!!!";
    const validateResult = JSON.stringify({
      valid: false,
      diagnostics: [{ line: 2, column: 1, message: "syntax error" }],
    });
    const result = normalizeValidateResultToDiagnostics(input, validateResult);
    expect(result).toHaveLength(1);
    expect(result[0].severity).toBe("error");
  });

  it("maps warning severity from validate result", () => {
    const input = "graph TD\nA --> B\nstyle A fill:#f9f";
    const validateResult = JSON.stringify({
      valid: true,
      diagnostics: [
        {
          severity: "warning",
          line: 3,
          column: 1,
          message: "style statements are parsed but ignored in rendering",
        },
      ],
    });
    const result = normalizeValidateResultToDiagnostics(input, validateResult);
    expect(result).toHaveLength(1);
    expect(result[0].severity).toBe("warning");
    expect(result[0].message).toBe(
      "style statements are parsed but ignored in rendering",
    );
  });

  it("maps error severity explicitly from validate result", () => {
    const input = "graph TD\n!!!";
    const validateResult = JSON.stringify({
      valid: false,
      diagnostics: [
        { severity: "error", line: 2, column: 1, message: "syntax error" },
      ],
    });
    const result = normalizeValidateResultToDiagnostics(input, validateResult);
    expect(result).toHaveLength(1);
    expect(result[0].severity).toBe("error");
  });
});
