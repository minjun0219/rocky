import { describe, expect, it } from "bun:test";
import agentToolkitPlugin from "./plugin";

/**
 * Smoke test: ensure the opencode plugin entrypoint returns exactly the 7
 * `openapi_*` tools, with no removed-domain tools leaking back into the
 * surface. Detailed handler behaviour is covered by openapi-core's own
 * `handlers.test.ts`.
 */

const EXPECTED_TOOLS = [
  "openapi_get",
  "openapi_refresh",
  "openapi_status",
  "openapi_search",
  "openapi_envs",
  "openapi_endpoint",
  "openapi_tags",
];

const REMOVED_TOOLS = [
  "notion_get",
  "notion_refresh",
  "notion_status",
  "notion_extract",
  "journal_append",
  "journal_read",
  "journal_search",
  "journal_status",
  "mysql_envs",
  "mysql_status",
  "mysql_tables",
  "mysql_schema",
  "mysql_query",
  "spec_pact_fragment",
  "pr_watch_start",
  "pr_watch_stop",
  "pr_watch_status",
  "pr_event_record",
  "pr_event_pending",
  "pr_event_resolve",
];

describe("agent-toolkit-opencode plugin", () => {
  it("exposes exactly the 7 openapi tools", async () => {
    const plugin = (await agentToolkitPlugin(null)) as {
      tool?: Record<string, unknown>;
    };
    expect(plugin.tool).toBeDefined();
    const names = Object.keys(plugin.tool ?? {}).sort();
    expect(names).toEqual([...EXPECTED_TOOLS].sort());
  });

  it("does not leak removed-domain tools", async () => {
    const plugin = (await agentToolkitPlugin(null)) as {
      tool?: Record<string, unknown>;
    };
    const names = new Set(Object.keys(plugin.tool ?? {}));
    for (const removed of REMOVED_TOOLS) {
      expect(names.has(removed)).toBe(false);
    }
  });

  it("does not register skills or agents (those layers are archived)", async () => {
    const plugin = (await agentToolkitPlugin(null)) as {
      config?: (config: any) => void;
    };
    expect(plugin.config).toBeUndefined();
  });
});
