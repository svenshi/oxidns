import { describe, expect, it } from "vitest";
import { parseOxiDnsYaml, type OxiDnsConfig } from "./oxidns-config";
import { patchPluginsYaml } from "./oxidns-config-patch";

function configOf(source: string): OxiDnsConfig {
  const result = parseOxiDnsYaml(source);
  expect(result.diagnostics).toEqual([]);
  expect(result.config).toBeDefined();
  return result.config!;
}

function patchedContent(
  source: string,
  update: (config: OxiDnsConfig) => OxiDnsConfig,
  renamedTags?: ReadonlyMap<string, string>,
) {
  const before = configOf(source);
  const after = update(structuredClone(before));
  const result = patchPluginsYaml(source, before, after, { renamedTags });
  if (result.status !== "patched") throw new Error(result.reason);
  return result.content;
}

describe("plugin YAML incremental patching", () => {
  it("returns the original bytes for a semantic no-op", () => {
    const source = "\uFEFF---\r\nplugins: [] # untouched\r\n...";
    const before = configOf(source);
    const result = patchPluginsYaml(source, before, structuredClone(before));
    expect(result).toEqual({ status: "patched", content: source });
  });

  it("treats mapping key order as semantically equivalent", () => {
    const source = `plugins:
  - tag: main
    type: forward
    args:
      upstream: udp://1.1.1.1
      timeout: 2s
`;
    const before = configOf(source);
    const after = structuredClone(before);
    after.plugins[0].args = {
      timeout: "2s",
      upstream: "udp://1.1.1.1",
    };

    expect(patchPluginsYaml(source, before, after)).toEqual({
      status: "patched",
      content: source,
    });
  });

  it("changes one scalar without moving comments or reformatting the file", () => {
    const source = `# root
plugins: # list comment

  # plugin comment
  - tag: "main" # tag comment
    type: forward
    args:
      # target comment
      upstream: "udp://1.1.1.1" # inline comment
      timeout: 2s

log: { level: info } # untouched
`;

    const output = patchedContent(source, (config) => {
      (config.plugins[0].args as Record<string, unknown>).upstream =
        "udp://8.8.8.8";
      return config;
    });

    expect(output).toBe(source.replace('"udp://1.1.1.1"', '"udp://8.8.8.8"'));
  });

  it("preserves CRLF, BOM, document markers and the final-newline choice", () => {
    const source =
      "\uFEFF---\r\nplugins:\r\n  - tag: 'main' # tag\r\n    type: forward\r\n    args: { timeout: '2s' } # args\r\n...";
    const output = patchedContent(source, (config) => {
      (config.plugins[0].args as Record<string, unknown>).timeout = "3s";
      return config;
    });

    expect(output).toBe(source.replace("'2s'", "'3s'"));
    expect(output.endsWith("...")).toBe(true);
    expect(output.replaceAll("\r\n", "")).not.toContain("\n");
  });

  it("adds and removes mapping fields while keeping unknown fields and comments", () => {
    const source = `plugins:
  - tag: main
    type: forward
    x-extension: keep # unmanaged plugin key
    args:
      upstream: udp://1.1.1.1
      unknown_option: keep # unknown args
`;
    const before = configOf(source);
    const after = structuredClone(before);
    after.plugins[0].args = {
      unknown_option: "keep",
      timeout: "3s",
    };
    const result = patchPluginsYaml(source, before, after);
    if (result.status !== "patched") throw new Error(result.reason);

    expect(result.content).toContain(
      "    x-extension: keep # unmanaged plugin key",
    );
    expect(result.content).toContain(
      "      unknown_option: keep # unknown args",
    );
    expect(result.content).not.toContain("upstream:");
    expect(result.content).toContain("      timeout: 3s");
  });

  it("appends a plugin without changing existing plugin text", () => {
    const source = `# header
log: { level: info }
plugins: # keep
  - { tag: first, type: debug_print } # first inline
`;
    const existing = "  - { tag: first, type: debug_print } # first inline";
    const output = patchedContent(source, (config) => {
      config.plugins.push({ tag: "second", type: "debug_print" });
      return config;
    });

    expect(output).toContain("plugins: # keep");
    expect(output).toContain(existing);
    expect(configOf(output).plugins.map((plugin) => plugin.tag)).toEqual([
      "first",
      "second",
    ]);
  });

  it("adds to an empty flow list without moving its inline comment", () => {
    const source =
      "plugins: [] # keep empty-list comment\nlog: { level: info }\n";
    const output = patchedContent(source, (config) => {
      config.plugins.push({ tag: "main", type: "debug_print" });
      return config;
    });

    expect(output).toContain("# keep empty-list comment");
    expect(output).toContain("log: { level: info }");
    expect(configOf(output).plugins[0].tag).toBe("main");
  });

  it("adds the plugins key when it was omitted", () => {
    const source = "---\n# keep\nlog: { level: info }\n...\n";
    const output = patchedContent(source, (config) => {
      config.plugins.push({ tag: "main", type: "debug_print" });
      return config;
    });

    expect(output.startsWith("---\n# keep\nlog: { level: info }\n")).toBe(true);
    expect(output.endsWith("...\n")).toBe(true);
    expect(configOf(output).plugins[0].tag).toBe("main");
  });

  it("adds the plugins key to a flow-style root mapping", () => {
    const source = "{ log: { level: info } } # keep flow root\n# keep tail\n";
    const output = patchedContent(source, (config) => {
      config.plugins.push({ tag: "main", type: "debug_print" });
      return config;
    });

    expect(output).toContain("log: { level: info }");
    expect(output).toContain("# keep flow root\n# keep tail\n");
    expect(configOf(output).plugins[0].tag).toBe("main");
  });

  it("reorders original plugin chunks with their comments intact", () => {
    const source = `plugins:
  # first docs
  - tag: first # first inline
    type: debug_print

  # second docs
  - tag: second # second inline
    type: debug_print
`;
    const output = patchedContent(source, (config) => {
      config.plugins.reverse();
      return config;
    });

    expect(output.indexOf("# second docs")).toBeLessThan(
      output.indexOf("tag: second"),
    );
    expect(output).toContain("tag: second # second inline");
    expect(output).toContain("tag: first # first inline");
    expect(output.indexOf("tag: second")).toBeLessThan(
      output.indexOf("tag: first"),
    );
    expect(configOf(output).plugins.map((plugin) => plugin.tag)).toEqual([
      "second",
      "first",
    ]);
  });

  it("reorders flow-style plugins while preserving the surrounding source", () => {
    const source =
      "# head\nplugins: [ { tag: first, type: debug_print }, { tag: second, type: debug_print } ] # tail\nlog: info\n";
    const output = patchedContent(source, (config) => {
      config.plugins.reverse();
      return config;
    });

    expect(output.startsWith("# head\nplugins:")).toBe(true);
    expect(output).toContain("] # tail\nlog: info\n");
    expect(configOf(output).plugins.map((plugin) => plugin.tag)).toEqual([
      "second",
      "first",
    ]);
  });

  it("deletes the final plugin without reformatting other top-level keys", () => {
    const source =
      "log: { level: info } # before\nplugins:\n  - tag: only\n    type: debug_print\napi: {} # after\n";
    const output = patchedContent(source, (config) => {
      config.plugins = [];
      return config;
    });

    expect(output).toContain("log: { level: info } # before");
    expect(output).toContain("api: {} # after");
    expect(configOf(output).plugins).toEqual([]);
  });

  it("preserves retained array items and their comments when removing one", () => {
    const source = `plugins:
  - tag: list
    type: qname
    args:
      - one # first
      - two # removed
      - three # last
`;
    const output = patchedContent(source, (config) => {
      config.plugins[0].args = ["one", "three"];
      return config;
    });

    expect(output).toContain("- one # first");
    expect(output).toContain("- three # last");
    expect(output).not.toContain("two # removed");
  });

  it("can reset a block sequence to an empty sequence", () => {
    const source = `plugins:
  - tag: list
    type: qname
    args:
      - one
`;
    const output = patchedContent(source, (config) => {
      config.plugins[0].args = [];
      return config;
    });
    expect(configOf(output).plugins[0].args).toEqual([]);
    expect(output).toContain("args:\n      []");
  });

  it("keeps a modified array object when another object is appended", () => {
    const source = `plugins:
  - tag: jobs
    type: cron
    args:
      jobs:
        - name: refresh # keep job comment
          interval: 1m # keep interval comment
`;
    const output = patchedContent(source, (config) => {
      config.plugins[0].args = {
        jobs: [
          { name: "refresh", interval: "5m" },
          { name: "cleanup", interval: "1h" },
        ],
      };
      return config;
    });

    expect(output).toContain("name: refresh # keep job comment");
    expect(output).toContain("interval: 5m # keep interval comment");
    expect(output).toContain("name: cleanup");
  });

  it("retains a block scalar style when changing multiline text", () => {
    const source = `plugins:
  - tag: notes
    type: debug_print
    args:
      note: |-
        first line
        second line
      untouched: "quoted" # keep
`;
    const output = patchedContent(source, (config) => {
      (config.plugins[0].args as Record<string, unknown>).note =
        "replacement line\nsecond replacement";
      return config;
    });

    expect(output).toContain("note: |-");
    expect(output).toContain("replacement line");
    expect(output).toContain('untouched: "quoted" # keep');
  });

  it("renames a plugin and its references without rebuilding either plugin", () => {
    const source = `plugins:
  - tag: target # keep target
    type: debug_print
  - tag: seq # keep source
    type: sequence
    args:
      - exec: $target # keep reference
`;
    const before = configOf(source);
    const after = structuredClone(before);
    after.plugins[0].tag = "renamed";
    (after.plugins[1].args as Array<Record<string, unknown>>)[0].exec =
      "$renamed";

    const result = patchPluginsYaml(source, before, after, {
      renamedTags: new Map([["target", "renamed"]]),
    });
    if (result.status !== "patched") throw new Error(result.reason);
    expect(result.content).toContain("tag: renamed # keep target");
    expect(result.content).toContain("exec: $renamed # keep reference");
  });

  it("returns a reviewed local candidate instead of rewriting aliases", () => {
    const source = `shared: &shared { timeout: 2s }
plugins:
  - tag: main
    type: forward
    args: *shared # keep alias
`;
    const before = configOf(source);
    const after = structuredClone(before);
    after.plugins[0].args = { timeout: "3s" };
    const result = patchPluginsYaml(source, before, after);

    expect(result.status).toBe("needs_confirmation");
    if (result.status !== "needs_confirmation") return;
    expect(result.affectedPath).toBe("plugins.main.args");
    expect(result.candidate?.content).toContain("shared: &shared");
    expect(result.candidate?.content).toContain("args: { timeout: 3s }");
  });
});
