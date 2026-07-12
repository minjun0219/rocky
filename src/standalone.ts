/**
 * openapi-mcp 단독(standalone) 진입점 로직. `bin/openapi-mcp` 가 이 모듈을 띄운다.
 *
 * Claude Code 플러그인 진입점(`./index`)과 달리 config 형태가 `openapi-mcp.json`
 * (`specs.<name>.environments.<env>.baseUrl`) 그대로다 — `rocky.json` adapter 를
 * 거치지 않고 `SpecRegistry` 에 직접 등록한다.
 *
 * tool 표면은 플러그인과 동일한 7 개 (`openapi_*`) — handler 코어(`./core`)를 공유해
 * 두 진입점이 같은 surface 를 노출한다.
 */

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import pkg from '../package.json' with { type: 'json' };
import { createDiskCache, createNoopDiskCache, type DiskCache } from './core/cache';
import { defaultDiskCacheDir } from './core/config-loader';
import { createFetcher } from './core/fetcher';
import { buildEndpointDetail, HTTP_METHODS, resolveEndpoint } from './core/indexer';
import { filterEndpoints } from './core/filter';
import {
  createSpecRegistry,
  UnknownEnvironmentError,
  UnknownSpecError,
  type SpecRegistry,
} from './core/registry';
import type { OpenApiMcpConfig } from './core/schema';
import { getLogger } from './core/logger';

export const SERVER_NAME = 'openapi-mcp';
/** package.json 의 version 을 단일 source 로 사용 — `bin/openapi-mcp -V` 와 동기. */
export const SERVER_VERSION = pkg.version;

export interface BuildServerOptions {
  /** 상대 file 소스 경로의 기준 디렉토리. 보통 config 파일이 있는 디렉토리. */
  configDir?: string;
}

export interface ServerHandle {
  server: McpServer;
  registry: SpecRegistry;
}

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

function errorResult(message: string) {
  return {
    isError: true,
    content: [
      {
        type: 'text' as const,
        text: JSON.stringify({ error: message }, null, 2),
      },
    ],
  };
}

export function buildServer(
  config: OpenApiMcpConfig,
  options: BuildServerOptions = {},
): ServerHandle {
  const fetcherOptions: Parameters<typeof createFetcher>[0] = {};
  if (config.http?.timeoutMs !== undefined) {
    fetcherOptions.timeoutMs = config.http.timeoutMs;
  }
  if (config.http?.insecureTls !== undefined) {
    fetcherOptions.insecureTls = config.http.insecureTls;
  }
  if (config.http?.extraCaCerts !== undefined) {
    fetcherOptions.extraCaCerts = config.http.extraCaCerts;
  }
  const fetcher = createFetcher(fetcherOptions);

  const diskCacheEnabled = config.cache?.diskCache ?? true;
  const diskCache: DiskCache = diskCacheEnabled
    ? createDiskCache(config.cache?.diskCachePath ?? defaultDiskCacheDir())
    : createNoopDiskCache();

  const registry = createSpecRegistry(config, fetcher, {
    diskCache,
    // 외부 `$ref` 다운로드도 root fetcher 와 같은 timeout / TLS 정책을 따르게 한다.
    parseFetcherOptions: fetcherOptions,
    ...(options.configDir ? { configDir: options.configDir } : {}),
  });

  const server = new McpServer(
    { name: SERVER_NAME, version: SERVER_VERSION },
    {
      instructions:
        'Browse internal OpenAPI / Swagger specs. openapi_envs → openapi_get → openapi_search → openapi_endpoint / openapi_tags. openapi_refresh forces a re-fetch.',
    },
  );

  // openapi_envs (단독 entry 에는 rocky-config registry 가 없으므로 specs.* 를 그대로 평탄화)
  server.registerTool(
    'openapi_envs',
    {
      description:
        'Configured spec 목록과 각 spec 의 environments(baseUrl 포함). remote 호출 없음.',
      inputSchema: {},
    },
    async () => {
      const out = registry.listSpecs().flatMap((s) =>
        s.environments.map((env) => ({
          spec: s.name,
          environment: env,
          baseUrl: registry.getEnvironment(s.name, env).baseUrl,
        })),
      );
      return jsonResult({ entries: out });
    },
  );

  server.registerTool(
    'openapi_get',
    {
      description:
        'spec 을 캐시 우선으로 가져온다. swagger 2.0 자동 변환 + $ref deref. (input: spec name, optional environment)',
      inputSchema: {
        input: z.string(),
        environment: z.string().optional(),
      },
    },
    async ({ input, environment }) => {
      try {
        const indexed = await registry.loadSpec(input, environment);
        return jsonResult({
          spec: input,
          environment: environment ?? null,
          document: indexed.document,
        });
      } catch (err) {
        if (err instanceof UnknownSpecError || err instanceof UnknownEnvironmentError) {
          return errorResult(err.message);
        }
        throw err;
      }
    },
  );

  server.registerTool(
    'openapi_refresh',
    {
      description: '캐시를 비우고 spec 을 강제 재다운로드. (input: spec name 옵션)',
      inputSchema: { input: z.string().optional() },
    },
    async ({ input }) => jsonResult({ refreshed: await registry.refresh(input) }),
  );

  server.registerTool(
    'openapi_status',
    {
      description: 'spec 의 cache status. remote 호출 없음.',
      inputSchema: { input: z.string() },
    },
    async ({ input }) => {
      const summary = registry.listSpecs().find((s) => s.name === input);
      if (!summary) {
        return errorResult(`unknown spec '${input}'`);
      }
      return jsonResult(summary);
    },
  );

  server.registerTool(
    'openapi_search',
    {
      description:
        'endpoint 점수화 검색 (operationId>path>summary>description). spec / tag / method 로 필터.',
      inputSchema: {
        query: z.string().optional(),
        spec: z.string().optional(),
        tag: z.string().optional(),
        method: z.enum(HTTP_METHODS).optional(),
        limit: z.number().int().positive().max(200).optional(),
      },
    },
    async ({ query, spec, tag, method, limit }) => {
      try {
        const targets = spec ? [spec] : registry.listSpecs().map((s) => s.name);
        for (const name of targets) {
          if (!registry.hasSpec(name)) {
            return errorResult(`unknown spec '${name}'`);
          }
        }
        // 후보 spec 들을 병렬 로드. 한 spec fetch 실패는 그 spec 만 빼고 나머지는
        // 계속 (allSettled).
        const settled = await Promise.allSettled(targets.map((name) => registry.loadSpec(name)));
        const all = settled.flatMap((r) => (r.status === 'fulfilled' ? r.value.endpoints : []));
        const filter: Parameters<typeof filterEndpoints>[1] = {};
        if (spec) {
          filter.spec = spec;
        }
        if (tag) {
          filter.tag = tag;
        }
        if (method) {
          filter.method = method;
        }
        if (query?.trim()) {
          filter.keyword = query.trim();
        }
        const filtered = filterEndpoints(all, filter);
        const cap = limit ?? 50;
        return jsonResult({
          total: filtered.length,
          returned: Math.min(filtered.length, cap),
          endpoints: filtered.slice(0, cap).map((e) => ({
            spec: e.specName,
            operationId: e.operationId ?? e.syntheticOperationId,
            method: e.method,
            path: e.path,
            summary: e.summary,
            tags: e.tags,
            deprecated: e.deprecated,
          })),
        });
      } catch (err) {
        if (err instanceof UnknownSpecError) {
          return errorResult(err.message);
        }
        throw err;
      }
    },
  );

  server.registerTool(
    'openapi_endpoint',
    {
      description:
        '단일 endpoint 의 detail (parameters / requestBody / responses / examples / fullUrl).',
      inputSchema: {
        spec: z.string(),
        environment: z.string(),
        operationId: z.string().optional(),
        method: z.enum(HTTP_METHODS).optional(),
        path: z.string().optional(),
      },
    },
    async ({ spec, environment, operationId, method, path }) => {
      try {
        if (!operationId && !(method && path)) {
          return errorResult('must supply either operationId or both method and path');
        }
        const env = registry.getEnvironment(spec, environment);
        const indexed = await registry.loadSpec(spec, environment);
        const ep = resolveEndpoint(indexed, { operationId, method, path });
        if (!ep) {
          const where = operationId ? `operationId='${operationId}'` : `${method} ${path}`;
          return errorResult(`endpoint not found in spec '${spec}' for ${where}`);
        }
        const detail = buildEndpointDetail(indexed, ep, env.baseUrl);
        return jsonResult({ spec, environment, endpoint: detail });
      } catch (err) {
        if (err instanceof UnknownSpecError || err instanceof UnknownEnvironmentError) {
          return errorResult(err.message);
        }
        throw err;
      }
    },
  );

  server.registerTool(
    'openapi_tags',
    {
      description: 'spec 의 OpenAPI tag 목록 + endpoint 개수.',
      inputSchema: { spec: z.string() },
    },
    async ({ spec }) => {
      try {
        if (!registry.hasSpec(spec)) {
          return errorResult(`unknown spec '${spec}'`);
        }
        const indexed = await registry.loadSpec(spec);
        return jsonResult({ spec, tags: indexed.tags });
      } catch (err) {
        if (err instanceof UnknownSpecError) {
          return errorResult(err.message);
        }
        throw err;
      }
    },
  );

  return { server, registry };
}

export async function startStdioServer(
  config: OpenApiMcpConfig,
  options: BuildServerOptions = {},
): Promise<ServerHandle> {
  const handle = buildServer(config, options);
  const transport = new StdioServerTransport();
  await handle.server.connect(transport);
  getLogger().info({ specs: Object.keys(config.specs).length }, 'openapi-mcp connected over stdio');
  return handle;
}
