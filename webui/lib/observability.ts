import type { DNSObservation, DNSObservationEvent } from "./oxidns-api";

/** Only a winner on the selected fallback path can describe the returned query. */
export function selectedUpstream(
  query: DNSObservation,
): DNSObservationEvent | undefined {
  if (query.status === "error" || query.status === "no_response")
    return undefined;
  const byId = new Map(query.events.map((event) => [event.id, event]));
  for (const event of [...query.events].reverse()) {
    if (event.kind !== "upstream" || !event.selected) continue;
    let parentId = event.parent_id;
    let remaining = query.events.length;
    while (parentId !== null && remaining > 0) {
      remaining -= 1;
      const parent = byId.get(parentId);
      if (!parent || (parent.kind === "fallback" && !parent.selected)) break;
      parentId = parent.parent_id;
    }
    if (parentId === null) return event;
  }
  return undefined;
}
