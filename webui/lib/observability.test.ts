import { describe, expect, it } from "vitest";
import type { DNSObservation, DNSObservationEvent } from "./oxidns-api";
import { selectedUpstream } from "./observability";

function event(
  id: number,
  kind: string,
  parent_id: number | null,
  selected: boolean,
): DNSObservationEvent {
  return {
    id,
    kind,
    parent_id,
    selected,
    plugin: "test",
    target: `${id}`,
    method: "tls",
    status: "success",
    started_ms: 0,
    elapsed_ms: 1,
    latency_ms: null,
    sampled_at_ms: null,
    reason: null,
    rcode: "No Error",
    answer_count: 1,
  };
}

function query(events: DNSObservationEvent[]): DNSObservation {
  return {
    version: 1,
    id: "test",
    record_id: 1,
    created_at_ms: 1,
    client_ip: "127.0.0.1",
    qname: "example.test.",
    qtype: "A",
    status: "success",
    elapsed_ms: 2,
    rcode: "No Error",
    answers: [],
    events,
    truncated: false,
  };
}

describe("selectedUpstream", () => {
  it("does not report a forward winner from a discarded fallback branch", () => {
    const trace = query([
      event(0, "fallback", null, false),
      event(1, "fallback", null, true),
      event(2, "upstream", 0, true),
      event(3, "upstream", 1, true),
      event(4, "upstream", 0, true),
    ]);
    expect(selectedUpstream(trace)?.id).toBe(3);
  });

  it("uses the last selected forward for a direct execution path", () => {
    expect(
      selectedUpstream(
        query([
          event(0, "upstream", null, true),
          event(1, "upstream", null, true),
        ]),
      )?.id,
    ).toBe(1);
  });

  it("does not invent an upstream for cache-only, failed, or incomplete paths", () => {
    expect(
      selectedUpstream(query([event(0, "cache", null, true)])),
    ).toBeUndefined();
    const failed = query([event(0, "upstream", null, true)]);
    failed.status = "error";
    expect(selectedUpstream(failed)).toBeUndefined();
    expect(
      selectedUpstream(query([event(0, "upstream", 9, true)])),
    ).toBeUndefined();
    expect(
      selectedUpstream(query([event(0, "upstream", 0, true)])),
    ).toBeUndefined();
  });
});
