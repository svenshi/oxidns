/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CacheEntriesRequests,
  cacheSelectionReducer,
  mergeCacheEntries,
} from "./cache-entries";
import { deleteCacheEntries, fetchCacheEntries } from "./oxidns-api";
import { useAuthStore } from "./auth-store";

afterEach(() => vi.unstubAllGlobals());
describe("cache management", () => {
  it("keeps selected keys independent and clears scope on refresh/filter changes", () => {
    let state = cacheSelectionReducer(
      { ids: new Set<string>(), allMatching: false },
      { type: "loaded", ids: ["a", "b"], checked: true },
    );
    state = cacheSelectionReducer(state, {
      type: "toggle",
      id: "a",
      checked: false,
    });
    expect([...state.ids]).toEqual(["b"]);
    state = cacheSelectionReducer(state, { type: "allMatching" });
    expect(state.allMatching).toBe(true);
    expect(
      cacheSelectionReducer(state, { type: "toggle", id: "b", checked: false }),
    ).toBe(state);
    state = cacheSelectionReducer(state, { type: "clear" });
    expect(state.allMatching).toBe(false);
    expect(state.ids.size).toBe(0);
  });
  it("deduplicates moving pages without losing refreshed row data", () => {
    expect(
      mergeCacheEntries(
        [{ id: "a", ttl: 10 }],
        [
          { id: "a", ttl: 9 },
          { id: "b", ttl: 20 },
        ],
      ),
    ).toEqual([
      { id: "a", ttl: 9 },
      { id: "b", ttl: 20 },
    ]);
  });
  it("invalidates earlier requests and cancels on unmount", () => {
    const requests = new CacheEntriesRequests();
    const first = requests.start();
    const second = requests.start();
    expect(first.signal.aborted).toBe(true);
    expect(second.signal.aborted).toBe(false);
    requests.cancel();
    expect(second.signal.aborted).toBe(true);
  });
  it("passes filter and cursor together and forwards cancellation", async () => {
    useAuthStore.setState((state) => ({
      serverConfig: { ...state.serverConfig, url: "/api" },
    }));
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        new Response(
          JSON.stringify({ ok: true, entries: [], total_entries: 0 }),
        ),
      );
    vi.stubGlobal("fetch", fetchMock);
    const signal = new AbortController().signal;
    await fetchCacheEntries("cache main", {
      qname: "example",
      answer_ip: "2001:db8::1",
      cursor: "100",
      signal,
    });
    const [url, options] = fetchMock.mock.calls[0];
    const parsed = new URL(url, "http://localhost");
    expect(parsed.pathname).toBe("/api/plugins/cache%20main/entries");
    expect(parsed.searchParams.get("answer_ip")).toBe("2001:db8::1");
    expect(parsed.searchParams.get("cursor")).toBe("100");
    expect(options.signal).toBe(signal);
  });
  it("sends explicit selection and reports rejection rather than success", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ok: true, deleted_entries: 2 })),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            code: "invalid_delete_request",
            message: "Invalid selection",
          }),
          { status: 400 },
        ),
      );
    vi.stubGlobal("fetch", fetchMock);
    expect(
      await deleteCacheEntries("cache", { filter: { answer_ip: "192.0.2.1" } }),
    ).toEqual({ ok: true, deleted_entries: 2 });
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({
      filter: { answer_ip: "192.0.2.1" },
    });
    await expect(
      deleteCacheEntries("cache", { ids: ["bad"] }),
    ).rejects.toThrow();
  });
});
