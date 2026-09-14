import { describe, expect, it } from "vitest";

import {
  pluginCardGridColumnClass,
  pluginCardItemColumnClass,
} from "./plugin-card-item-grid";

describe("plugin card item grid", () => {
  it.each([1, 2, 3])("uses one column for %i items", (itemCount) => {
    expect(pluginCardGridColumnClass(itemCount)).toBe("grid-cols-1");
  });

  it.each([4, 5, 6])("uses two columns for %i items", (itemCount) => {
    expect(pluginCardGridColumnClass(itemCount)).toBe("grid-cols-2");
  });

  it("reserves readable label width for configuration summaries", () => {
    expect(pluginCardItemColumnClass(false)).toBe(
      "grid-cols-[minmax(5.25rem,52%)_minmax(0,1fr)]",
    );
    expect(pluginCardItemColumnClass(true)).toBe(
      "grid-cols-[minmax(0,1fr)_auto]",
    );
  });
});
