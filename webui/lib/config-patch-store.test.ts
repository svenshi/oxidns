import { beforeEach, describe, expect, it } from "vitest";
import { parseOxiDnsYaml, pluginsFromConfig } from "./oxidns-config";
import { useAppStore } from "./store";

function loadStoreConfig(source: string) {
  const parsed = parseOxiDnsYaml(source);
  if (!parsed.config) throw new Error(parsed.diagnostics.join("\n"));
  useAppStore.setState({
    configModel: parsed.config,
    configText: source,
    yamlConfig: source,
    plugins: pluginsFromConfig(parsed.config),
    configError: null,
    configDiagnostics: [],
    configPatchConfirmation: null,
    configEditorBaseline: null,
    editorMode: false,
  });
}

describe("config patch confirmation state", () => {
  beforeEach(() => {
    useAppStore.getState().resolveConfigPatchConfirmation("cancel");
  });

  it("applies a safe edit immediately", async () => {
    const source = `plugins: # keep
  - tag: main
    type: forward
    args:
      upstream: "udp://1.1.1.1" # keep
`;
    loadStoreConfig(source);

    const resolution = await useAppStore
      .getState()
      .updatePluginConfig("main", { upstream: "udp://8.8.8.8" });

    expect(resolution).toBe("patched");
    expect(useAppStore.getState().configText).toBe(
      source.replace("udp://1.1.1.1", "udp://8.8.8.8"),
    );
    expect(useAppStore.getState().configPatchConfirmation).toBeNull();
  });

  it("keeps state unchanged on cancel and stages a local candidate for review", async () => {
    const source = `shared: &shared { timeout: 2s }
plugins:
  - tag: main
    type: forward
    args: *shared # keep alias
`;
    loadStoreConfig(source);

    const cancelled = useAppStore
      .getState()
      .updatePluginConfig("main", { timeout: "3s" });
    expect(useAppStore.getState().configPatchConfirmation).toMatchObject({
      affectedPath: "plugins.main.args",
      canForce: true,
    });
    useAppStore.getState().resolveConfigPatchConfirmation("cancel");
    await expect(cancelled).resolves.toBe("cancelled");
    expect(useAppStore.getState().configText).toBe(source);

    const reviewed = useAppStore
      .getState()
      .updatePluginConfig("main", { timeout: "3s" });
    useAppStore.getState().resolveConfigPatchConfirmation("review");
    await expect(reviewed).resolves.toBe("review");
    expect(useAppStore.getState().editorMode).toBe(true);
    expect(useAppStore.getState().configEditorBaseline).toBe(source);
    expect(useAppStore.getState().configText).toContain(
      "args: { timeout: 3s } # keep alias",
    );
  });

  it("applies the reviewed local candidate after explicit force", async () => {
    const source = `shared: &shared { timeout: 2s }
plugins:
  - tag: main
    type: forward
    args: *shared
`;
    loadStoreConfig(source);

    const forced = useAppStore
      .getState()
      .updatePluginConfig("main", { timeout: "3s" });
    useAppStore.getState().resolveConfigPatchConfirmation("force");

    await expect(forced).resolves.toBe("forced");
    expect(useAppStore.getState().configText).toContain(
      "args: { timeout: 3s }",
    );
  });
});
