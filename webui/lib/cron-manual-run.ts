import type { PluginAppliedStatus } from "@/hooks/use-plugin-applied";
import type {
  CronCurrentRun,
  CronJobRunSnapshot,
  CronManualRunStatus,
} from "@/lib/oxidns-api";

export const CRON_STATUS_POLL_INTERVAL_MS = 2000;
export const CRON_SUCCESS_DURATION_MS = 3000;

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
  success: "none" | "queued" | "visible";
}

export interface CronManualRunEffect {
  jobName: string;
  type: Exclude<CronManualRunStatus, "completed"> | "lost";
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

export function beginCronManualRun(
  view: CronManualRunView | undefined,
): CronManualRunView {
  return {
    ...(view ?? emptyCronManualRunView()),
    starting: true,
    trackedManualRunId: null,
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
  };
}

export function rejectCronManualRun(
  view: CronManualRunView | undefined,
): CronManualRunView {
  return {
    ...(view ?? emptyCronManualRunView()),
    starting: false,
    trackedManualRunId: null,
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
          starting: false,
          currentRun: null,
          success: "none",
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
  const views: Record<string, CronManualRunView> = {};
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
    };
    const trackedRunId = prior.trackedManualRunId;

    if (trackedRunId !== null) {
      if (snapshot.current_run?.run_id === trackedRunId) {
        next.starting = false;
      } else if (snapshot.last_manual_run?.run_id === trackedRunId) {
        next.starting = false;
        next.trackedManualRunId = null;
        const result = snapshot.last_manual_run;
        if (result.status === "completed") {
          next.success = snapshot.current_run ? "queued" : "visible";
        } else {
          next.success = "none";
          effects.push({
            jobName,
            type: result.status,
            executorErrorCount: result.executor_error_count,
          });
        }
      } else {
        next.starting = false;
        next.trackedManualRunId = null;
        next.success = "none";
        effects.push({ jobName, type: "lost", executorErrorCount: 0 });
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

    views[jobName] = next;
  }

  return { views, effects };
}
