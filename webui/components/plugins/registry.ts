import type { PluginInstance } from "@/lib/types";
import type { PluginComponentDefinition } from "./types";
import { sequencePlugin } from "./kinds/sequence";
import { cachePlugin } from "./kinds/cache";
import { queryRecorderPlugin } from "./kinds/query-recorder";
import { downloadPlugin } from "./kinds/download";
import { cronPlugin } from "./kinds/cron";
import { dynamicDomainSetPlugin } from "./kinds/dynamic-domain-set";

// Custom card/detail overrides for plugin kinds that need bespoke UI beyond
// the generic template. Any kind not listed here falls back to
// `PluginCardTemplate` / `PluginDetailTemplate`. Keep entries here aligned
// with the Rust factory names registered under `src/plugin/`.
export const pluginComponentRegistry: Record<
  string,
  PluginComponentDefinition
> = {
  sequence: sequencePlugin,
  cache: cachePlugin,
  query_recorder: queryRecorderPlugin,
  cron: cronPlugin,
  download: downloadPlugin,
  dynamic_domain_set: dynamicDomainSetPlugin,
};

export function getPluginComponentDefinition(plugin: PluginInstance) {
  return pluginComponentRegistry[plugin.pluginKind];
}
