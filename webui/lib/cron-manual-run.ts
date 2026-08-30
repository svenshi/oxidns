import type { PluginAppliedStatus } from "@/hooks/use-plugin-applied";
import type {
  CronCurrentRun,
  CronJobRunSnapshot,
  CronManualRunStatus,
} from "@/lib/oxidns-api";

export const CRON_STATUS_POLL_INTERVAL_MS = 2000;
export const CRON_SUCCESS_DURATION_MS = 3000;
const CRON_UNCERTAIN_START_MAX_STATUS_MISSES = 2;

export type CronRunButtonPhase =
  | "idle"
  | "starting"
  | "pending"
  | "running"
  | "success";

export interface CronManualRunView {
  starting: boolean;
  currentRun: CronCurrentRun | null;
  trackedManualRunId: number | null;
  lastObservedManualRunId: number | null | undefined;
  manualStartBaselineRunId: number | null | undefined;
  uncertainStartStatusMisses: number | null;
  success: "none" | "queued" | "visible";
}

export interface CronManualRunEffect {
  jobName: string;
  type:
    | Exclude<CronManualRunStatus, "completed">
    | "lost"
    | "start_failed";
  executorErrorCount: number;
}

export interface CronManualRunReconcileResult {
  views: Record<string, CronManualRunView>;
  effects: CronManualRunEffect[];
}

interface CronStatusRequestLease {
  signal: AbortSignal;
  isCurrent: () => boolean;
  release: () => void;
}

export class CronStatusRequestCoordinator {
  private version = 0;
  private active:
    | {
        controller: AbortController;
        parentSignal?: AbortSignal;
        abortFromParent: () => void;
        version: number;
      }
    | undefined;

  begin(parentSignal?: AbortSignal): CronStatusRequestLease {
    this.cancelActive();
    const version = ++this.version;
    const controller = new AbortController();
    const abortFromParent = () => controller.abort();
    if (parentSignal?.aborted) {
      controller.abort();
    } else {
      parentSignal?.addEventListener("abort", abortFromParent, { once: true });
    }
    const active = {
      controller,
      parentSignal,
      abortFromParent,
      version,
    };
    this.active = active;

    return {
      signal: controller.signal,
      isCurrent: () =>
        !controller.signal.aborted &&
        this.active === active &&
        this.version === version,
      release: () => {
        parentSignal?.removeEventListener("abort", abortFromParent);
        if (this.active === active) this.active = undefined;
      },
    };
  }

  invalidate() {
    this.version += 1;
    this.cancelActive();
  }

  private cancelActive() {
    const active = this.active;
    if (!active) return;
    active.parentSignal?.removeEventListener("abort", active.abortFromParent);
    active.controller.abort();
    this.active = undefined;
  }
}

export function emptyCronManualRunView(): CronManualRunView {
  return {
    starting: false,
    currentRun: null,
    trackedManualRunId: null,
    lastObservedManualRunId: undefined,
    manualStartBaselineRunId: undefined,
    uncertainStartStatusMisses: null,
    success: "none",
  };
}

export function cronConfigValuesForDisplay(
  editing: boolean,
  draft: Record<string, unknown>,
  current: Record<string, unknown>,
): Record<string, unknown> {
  return editing ? draft : current;
}

export function cronManualRunRuntimeTag(
  editing: boolean,
  appliedStatus: PluginAppliedStatus,
  pluginTag: string,
  configVersion: string | null,
  runningVersion: string | null,
): string | undefined {
  return !editing &&
    appliedStatus === "applied" &&
    configVersion !== null &&
    configVersion === runningVersion
    ? pluginTag
    : undefined;
}

export function cronRunButtonPhase(
  view: CronManualRunView | undefined,
): CronRunButtonPhase {
  if (!view) return "idle";
  if (view.starting) return "starting";
  if (view.currentRun) return view.currentRun.status;
  if (view.success === "visible") return "success";
  return "idle";
}

export function hasCronManualRunLocalState(
  view: CronManualRunView,
): boolean {
  return (
    view.starting ||
    view.trackedManualRunId !== null ||
    view.success !== "none"
  );
}

export function beginCronManualRun(
  view: CronManualRunView | undefined,
): CronManualRunView {
  return {
    ...(view ?? emptyCronManualRunView()),
    starting: true,
    trackedManualRunId: null,
    manualStartBaselineRunId: view?.lastObservedManualRunId,
    uncertainStartStatusMisses: null,
    success: "none",
  };
}

export function acceptCronManualRun(
  view: CronManualRunView | undefined,
  runId: number,
): CronManualRunView {
  return {
    ...(view ?? emptyCronManualRunView()),
    starting: true,
    trackedManualRunId: runId,
    manualStartBaselineRunId: undefined,
    uncertainStartStatusMisses: null,
  };
}

export function setCronManualRunStatusBaseline(
  view: CronManualRunView | undefined,
  snapshot: CronJobRunSnapshot | undefined,
): CronManualRunView {
  return {
    ...(view ?? emptyCronManualRunView()),
    lastObservedManualRunId: snapshot?.last_manual_run?.run_id ?? null,
    manualStartBaselineRunId: snapshot?.last_manual_run?.run_id ?? null,
  };
}

export function markCronManualRunStartUncertain(
  view: CronManualRunView | undefined,
): CronManualRunView {
  return {
    ...(view ?? emptyCronManualRunView()),
    starting: true,
    trackedManualRunId: null,
    uncertainStartStatusMisses: 0,
  };
}

export function rejectCronManualRun(
  view: CronManualRunView | undefined,
): CronManualRunView {
  return {
    ...(view ?? emptyCronManualRunView()),
    starting: false,
    trackedManualRunId: null,
    manualStartBaselineRunId: undefined,
    uncertainStartStatusMisses: null,
  };
}

export function expireCronManualRunSuccess(
  view: CronManualRunView,
): CronManualRunView {
  return view.success === "visible" ? { ...view, success: "none" } : view;
}

export function clearCronManualRunViewsAfterStatusFailure(
  previous: Record<string, CronManualRunView>,
  jobNames: string[],
): Record<string, CronManualRunView> {
  return Object.fromEntries(
    jobNames.map((jobName) => {
      const prior = previous[jobName] ?? emptyCronManualRunView();
      return [
        jobName,
        {
          ...prior,
          starting:
            prior.starting ||
            prior.trackedManualRunId !== null ||
            prior.uncertainStartStatusMisses !== null,
          currentRun: null,
        },
      ];
    }),
  );
}

export function initializeCronManualRunViews(
  jobNames: string[],
  snapshots: Record<string, CronJobRunSnapshot>,
): Record<string, CronManualRunView> {
  return Object.fromEntries(
    jobNames.map((jobName) => {
      const currentRun = snapshots[jobName]?.current_run ?? null;
      return [
        jobName,
        {
          ...emptyCronManualRunView(),
          currentRun,
          trackedManualRunId:
            currentRun?.trigger === "manual" ? currentRun.run_id : null,
          lastObservedManualRunId:
            snapshots[jobName]?.last_manual_run?.run_id ?? null,
          manualStartBaselineRunId: undefined,
        },
      ];
    }),
  );
}

export function reconcileCronManualRunViews(
  previous: Record<string, CronManualRunView>,
  jobNames: string[],
  snapshots: Record<string, CronJobRunSnapshot>,
): CronManualRunReconcileResult {
  const viewEntries: Array<[string, CronManualRunView]> = [];
  const effects: CronManualRunEffect[] = [];

  for (const jobName of jobNames) {
    const snapshot = snapshots[jobName] ?? {
      current_run: null,
      last_manual_run: null,
    };
    const prior = previous[jobName] ?? emptyCronManualRunView();
    const next: CronManualRunView = {
      ...prior,
      currentRun: snapshot.current_run,
      lastObservedManualRunId: snapshot.last_manual_run?.run_id ?? null,
    };
    const trackedRunId = prior.trackedManualRunId;

    if (trackedRunId !== null) {
      if (snapshot.current_run?.run_id === trackedRunId) {
        next.starting = false;
        next.manualStartBaselineRunId = undefined;
        next.uncertainStartStatusMisses = null;
      } else if (snapshot.last_manual_run?.run_id === trackedRunId) {
        applyCronManualRunResult(next, snapshot, jobName, effects);
      } else {
        next.starting = false;
        next.trackedManualRunId = null;
        next.manualStartBaselineRunId = undefined;
        next.uncertainStartStatusMisses = null;
        next.success = "none";
        effects.push({ jobName, type: "lost", executorErrorCount: 0 });
      }
    } else if (prior.uncertainStartStatusMisses !== null) {
      if (snapshot.current_run?.trigger === "manual") {
        next.starting = false;
        next.trackedManualRunId = snapshot.current_run.run_id;
        next.manualStartBaselineRunId = undefined;
        next.uncertainStartStatusMisses = null;
      } else if (
        snapshot.last_manual_run &&
        snapshot.last_manual_run.run_id !== prior.manualStartBaselineRunId
      ) {
        applyCronManualRunResult(next, snapshot, jobName, effects);
      } else {
        const misses = prior.uncertainStartStatusMisses + 1;
        if (misses >= CRON_UNCERTAIN_START_MAX_STATUS_MISSES) {
          next.starting = false;
          next.manualStartBaselineRunId = undefined;
          next.uncertainStartStatusMisses = null;
          next.success = "none";
          effects.push({
            jobName,
            type: "start_failed",
            executorErrorCount: 0,
          });
        } else {
          next.starting = true;
          next.uncertainStartStatusMisses = misses;
        }
      }
    }

    if (next.success === "visible" && snapshot.current_run) {
      next.success = "queued";
    } else if (next.success === "queued" && !snapshot.current_run) {
      next.success = "visible";
    }

    if (
      !next.starting &&
      next.trackedManualRunId === null &&
      snapshot.current_run?.trigger === "manual"
    ) {
      next.trackedManualRunId = snapshot.current_run.run_id;
    }

    viewEntries.push([jobName, next]);
  }

  return { views: Object.fromEntries(viewEntries), effects };
}

function applyCronManualRunResult(
  view: CronManualRunView,
  snapshot: CronJobRunSnapshot,
  jobName: string,
  effects: CronManualRunEffect[],
) {
  const result = snapshot.last_manual_run;
  if (!result) return;
  view.starting = false;
  view.trackedManualRunId = null;
  view.manualStartBaselineRunId = undefined;
  view.uncertainStartStatusMisses = null;
  if (result.status === "completed") {
    view.success = snapshot.current_run ? "queued" : "visible";
  } else {
    view.success = "none";
    effects.push({
      jobName,
      type: result.status,
      executorErrorCount: result.executor_error_count,
    });
  }
}
