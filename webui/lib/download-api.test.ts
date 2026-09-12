import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchDownloads, runDownload } from "./oxidns-api";

const fetchMock = vi.fn();
vi.stubGlobal("fetch", fetchMock);
afterEach(() => fetchMock.mockReset());

describe("manual download API", () => {
  it("loads the live list without caching and supports cancellation", async () => {
    const payload = {
      ok: true,
      running: false,
      downloads: [
        { index: 0, url: "https://example.com/rules", path: "rules.txt" },
      ],
    };
    fetchMock.mockResolvedValue(new Response(JSON.stringify(payload)));
    const signal = new AbortController().signal;
    expect(await fetchDownloads("rules download", signal)).toEqual(payload);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/plugins/rules%20download/downloads",
      expect.objectContaining({ cache: "no-store", signal }),
    );
  });

  it("preserves index zero when downloading the first item", async () => {
    fetchMock.mockResolvedValue(
      new Response(
        JSON.stringify({ ok: true, total: 1, succeeded: 1, failed: 0 }),
      ),
    );
    await runDownload("rules", 0);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/plugins/rules/download",
      expect.objectContaining({ method: "POST", body: '{"index":0}' }),
    );
  });

  it("downloads all with an empty object and preserves partial failures", async () => {
    const payload = { ok: false, total: 2, succeeded: 1, failed: 1 };
    fetchMock.mockResolvedValue(new Response(JSON.stringify(payload)));
    expect(await runDownload("rules")).toEqual(payload);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/plugins/rules/download",
      expect.objectContaining({ body: "{}" }),
    );
  });

  it("rejects busy and server errors", async () => {
    fetchMock.mockResolvedValueOnce(new Response("{}", { status: 409 }));
    await expect(runDownload("rules")).rejects.toThrow();
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ message: "missing plugin" }), {
        status: 404,
      }),
    );
    await expect(runDownload("rules")).rejects.toThrow("missing plugin");
  });
});
