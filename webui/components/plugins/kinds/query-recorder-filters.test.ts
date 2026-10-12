import { describe, expect, it } from "vitest";
import {
  recordTextFilterForm,
  recordTextFilters,
} from "./query-recorder-filters";

describe("record text matching modes", () => {
  it("restores exact drilldowns in the existing input fields", () => {
    expect(
      recordTextFilterForm({
        qnameExact: "example.com.",
        clientIpExact: "::1",
      }),
    ).toEqual({
      matcherTag: "",
      qname: "example.com.",
      qnameMatch: "exact",
      clientIp: "::1",
      clientIpMatch: "exact",
    });
    expect(recordTextFilterForm()).toEqual({
      matcherTag: "",
      qname: "",
      qnameMatch: "contains",
      clientIp: "",
      clientIpMatch: "contains",
    });
  });

  it("restores, edits, and clears an exact matcher filter without keeping the old tag", () => {
    const form = recordTextFilterForm({ matcherTag: "old_ads" });
    expect(form.matcherTag).toBe("old_ads");
    expect(
      recordTextFilters({ ...form, matcherTag: "  new_ads  " }).matcherTag,
    ).toBe("new_ads");
    expect(
      recordTextFilters({ ...form, matcherTag: "  " }).matcherTag,
    ).toBeUndefined();
  });

  it("switches modes without retaining a conflicting hidden filter", () => {
    const exact = recordTextFilterForm({
      qnameExact: "example.com.",
      clientIpExact: "192.0.2.1",
    });
    expect(
      recordTextFilters({
        ...exact,
        qnameMatch: "contains",
        clientIpMatch: "contains",
      }),
    ).toEqual({
      matcherTag: undefined,
      qname: "example.com.",
      clientIp: "192.0.2.1",
      qnameExact: undefined,
      clientIpExact: undefined,
    });
    expect(
      recordTextFilters({
        ...exact,
        matcherTag: "",
        qname: "other.example",
        clientIp: "192.0.2.2",
      }),
    ).toEqual({
      matcherTag: undefined,
      qname: undefined,
      clientIp: undefined,
      qnameExact: "other.example.",
      clientIpExact: "192.0.2.2",
    });
  });

  it("clears both matching modes when the inputs are empty", () => {
    expect(
      recordTextFilters({
        matcherTag: "",
        qname: "  ",
        qnameMatch: "exact",
        clientIp: "",
        clientIpMatch: "contains",
      }),
    ).toEqual({
      matcherTag: undefined,
      qname: undefined,
      qnameExact: undefined,
      clientIp: undefined,
      clientIpExact: undefined,
    });
  });

  it("accepts absolute domains, a missing final dot, and the root name", () => {
    for (const [value, expected] of [
      [" Example.COM ", "Example.COM."],
      ["example.com.", "example.com."],
      [".", "."],
    ]) {
      expect(
        recordTextFilters({
          matcherTag: "",
          qname: value,
          qnameMatch: "exact",
          clientIp: "",
          clientIpMatch: "contains",
        }).qnameExact,
      ).toBe(expected);
    }
  });
});
