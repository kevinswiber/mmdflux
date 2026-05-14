import { describe, expect, it } from "vitest";
import { PLAYGROUND_EXAMPLES } from "./examples";

describe("PLAYGROUND_EXAMPLES", () => {
  it("includes a multi-font flowchart example", () => {
    const example = PLAYGROUND_EXAMPLES.find(
      (candidate) => candidate.id === "flowchart-dynamic-fonts",
    );

    expect(example?.input).toContain(
      "style A font-family:Verdana,font-size:8px",
    );
    expect(example?.input).toContain(
      "style B font-family:Courier New,font-size:20px",
    );
    expect(example?.input).toContain(
      "linkStyle 0 font-family:Times New Roman,font-size:32px",
    );
    expect(example?.input).toContain(
      "classDef groupTitle font-family:Georgia,font-size:24px,font-style:italic,color:#24476b",
    );
    expect(example?.input).toContain("class G groupTitle");
  });
});
