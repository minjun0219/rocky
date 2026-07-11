import { tool as defineTool } from "@opencode-ai/plugin";
import {
  createAgentToolkitRegistry,
  handleSwaggerEndpoint,
  handleSwaggerEnvs,
  handleSwaggerGet,
  handleSwaggerRefresh,
  handleSwaggerSearch,
  handleSwaggerStatus,
  handleSwaggerTags,
  loadConfig,
} from "@minjun0219/openapi-core";

/**
 * agent-toolkit opencode plugin entrypoint.
 *
 * Exposes the 7 `openapi_*` tools — same surface as the Claude Code plugin
 * (`@minjun0219/agent-toolkit-claude-code`) and the standalone `openapi-mcp`
 * CLI. Handler implementations live in `@minjun0219/openapi-core/handlers`,
 * so behavioural drift across the three hosts is impossible.
 *
 * v0.3 부터 toolkit 은 OpenAPI 도메인만 다룬다. 이전 surface 의 journal / mysql /
 * spec-pact / pr-watch / notion 도메인은 `archive/pre-openapi-only-slim` 브랜치에 박제되어
 * 있고, ROADMAP 에 정의된 phase 단위로 (a) 두 plugin 에 다시 합류 (b) subset MCP 패키지로
 * 분리 셋 중 하나로 재추가된다.
 */

type LegacyToolParam = {
  type?: string;
  required?: boolean;
  items?: LegacyToolParam;
};

type LegacyToolDefinition = {
  description: string;
  parameters?: Record<string, LegacyToolParam>;
  handler(args: any): Promise<unknown> | unknown;
};

function schemaFromParam(param: LegacyToolParam = {}): any {
  const z = defineTool.schema;
  let schema: any;
  switch (param.type) {
    case "string":
      schema = z.string();
      break;
    case "number":
      schema = z.number();
      break;
    case "boolean":
      schema = z.boolean();
      break;
    case "array":
      schema = z.array(schemaFromParam(param.items ?? {}));
      break;
    default:
      schema = z.any();
      break;
  }
  return param.required ? schema : schema.optional();
}

function serializeToolResult(result: unknown): string {
  if (typeof result === "string") return result;
  return JSON.stringify(result, null, 2);
}

function createOpencodeTools<T extends Record<string, LegacyToolDefinition>>(
  tools: T,
) {
  return Object.fromEntries(
    Object.entries(tools).map(([name, legacy]) => {
      const args = Object.fromEntries(
        Object.entries(legacy.parameters ?? {}).map(([key, param]) => [
          key,
          schemaFromParam(param),
        ]),
      );
      const opencodeTool = defineTool({
        description: legacy.description,
        args,
        async execute(args) {
          return serializeToolResult(await legacy.handler(args));
        },
      });

      // Kept for direct unit tests of handler-level behavior; opencode uses execute().
      return [name, { ...opencodeTool, handler: legacy.handler }];
    }),
  ) as unknown as {
    [K in keyof T]: ReturnType<typeof defineTool> & {
      handler: T[K]["handler"];
    };
  };
}

export default async function agentToolkitPlugin(_input: unknown) {
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

  return {
    tool: createOpencodeTools({
      openapi_get: {
        description:
          "OpenAPI / Swagger spec 을 캐시 우선 정책으로 가져온다. swagger 2.0 은 자동으로 OpenAPI 3.0 으로 변환되고 $ref 는 모두 deref 된다. fresh hit 은 remote 호출 없음. stale hit (TTL 경과) 은 즉시 stale 데이터로 응답하고 백그라운드 conditional GET (If-None-Match / If-Modified-Since) 으로 재검증. miss 면 fetch + parse + index. (input: spec URL 또는 agent-toolkit.json 의 host:env:spec handle)",
        parameters: { input: { type: "string", required: true } },
        async handler({ input }: { input: string }) {
          return handleSwaggerGet(openapiRegistry, input, registry);
        },
      },
      openapi_refresh: {
        description:
          "캐시 (메모리 + 디스크) 를 무시하고 OpenAPI spec 을 강제 재다운로드한다. (input: spec URL 또는 host:env:spec handle)",
        parameters: { input: { type: "string", required: true } },
        async handler({ input }: { input: string }) {
          return handleSwaggerRefresh(openapiRegistry, input, registry);
        },
      },
      openapi_status: {
        description:
          "캐시된 OpenAPI spec 의 메타 (cached / fetchedAt / ttlSeconds / environments) 만 조회. remote 호출 없음. (input: spec URL 또는 host:env:spec handle)",
        parameters: { input: { type: "string", required: true } },
        async handler({ input }: { input: string }) {
          return handleSwaggerStatus(openapiRegistry, input, registry);
        },
      },
      openapi_search: {
        description:
          "캐시 (메모리 또는 디스크) 에 있는 OpenAPI spec 들을 가로질러 endpoint 를 점수화 검색한다 (operationId>path>summary>description). remote 호출 없음 — 미캐시 spec 은 결과에 포함되지 않으니 먼저 `openapi_get` 으로 받아둬야 한다. (query: 검색어, limit?: 결과 최대 개수 기본 20, scope?: agent-toolkit.json 에 등록된 host / host:env / host:env:spec — 주면 그 안에서만 검색)",
        parameters: {
          query: { type: "string", required: true },
          limit: { type: "number", required: false },
          scope: { type: "string", required: false },
        },
        async handler({
          query,
          limit,
          scope,
        }: {
          query: string;
          limit?: number;
          scope?: string;
        }) {
          return handleSwaggerSearch(
            openapiRegistry,
            query,
            { limit, scope },
            registry,
          );
        },
      },
      openapi_envs: {
        description:
          "agent-toolkit.json 의 openapi.registry 를 host:env:spec 평면 리스트로 반환한다. baseUrl / format 이 leaf 에 선언돼 있으면 함께 반환. remote 호출 없음. config 가 없거나 비어 있으면 빈 배열.",
        parameters: {},
        async handler() {
          return handleSwaggerEnvs(toolkitConfig);
        },
      },
      openapi_endpoint: {
        description:
          "단일 endpoint 의 풍부한 정보 (parameters / requestBody / responses / examples / fullUrl) 를 반환한다. baseUrl 은 host:env:spec leaf 의 baseUrl 또는 빈 문자열 — 비면 fullUrl 은 path 자체. (input: spec URL 또는 host:env:spec handle. operationId 단독, 또는 method+path 페어 중 하나 필수.)",
        parameters: {
          input: { type: "string", required: true },
          operationId: { type: "string", required: false },
          method: { type: "string", required: false },
          path: { type: "string", required: false },
        },
        async handler({
          input,
          operationId,
          method,
          path,
        }: {
          input: string;
          operationId?: string;
          method?: string;
          path?: string;
        }) {
          return handleSwaggerEndpoint(
            openapiRegistry,
            input,
            { operationId, method, path },
            registry,
          );
        },
      },
      openapi_tags: {
        description:
          "spec 의 OpenAPI tag 목록 + 각 tag 의 endpoint 개수를 반환한다. (input: spec URL 또는 host:env:spec handle)",
        parameters: { input: { type: "string", required: true } },
        async handler({ input }: { input: string }) {
          return handleSwaggerTags(openapiRegistry, input, registry);
        },
      },
    }),
  };
}
