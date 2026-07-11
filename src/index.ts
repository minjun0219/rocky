/**
 * Claude Code MCP server entrypoint for agent-toolkit.
 *
 * stdio JSON-RPC server that exposes the 7 `openapi_*` tools. Handler
 * implementations are shared through `./core` (`handleSwagger*`) with the
 * standalone `openapi-mcp` CLI (`./standalone` + `bin/openapi-mcp`).
 *
 * v0.3 부터 toolkit 은 OpenAPI 도메인만 다룬다. 이전 surface 의 journal / mysql / spec-pact /
 * pr-watch / notion 도메인은 `archive/pre-openapi-only-slim` 브랜치에, opencode plugin 은
 * in-tree `.archive/agent-toolkit-opencode/` 에 박제되어 있다. 활용 패턴이 잡히면 ROADMAP 의
 * phase 단위로 재추가된다.
 */

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import pkg from "../package.json" with { type: "json" };
import {
  createAgentToolkitRegistry,
  HTTP_METHODS,
  loadConfig,
  handleSwaggerEndpoint,
  handleSwaggerEnvs,
  handleSwaggerGet,
  handleSwaggerRefresh,
  handleSwaggerSearch,
  handleSwaggerStatus,
  handleSwaggerTags,
} from "./core";

/**
 * MCP `tools/call` results must be a `CallToolResult`. We always serialize the
 * handler return value as a single JSON text content block.
 */
function jsonResult(value: unknown) {
  return {
    content: [
      {
        type: "text" as const,
        text: JSON.stringify(value, null, 2),
      },
    ],
  };
}

/**
 * Build the MCP server with all 7 openapi tools wired up. Exported for tests so
 * they can register tools against an in-process server without spawning a child.
 */
export async function buildServer() {
  const { config: toolkitConfig, errors: configErrors } = await loadConfig();
  for (const e of configErrors) {
    console.error(
      `agent-toolkit: skipped config file ${e.source} — ${e.message}. Other config sources still apply.`,
    );
  }
  const registry = toolkitConfig.openapi?.registry;
  const openapiRegistry = createAgentToolkitRegistry({
    ...(registry !== undefined ? { registry } : {}),
  });

  const server = new McpServer({
    name: "agent-toolkit",
    version: pkg.version,
  });

  server.registerTool(
    "openapi_get",
    {
      description:
        "OpenAPI / Swagger spec 을 캐시 우선 정책으로 가져온다. swagger 2.0 은 자동으로 OpenAPI 3.0 으로 변환되고 $ref 는 모두 deref 된다. fresh hit 은 remote 호출 없음. stale hit (TTL 경과) 은 즉시 stale 데이터로 응답하고 백그라운드 conditional GET (If-None-Match / If-Modified-Since) 으로 재검증. miss 면 fetch + parse + index. (input: spec URL 또는 agent-toolkit.json 의 host:env:spec handle)",
      inputSchema: { input: z.string() },
    },
    async ({ input }) =>
      jsonResult(await handleSwaggerGet(openapiRegistry, input, registry)),
  );

  server.registerTool(
    "openapi_refresh",
    {
      description:
        "캐시 (메모리 + 디스크) 를 무시하고 OpenAPI spec 을 강제 재다운로드한다.",
      inputSchema: { input: z.string() },
    },
    async ({ input }) =>
      jsonResult(await handleSwaggerRefresh(openapiRegistry, input, registry)),
  );

  server.registerTool(
    "openapi_status",
    {
      description:
        "캐시된 OpenAPI spec 의 메타 (cached / fetchedAt / ttlSeconds / environments) 만 조회. remote 호출 없음.",
      inputSchema: { input: z.string() },
    },
    async ({ input }) =>
      jsonResult(await handleSwaggerStatus(openapiRegistry, input, registry)),
  );

  server.registerTool(
    "openapi_search",
    {
      description:
        "캐시 (메모리 또는 디스크) 에 있는 OpenAPI spec 들을 가로질러 endpoint 를 점수화 검색한다 (operationId>path>summary>description). remote 호출 없음 — 미캐시 spec 은 결과에 포함되지 않으니 먼저 `openapi_get` 으로 받아둬야 한다.",
      inputSchema: {
        query: z.string(),
        limit: z.number().int().positive().optional(),
        scope: z.string().optional(),
      },
    },
    async ({ query, limit, scope }) =>
      jsonResult(
        await handleSwaggerSearch(
          openapiRegistry,
          query,
          { limit, scope },
          registry,
        ),
      ),
  );

  server.registerTool(
    "openapi_envs",
    {
      description:
        "agent-toolkit.json 의 openapi.registry 를 host:env:spec 평면 리스트로 반환한다. baseUrl / format 이 있으면 함께 반환. remote 호출 없음.",
      inputSchema: {},
    },
    async () => jsonResult(handleSwaggerEnvs(toolkitConfig)),
  );

  server.registerTool(
    "openapi_endpoint",
    {
      description:
        "단일 endpoint 의 풍부한 정보 (parameters / requestBody / responses / examples / fullUrl) 를 반환한다. baseUrl 합성된 fullUrl 은 leaf 의 baseUrl 이 비어 있으면 path 자체.",
      inputSchema: {
        input: z.string(),
        operationId: z.string().optional(),
        method: z.enum(HTTP_METHODS).optional(),
        path: z.string().optional(),
      },
    },
    async ({ input, operationId, method, path }) =>
      jsonResult(
        await handleSwaggerEndpoint(
          openapiRegistry,
          input,
          { operationId, method, path },
          registry,
        ),
      ),
  );

  server.registerTool(
    "openapi_tags",
    {
      description:
        "spec 의 OpenAPI tag 목록 + 각 tag 의 endpoint 개수를 반환한다.",
      inputSchema: { input: z.string() },
    },
    async ({ input }) =>
      jsonResult(await handleSwaggerTags(openapiRegistry, input, registry)),
  );

  return server;
}

/**
 * Entrypoint when run as the MCP server binary. Tests import `buildServer`
 * directly and never hit this branch.
 */
async function main() {
  const server = await buildServer();
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

if (import.meta.main) {
  main().catch((err) => {
    console.error(
      `agent-toolkit MCP server failed: ${(err as Error).stack ?? err}`,
    );
    process.exit(1);
  });
}
