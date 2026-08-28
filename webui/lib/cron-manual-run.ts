import type { PluginAppliedStatus } from "@/hooks/use-plugin-applied";

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
