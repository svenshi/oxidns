/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

import { describe, expect, it } from "vitest";

import cases from "../../tests/fixtures/plugin_tag_cases.json";

import {
  isReservedPluginTag,
  isValidPluginTag,
  validatePluginTag,
} from "./plugin-tags";

type PluginTagCase = {
  tag: string;
  error: ReturnType<typeof validatePluginTag>;
};

describe("plugin tag validation", () => {
  it.each(cases as PluginTagCase[])("matches the shared rule for $tag", (testCase) => {
    expect(validatePluginTag(testCase.tag)).toBe(testCase.error);
    expect(isValidPluginTag(testCase.tag)).toBe(testCase.error === null);
  });

  it("recognizes quick-setup reserved prefixes separately from syntax", () => {
    expect(validatePluginTag("qs.exec.seq.0.cache")).toBeNull();
    expect(isReservedPluginTag("qs.exec.seq.0.cache")).toBe(true);
    expect(isReservedPluginTag("QS.exec.seq.0.cache")).toBe(false);
  });
});
