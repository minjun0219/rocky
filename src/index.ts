/**
 * Claude Code MCP server entrypoint for rocky.
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

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import pkg from '../package.json' with { type: 'json' };
import {
  createBunNotionCli,
  createNotionCacheFromEnv,
  createRockyRegistry,
  detectNotionCli,
  HTTP_METHODS,
  loadConfig,
  handleNotionExtract,
  handleNotionGet,
  handleNotionRefresh,
  handleNotionStatus,
  handleSwaggerEndpoint,
  handleSwaggerEnvs,
  handleSwaggerGet,
  handleSwaggerRefresh,
  handleSwaggerSearch,
  handleSeoValidate,
  handleSwaggerStatus,
  handleSwaggerTags,
  type NotionCliExecutor,
} from './core';

/**
 * MCP `tools/call` results must be a `CallToolResult`. We always serialize the
 * handler return value as a single JSON text content block.
 */
function jsonResult(value: unknown) {
  return {
    content: [
      {
        type: 'text' as const,
        text: JSON.stringify(value, null, 2),
      },
    ],
  };
}

/** `buildServer` 주입 옵션. 테스트가 CLI executor 를 fake 로 대체할 때 쓴다. */
export interface BuildServerOptions {
  /**
   * Notion CLI executor 주입. 미지정이면 `ntn` 을 spawn 하는 Bun 백엔드를 만든다.
   * detect 는 `ntn --version` 성공(= 설치/실행 가능) 여부만 본다 — 실패하면 (미설치 / 실행 불가)
   * notion_* 도구는 등록되지 않는다. 로그인/권한은 여기서 판정하지 않고 호출 시 에러로 표면화된다.
   */
  notionCli?: NotionCliExecutor;
}

/**
 * Build the MCP server with all openapi tools wired up, plus notion_* tools when
 * a Notion CLI (`ntn`) is detected. Exported for tests so they can register tools
 * against an in-process server without spawning a child.
 */
export async function buildServer(options: BuildServerOptions = {}) {
  const { config: toolkitConfig, errors: configErrors } = await loadConfig();
  for (const e of configErrors) {
    console.error(
      `rocky: skipped config file ${e.source} — ${e.message}. Other config sources still apply.`,
    );
  }
  const registry = toolkitConfig.openapi?.registry;
  const openapiRegistry = createRockyRegistry({
    ...(registry !== undefined ? { registry } : {}),
  });

  const server = new McpServer({
    name: 'rocky',
    version: pkg.version,
  });

  server.registerTool(
    'openapi_get',
    {
      description:
        'OpenAPI / Swagger spec 을 캐시 우선 정책으로 가져온다. swagger 2.0 은 자동으로 OpenAPI 3.0 으로 변환되고 $ref 는 모두 deref 된다. fresh hit 은 remote 호출 없음. stale hit (TTL 경과) 은 즉시 stale 데이터로 응답하고 백그라운드 conditional GET (If-None-Match / If-Modified-Since) 으로 재검증. miss 면 fetch + parse + index. (input: spec URL 또는 rocky.json 의 host:env:spec handle)',
      inputSchema: { input: z.string() },
    },
    async ({ input }) => jsonResult(await handleSwaggerGet(openapiRegistry, input, registry)),
  );

  server.registerTool(
    'openapi_refresh',
    {
      description: '캐시 (메모리 + 디스크) 를 무시하고 OpenAPI spec 을 강제 재다운로드한다.',
      inputSchema: { input: z.string() },
    },
    async ({ input }) => jsonResult(await handleSwaggerRefresh(openapiRegistry, input, registry)),
  );

  server.registerTool(
    'openapi_status',
    {
      description:
        '캐시된 OpenAPI spec 의 메타 (cached / fetchedAt / ttlSeconds / environments) 만 조회. remote 호출 없음.',
      inputSchema: { input: z.string() },
    },
    async ({ input }) => jsonResult(await handleSwaggerStatus(openapiRegistry, input, registry)),
  );

  server.registerTool(
    'openapi_search',
    {
      description:
        '캐시 (메모리 또는 디스크) 에 있는 OpenAPI spec 들을 가로질러 endpoint 를 점수화 검색한다 (operationId>path>summary>description). remote 호출 없음 — 미캐시 spec 은 결과에 포함되지 않으니 먼저 `openapi_get` 으로 받아둬야 한다.',
      inputSchema: {
        query: z.string(),
        limit: z.number().int().positive().optional(),
        scope: z.string().optional(),
      },
    },
    async ({ query, limit, scope }) =>
      jsonResult(await handleSwaggerSearch(openapiRegistry, query, { limit, scope }, registry)),
  );

  server.registerTool(
    'openapi_envs',
    {
      description:
        'rocky.json 의 openapi.registry 를 host:env:spec 평면 리스트로 반환한다. baseUrl / format 이 있으면 함께 반환. remote 호출 없음.',
      inputSchema: {},
    },
    async () => jsonResult(handleSwaggerEnvs(toolkitConfig)),
  );

  server.registerTool(
    'openapi_endpoint',
    {
      description:
        '단일 endpoint 의 풍부한 정보 (parameters / requestBody / responses / examples / fullUrl) 를 반환한다. baseUrl 합성된 fullUrl 은 leaf 의 baseUrl 이 비어 있으면 path 자체.',
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
    'openapi_tags',
    {
      description: 'spec 의 OpenAPI tag 목록 + 각 tag 의 endpoint 개수를 반환한다.',
      inputSchema: { input: z.string() },
    },
    async ({ input }) => jsonResult(await handleSwaggerTags(openapiRegistry, input, registry)),
  );

  // notion_* 는 외부 Notion CLI (`ntn`) 위임 도메인 — 설치가 탐지된 CLI 가 있을 때만 노출한다
  // (로그인/권한은 호출 시점에 에러로 표면화). gh CLI 위임 (`/finish`, `/pr-watch`) 과 동일
  // 정책: 토큰 / OAuth 를 rocky 가 직접 다루지 않는다.
  const notionCli = options.notionCli ?? createBunNotionCli();
  if (await detectNotionCli(notionCli)) {
    const notionCache = createNotionCacheFromEnv();
    server.registerTool(
      'notion_get',
      {
        description:
          'Notion 페이지를 캐시 우선 정책으로 가져온다. 캐시 hit (TTL 이내) 이면 `ntn` CLI 미호출, miss / 만료면 `ntn pages get` 으로 1회 fetch 후 캐시. (input: pageId 또는 Notion URL)',
        inputSchema: { input: z.string() },
      },
      async ({ input }) => jsonResult(await handleNotionGet(notionCache, notionCli, input)),
    );
    server.registerTool(
      'notion_refresh',
      {
        description:
          '캐시를 무시하고 Notion 페이지를 강제 재fetch 한다. 기존 캐시가 있으면 heading-section 단위 diff (added / removed / modified + line 수 + compact preview) 를 함께 반환해 긴 기획서의 변경 위치를 위에서부터 확인할 수 있다. (input: pageId 또는 Notion URL)',
        inputSchema: { input: z.string() },
      },
      async ({ input }) => jsonResult(await handleNotionRefresh(notionCache, notionCli, input)),
    );
    server.registerTool(
      'notion_status',
      {
        description:
          '캐시된 Notion 페이지의 메타 (exists / expired / cachedAt / ttlSeconds / ageSeconds / title) 만 조회한다. `ntn` CLI 미호출.',
        inputSchema: { input: z.string() },
      },
      async ({ input }) => jsonResult(await handleNotionStatus(notionCache, input)),
    );
    server.registerTool(
      'notion_extract',
      {
        description:
          '긴 Notion 페이지를 캐시 우선으로 읽고 heading 기반 chunk 와 구현 액션 후보 (requirements / screens / apis / todos / questions) 를 반환한다. remote 호출 정책은 notion_get 과 동일. (input: pageId 또는 URL, maxCharsPerChunk?: chunk 최대 문자 수 기본 1400)',
        inputSchema: {
          input: z.string(),
          maxCharsPerChunk: z.number().int().positive().optional(),
        },
      },
      async ({ input, maxCharsPerChunk }) =>
        jsonResult(await handleNotionExtract(notionCache, notionCli, input, { maxCharsPerChunk })),
    );
  }

  server.registerTool(
    'seo_validate',
    {
      description:
        '단일 URL 의 OG / Twitter Card / JSON-LD / favicon 메타를 ogpeek 으로 fetch + parse 해서 검증한다. summary (finalUrl / redirects / og:title / og:description / og:image / og:type / og:url / canonical / errors / warnings / info / hasJsonLd / hasFavicon / iconCount) + raw OgDebugResult 둘 다 반환. errors 는 ogpeek warnings 중 severity=error (`OG_TITLE_MISSING` / `OG_TYPE_MISSING` / `OG_IMAGE_MISSING` / `OG_URL_MISSING`) 만 추린 것. 기본 SSRF 가드는 private / loopback / link-local / IPv6 ULA 호스트 차단 — rocky.json 의 `seo.allowPrivateHosts:true` 또는 도구 호출 인자 `allowPrivateHosts:true` 로 끈다. (url: 검증할 http/https URL, timeoutMs?: fetch timeout (1..30000, 기본 8000), allowPrivateHosts?: SSRF 가드 비활성, 기본 config.seo.allowPrivateHosts ?? false)',
      inputSchema: {
        url: z.string(),
        timeoutMs: z.number().int().positive().optional(),
        allowPrivateHosts: z.boolean().optional(),
      },
    },
    async ({ url, timeoutMs, allowPrivateHosts }) =>
      jsonResult(await handleSeoValidate(toolkitConfig.seo, { url, timeoutMs, allowPrivateHosts })),
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
    console.error(`rocky MCP server failed: ${(err as Error).stack ?? err}`);
    process.exit(1);
  });
}
