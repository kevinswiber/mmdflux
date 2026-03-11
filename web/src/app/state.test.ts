import { describe, expect, it } from "vitest";

import { createPlaygroundStateStore } from "./state";

describe("createPlaygroundStateStore", () => {
  it("handles format transitions in the state layer", () => {
    const store = createPlaygroundStateStore();

    store.selectFormat("svg");

    expect(store.getState().format).toBe("svg");
  });
});
