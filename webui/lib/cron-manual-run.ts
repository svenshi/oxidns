import type { PluginAppliedStatus } from "@/hooks/use-plugin-applied";

export function cronManualRunRuntimeTag(
  editing: boolean,
  appliedStatus: PluginAppliedStatus,
  pluginTag: string,
): string | undefined {
  return !editing && appliedStatus === "applied" ? pluginTag : undefined;
}
