import { afterEach, describe, expect, it, vi } from "vitest";

import { useAuthStore } from "./auth-store";
import { cronManualRunRuntimeTag } from "./cron-manual-run";
import {
  CronJobAlreadyRunningError,
  CronJobUnavailableError,
  runCronJob,
} from "./oxidns-api";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("cron manual run", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("exposes the runtime action only for an applied, read-only plugin", () => {
    expect(
      cronManualRunRuntimeTag(
        false,
        "applied",
        "cron_main",
        "version-1",
        "version-1",
      ),
    ).toBe("cron_main");
    expect(
      cronManualRunRuntimeTag(
        true,
        "applied",
        "cron_main",
        "version-1",
        "version-1",
      ),
    ).toBeUndefined();
    expect(
      cronManualRunRuntimeTag(
        false,
        "not-applied",
        "cron_main",
        "version-1",
        "version-1",
      ),
    ).toBeUndefined();
    expect(
      cronManualRunRuntimeTag(
        false,
        "unknown",
        "cron_main",
        "version-1",
        "version-1",
      ),
    ).toBeUndefined();
  });

  it("hides the runtime action until the displayed config is running", () => {
    expect(
      cronManualRunRuntimeTag(
        false,
        "applied",
        "cron_main",
        "saved-version",
        "running-version",
      ),
    ).toBeUndefined();
    expect(
      cronManualRunRuntimeTag(
        false,
        "applied",
        "cron_main",
        null,
        null,
      ),
    ).toBeUndefined();
  });

  it("encodes plugin and job names and returns the accepted response", async () => {
    useAuthStore.setState((state) => ({
      serverConfig: { ...state.serverConfig, url: "/api" },
    }));
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse(202, {
        ok: true,
        job: "refresh sets/a+b",
        status: "started",
        trigger: "manual",
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      runCronJob("cron main", "refresh sets/a+b"),
    ).resolves.toMatchObject({ status: "started", trigger: "manual" });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/plugins/cron%20main/jobs/refresh%20sets%2Fa%2Bb/run",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("maps busy and unavailable responses to distinct UI errors", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse(409, {
          code: "cron_job_already_running",
          message: "busy",
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse(503, {
          code: "cron_scheduler_unavailable",
          message: "unavailable",
        }),
      );
    vi.stubGlobal("fetch", fetchMock);

    await expect(runCronJob("cron", "job")).rejects.toBeInstanceOf(
      CronJobAlreadyRunningError,
    );
    await expect(runCronJob("cron", "job")).rejects.toBeInstanceOf(
      CronJobUnavailableError,
    );
  });
});
