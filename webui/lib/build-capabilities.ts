import type { BuildInfo, SupportedPlugins } from "./oxidns-api";
import type { PluginType } from "./types";

const supportedPluginFieldByType: Record<PluginType, keyof SupportedPlugins> = {
  server: "servers",
  executor: "executors",
  matcher: "matchers",
  provider: "providers",
};

export function isPluginKindSupported(
  buildInfo: BuildInfo | null | undefined,
  type: PluginType,
  pluginKind: string,
) {
  if (!buildInfo) return true;
  const supportedKinds =
    buildInfo.supported_plugins[supportedPluginFieldByType[type]];
  return supportedKinds.includes(pluginKind);
}
