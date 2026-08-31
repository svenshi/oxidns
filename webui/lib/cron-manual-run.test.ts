import { afterEach, describe, expect, it, vi } from "vitest";

import { useAuthStore } from "./auth-store";
import {
  CronStatusRequestCoordinator,
  acceptCronManualRun,
  beginCronManualRun,
  clearCronManualRunViewsAfterStatusFailure,
  cronConfigValuesForDisplay,
  cronManualRunRuntimeTag,
  cronRunButtonPhase,
  expireCronManualRunSuccess,
  hasCronManualRunLocalState,
  initializeCronManualRunViews,
  markCronManualRunStartUncertain,
  reconcileCronManualRunViews,
  setCronManualRunStatusBaseline,
} from "./cron-manual-run";
import {
  CronJobAlreadyRunningError,
  CronJobNotFoundError,
  CronJobUnavailableError,
  fetchCronJobStatuses,
  runCronJob,
  type CronJobRunSnapshot,
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
      cronManualRunRuntimeTag(false, "applied", "cron_main", null, null),
    ).toBeUndefined();
  });

  it("displays refreshed plugin config without replacing an active draft", () => {
    const staleDraft = { jobs: [{ name: "old-job" }] };
    const refreshedConfig = { jobs: [{ name: "new-job" }] };

    expect(cronConfigValuesForDisplay(false, staleDraft, refreshedConfig)).toBe(
      refreshedConfig,
    );
    expect(cronConfigValuesForDisplay(true, staleDraft, refreshedConfig)).toBe(
      staleDraft,
    );
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
        run_id: 7,
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      runCronJob("cron main", "refresh sets/a+b"),
    ).resolves.toMatchObject({
      status: "started",
      trigger: "manual",
      run_id: 7,
    });
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
        jsonResponse(404, {
          code: "cron_job_not_found",
          message: "missing",
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
      CronJobNotFoundError,
    );
    await expect(runCronJob("cron", "job")).rejects.toBeInstanceOf(
      CronJobUnavailableError,
    );
  });

  it("fetches all job statuses with an abortable encoded plugin request", async () => {
    const controller = new AbortController();
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse(200, {
        ok: true,
        jobs: { job: { current_run: null, last_manual_run: null } },
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      fetchCronJobStatuses("cron main", controller.signal),
    ).resolves.toMatchObject({ ok: true });
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/plugins/cron%20main/jobs/status",
      expect.objectContaining({ method: "GET", signal: controller.signal }),
    );
  });

  it("keeps loading until the accepted run reaches a real terminal result", () => {
    let view = beginCronManualRun(undefined);
    expect(cronRunButtonPhase(view)).toBe("starting");
    view = acceptCronManualRun(view, 4);

    const pending = reconcileCronManualRunViews({ job: view }, ["job"], {
      job: runSnapshot(4, "pending"),
    });
    expect(cronRunButtonPhase(pending.views.job)).toBe("pending");
    expect(pending.effects).toEqual([]);

    const running = reconcileCronManualRunViews(pending.views, ["job"], {
      job: runSnapshot(4, "running"),
    });
    expect(cronRunButtonPhase(running.views.job)).toBe("running");

    const completed = reconcileCronManualRunViews(running.views, ["job"], {
      job: completedSnapshot(4, "completed"),
    });
    expect(cronRunButtonPhase(completed.views.job)).toBe("success");
    expect(completed.effects).toEqual([]);
  });

  it("preserves accepted run IDs while status synchronization is unavailable", () => {
    const acceptedSuccess = acceptCronManualRun(
      beginCronManualRun(undefined),
      10,
    );
    const runningSuccess = reconcileCronManualRunViews(
      { success: acceptedSuccess },
      ["success"],
      { success: runSnapshot(10, "running") },
    ).views.success;
    expect(cronRunButtonPhase(runningSuccess)).toBe("running");

    const failedViews = clearCronManualRunViewsAfterStatusFailure(
      {
        success: runningSuccess,
        failure: acceptCronManualRun(beginCronManualRun(undefined), 11),
      },
      ["success", "failure", "idle"],
    );

    expect(cronRunButtonPhase(failedViews.success)).toBe("starting");
    expect(cronRunButtonPhase(failedViews.failure)).toBe("starting");
    expect(cronRunButtonPhase(failedViews.idle)).toBe("idle");
    expect(failedViews.success.trackedManualRunId).toBe(10);
    expect(failedViews.failure.trackedManualRunId).toBe(11);

    const recovered = reconcileCronManualRunViews(
      failedViews,
      ["success", "failure", "idle"],
      {
        success: completedSnapshot(10, "completed"),
        failure: completedSnapshot(11, "failed"),
      },
    );
    expect(cronRunButtonPhase(recovered.views.success)).toBe("success");
    expect(recovered.effects).toEqual([
      { jobName: "failure", type: "failed", executorErrorCount: 0 },
    ]);
  });

  it("allows only the newest status request to update run state", () => {
    const coordinator = new CronStatusRequestCoordinator();
    const older = coordinator.begin();
    const newer = coordinator.begin();

    expect(older.signal.aborted).toBe(true);
    expect(older.isCurrent()).toBe(false);
    expect(newer.signal.aborted).toBe(false);
    expect(newer.isCurrent()).toBe(true);

    coordinator.invalidate();
    expect(newer.signal.aborted).toBe(true);
    expect(newer.isCurrent()).toBe(false);
  });

  it("maps partial, failed, cancelled, and lost runs to one-shot effects", () => {
    for (const status of [
      "completed_with_errors",
      "failed",
      "cancelled",
    ] as const) {
      const accepted = acceptCronManualRun(beginCronManualRun(undefined), 8);
      const result = reconcileCronManualRunViews({ job: accepted }, ["job"], {
        job: completedSnapshot(8, status, 2),
      });
      expect(result.effects).toEqual([
        { jobName: "job", type: status, executorErrorCount: 2 },
      ]);
      const replay = reconcileCronManualRunViews(result.views, ["job"], {
        job: completedSnapshot(8, status, 2),
      });
      expect(replay.effects).toEqual([]);
    }

    const lost = reconcileCronManualRunViews(
      { job: acceptCronManualRun(beginCronManualRun(undefined), 9) },
      ["job"],
      { job: { current_run: null, last_manual_run: null } },
    );
    expect(lost.effects).toEqual([
      { jobName: "job", type: "lost", executorErrorCount: 0 },
    ]);
  });

  it("reconciles an ambiguous POST failure against the previous manual result", () => {
    const baseline = initializeCronManualRunViews(["job"], {
      job: completedSnapshot(3, "completed"),
    });
    const posting = beginCronManualRun(baseline.job);
    const observedWhilePosting = reconcileCronManualRunViews(
      { job: posting },
      ["job"],
      { job: completedSnapshot(4, "completed") },
    ).views.job;
    expect(observedWhilePosting.lastObservedManualRunId).toBe(4);
    expect(observedWhilePosting.manualStartBaselineRunId).toBe(3);
    const uncertain = markCronManualRunStartUncertain(observedWhilePosting);

    const completed = reconcileCronManualRunViews(
      { job: uncertain },
      ["job"],
      { job: completedSnapshot(4, "completed") },
    );
    expect(cronRunButtonPhase(completed.views.job)).toBe("success");
    expect(completed.effects).toEqual([]);

    const notStarted = markCronManualRunStartUncertain(
      beginCronManualRun(baseline.job),
    );
    const firstMiss = reconcileCronManualRunViews(
      { job: notStarted },
      ["job"],
      { job: completedSnapshot(3, "completed") },
    );
    expect(cronRunButtonPhase(firstMiss.views.job)).toBe("starting");
    expect(firstMiss.effects).toEqual([]);

    const confirmedFailure = reconcileCronManualRunViews(
      firstMiss.views,
      ["job"],
      { job: completedSnapshot(3, "completed") },
    );
    expect(cronRunButtonPhase(confirmedFailure.views.job)).toBe("idle");
    expect(confirmedFailure.effects).toEqual([
      { jobName: "job", type: "start_failed", executorErrorCount: 0 },
    ]);
  });

  it("refreshes a stale manual-result baseline before starting a new run", () => {
    const stale = initializeCronManualRunViews(["job"], {
      job: completedSnapshot(3, "completed"),
    });
    const posting = beginCronManualRun(stale.job);
    const refreshed = setCronManualRunStatusBaseline(
      posting,
      completedSnapshot(4, "completed"),
    );

    expect(refreshed.lastObservedManualRunId).toBe(4);
    expect(refreshed.manualStartBaselineRunId).toBe(4);

    const uncertain = markCronManualRunStartUncertain(refreshed);
    const completed = reconcileCronManualRunViews(
      { job: uncertain },
      ["job"],
      { job: completedSnapshot(5, "completed") },
    );
    expect(cronRunButtonPhase(completed.views.job)).toBe("success");
    expect(completed.effects).toEqual([]);
  });

  it("stores __proto__ as an own job key through completion and expiry", () => {
    const jobName = "__proto__";
    const previous = Object.fromEntries([
      [jobName, acceptCronManualRun(beginCronManualRun(undefined), 7)],
    ]);
    const snapshots = Object.fromEntries([
      [jobName, completedSnapshot(7, "completed")],
    ]);

    const completed = reconcileCronManualRunViews(
      previous,
      [jobName],
      snapshots,
    );
    expect(
      Object.prototype.hasOwnProperty.call(completed.views, jobName),
    ).toBe(true);
    expect(Object.keys(completed.views)).toEqual([jobName]);
    expect(cronRunButtonPhase(completed.views[jobName])).toBe("success");

    const expired = expireCronManualRunSuccess(completed.views[jobName]);
    expect(cronRunButtonPhase(expired)).toBe("idle");
  });

  it("does not replay old results but adopts a manual run already in progress", () => {
    const initialized = initializeCronManualRunViews(["done", "active"], {
      done: completedSnapshot(2, "failed"),
      active: runSnapshot(3, "running"),
    });
    expect(cronRunButtonPhase(initialized.done)).toBe("idle");
    expect(cronRunButtonPhase(initialized.active)).toBe("running");
    expect(initialized.active.trackedManualRunId).toBe(3);

    const completed = reconcileCronManualRunViews(
      initialized,
      ["done", "active"],
      {
        done: completedSnapshot(2, "failed"),
        active: completedSnapshot(3, "completed"),
      },
    );
    expect(cronRunButtonPhase(completed.views.active)).toBe("success");
    expect(completed.effects).toEqual([]);
  });

  it("keeps a completed flash queued behind a newer active run and drops removed jobs", () => {
    const scheduledWhileCompleted: CronJobRunSnapshot = {
      current_run: {
        run_id: 6,
        trigger: "schedule",
        status: "running",
        started_at_ms: 20,
      },
      last_manual_run: {
        run_id: 5,
        status: "completed",
        executor_error_count: 0,
        completed_at_ms: 10,
      },
    };
    const result = reconcileCronManualRunViews(
      {
        removed: acceptCronManualRun(beginCronManualRun(undefined), 4),
        kept: acceptCronManualRun(beginCronManualRun(undefined), 5),
      },
      ["kept"],
      { kept: scheduledWhileCompleted },
    );
    expect(result.views.removed).toBeUndefined();
    expect(result.views.kept.success).toBe("queued");
    expect(cronRunButtonPhase(result.views.kept)).toBe("running");

    const failedViews = clearCronManualRunViewsAfterStatusFailure(
      result.views,
      ["kept"],
    );
    expect(failedViews.kept.success).toBe("queued");
    expect(hasCronManualRunLocalState(failedViews.kept)).toBe(true);

    const recovered = reconcileCronManualRunViews(
      failedViews,
      ["kept"],
      { kept: scheduledWhileCompleted },
    );
    expect(recovered.views.kept.success).toBe("queued");
    expect(cronRunButtonPhase(recovered.views.kept)).toBe("running");

    const idle = reconcileCronManualRunViews(recovered.views, ["kept"], {
      kept: completedSnapshot(5, "completed"),
    });
    expect(idle.views.kept.success).toBe("visible");
    expect(cronRunButtonPhase(idle.views.kept)).toBe("success");
  });
});

function runSnapshot(
  runId: number,
  status: "pending" | "running",
): CronJobRunSnapshot {
  return {
    current_run: {
      run_id: runId,
      trigger: "manual",
      status,
      started_at_ms: 1,
    },
    last_manual_run: null,
  };
}

function completedSnapshot(
  runId: number,
  status: "completed" | "completed_with_errors" | "failed" | "cancelled",
  executorErrorCount = 0,
): CronJobRunSnapshot {
  return {
    current_run: null,
    last_manual_run: {
      run_id: runId,
      status,
      executor_error_count: executorErrorCount,
      completed_at_ms: 2,
    },
  };
}
