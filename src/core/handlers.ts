import type { OpenAPIV3 } from "openapi-types";
import {
  buildEphemeralSpec,
  DEFAULT_ENVIRONMENT,
  ephemeralSpecName,
} from "./adapter";
import {
  buildEndpointDetail,
  resolveEndpoint,
  type EndpointDetail,
  type IndexedSpec,
  type TagSummary,
} from "./indexer";
import { filterEndpoints } from "./filter";
import {
  type RefreshOutcome,
  type SpecRegistry,
  type SpecSummary,
  UnknownSpecError,
} from "./registry";
import {
  isFullHandle,
  listRegistry,
  resolveHandleToUrl,
  resolveScopeToHandles,
  type OpenapiRegistryEntry,
} from "./openapi-registry";
import type { OpenapiRegistry, ToolkitConfig } from "./toolkit-config";

/**
 * agent-toolkit 의 openapi_* tool 들이 공유하는 handler 묶음.
 *
 * 두 plugin host (Claude Code MCP, opencode plugin) 가 동일 함수를 호출하고 자기 host
 * 의 RPC 모양으로 wrap 한다. standalone `openapi-mcp` CLI 는 config shape (`openapi-mcp.json`)
 * 이 다르고 toolkit-config 의 registry 평탄화 단계를 거치지 않으므로, 자체 tool 정의를
 * 가진 채로 `SpecRegistry` 만 공유한다.
 */

/**
 * input 형태:
 *   - `host:env:spec` handle → registry 에서 leaf 찾아 flatten 된 specName + DEFAULT_ENVIRONMENT
 *   - URL (`http://`/`https://`/`file://`) → ad-hoc spec 으로 SpecRegistry 에 등록
 *     (`url:<sha1-16>`) + DEFAULT_ENVIRONMENT
 *   - 그 외 → throw (16-hex disk key 같은 legacy 키 형태는 v0.2 부터 미지원)
 */
function resolveSwaggerInput(
  registry: SpecRegistry,
  input: string,
  toolkitRegistry?: OpenapiRegistry,
): { specName: string; environment: string; baseUrl?: string } {
  // MCP 호출에서 흔히 따라붙는 앞뒤 공백 / 개행을 입력 검증 전에 정규화 — 그렇지 않으면
  // "https://… \n" 같은 입력이 handle / URL 둘 다에서 곧장 throw 된다.
  const trimmed = input.trim();
  if (isFullHandle(trimmed)) {
    const url = resolveHandleToUrl(trimmed, toolkitRegistry);
    // isFullHandle 통과 시점에 `host:env:spec` 형식이 검증됐고, `flattenHandle` 의 결과는
    // 같은 `:` 조인이라 trimmed 자체를 그대로 SpecRegistry key 로 쓴다 (split + flatten 왕복 제거).
    const flat = trimmed;
    if (!registry.hasSpec(flat)) {
      // registry 가 toolkit-config 로 만들어지지 않은 (단독 진입점) 경우 ad-hoc 등록.
      registry.registerSpec(flat, {
        source: { type: "url", url },
        environments: { [DEFAULT_ENVIRONMENT]: { baseUrl: "" } },
      });
    }
    const baseUrlMaybe = registry
      .listEnvironments(flat)
      .find((e) => e.name === DEFAULT_ENVIRONMENT)?.baseUrl;
    return {
      specName: flat,
      environment: DEFAULT_ENVIRONMENT,
      ...(baseUrlMaybe ? { baseUrl: baseUrlMaybe } : {}),
    };
  }
  if (looksLikeUrl(trimmed)) {
    const name = ephemeralSpecName(trimmed);
    if (!registry.hasSpec(name)) {
      const { spec } = buildEphemeralSpec(trimmed);
      registry.registerSpec(name, spec);
    }
    return { specName: name, environment: DEFAULT_ENVIRONMENT };
  }
  throw new Error(
    `openapi tool input "${input}" is not a host:env:spec handle or http(s)/file URL — pass a registered handle or a full URL`,
  );
}

function looksLikeUrl(s: string): boolean {
  return (
    s.startsWith("http://") ||
    s.startsWith("https://") ||
    s.startsWith("file://")
  );
}

/** openapi_search 옵션 — `scope` 는 host / host:env / host:env:spec 중 하나. */
export interface SwaggerSearchOptions {
  limit?: number;
  scope?: string;
}

/** openapi_get 결과 — fetched / parsed / dereferenced 된 OpenAPI 3.x document. */
export interface SwaggerGetResult {
  spec: string;
  environment: string;
  fromCache: boolean;
  document: OpenAPIV3.Document;
  baseUrl?: string;
}

/**
 * 도구 핸들러: 캐시 우선으로 spec 을 가져온다. 결과는 deref 된 OpenAPI 3.x document.
 * swagger 2.0 입력은 자동 변환된다.
 */
export async function handleSwaggerGet(
  registry: SpecRegistry,
  input: string,
  toolkitRegistry?: OpenapiRegistry,
): Promise<SwaggerGetResult> {
  const { specName, environment, baseUrl } = resolveSwaggerInput(
    registry,
    input,
    toolkitRegistry,
  );
  // loadSpecDetailed 가 memory / disk / remote 중 어디서 왔는지 알려준다.
  const detailed = await registry.loadSpecDetailed(specName, environment);
  const result: SwaggerGetResult = {
    spec: specName,
    environment,
    fromCache: detailed.fromCache,
    document: detailed.indexed.document,
  };
  if (baseUrl) result.baseUrl = baseUrl;
  return result;
}

/** 도구 핸들러: 캐시 (메모리 + 디스크) 비우고 강제 재다운로드. */
export async function handleSwaggerRefresh(
  registry: SpecRegistry,
  input: string,
  toolkitRegistry?: OpenapiRegistry,
): Promise<RefreshOutcome[]> {
  const { specName } = resolveSwaggerInput(registry, input, toolkitRegistry);
  return registry.refresh(specName);
}

/** 도구 핸들러: 캐시 메타 (cached / fetchedAt / ttlSeconds) 만 조회. remote 호출 없음. */
export async function handleSwaggerStatus(
  registry: SpecRegistry,
  input: string,
  toolkitRegistry?: OpenapiRegistry,
): Promise<SpecSummary> {
  const { specName } = resolveSwaggerInput(registry, input, toolkitRegistry);
  const summary = registry.listSpecs().find((s) => s.name === specName);
  if (!summary) {
    // resolveSwaggerInput 직후라 보통 미스 발생 안 함. 안전망.
    throw new UnknownSpecError(specName);
  }
  return summary;
}

/**
 * 캐시된 spec 들을 가로질러 endpoint 검색 (점수화: operationId>path>summary>description).
 * - `options.scope` (`host` / `host:env` / `host:env:spec`) 가 주어지면 registry 에서
 *   해당 entry 들로 좁힌다. 매칭 0 건이면 throw — 사용자가 잘못된 scope 를 넘겼음을 빨리
 *   알리려고.
 * - `options.limit` 는 결과 최대 개수 (기본 20).
 * - 빈 query 는 (scope 안의) 모든 endpoint 를 limit 까지 나열.
 *
 * remote-free 보장: 메모리 또는 디스크 캐시에 있는 spec 만 검색한다. 미캐시 spec 은
 * 결과에서 빠지며, 사용자가 먼저 `openapi_get` 으로 받아두어야 한다.
 */
export async function handleSwaggerSearch(
  registry: SpecRegistry,
  query: string,
  options: SwaggerSearchOptions = {},
  toolkitRegistry?: OpenapiRegistry,
): Promise<SwaggerSearchMatch[]> {
  const { limit, scope } = options;
  const cap =
    Number.isFinite(limit) && (limit as number) > 0 ? (limit as number) : 20;

  const allSpecs = registry.listSpecs();
  let candidates = allSpecs.map((s) => s.name);
  if (scope) {
    // scope 는 사용자가 지정한 registry path 단위 (host / host:env / host:env:spec)
    // 이므로 같은 URL 을 공유하는 다른 handle 까지 끌려가지 않게 handle 자체로
    // 좁힌다.
    const handles = resolveScopeToHandles(scope, toolkitRegistry);
    if (handles.length === 0) {
      throw new Error(
        `openapi_search: scope "${scope}" matched no entries in openapi.registry — check ./.opencode/agent-toolkit.json or ~/.config/opencode/agent-toolkit/agent-toolkit.json`,
      );
    }
    // resolveScopeToHandles 가 이미 `host:env:spec` 형태의 canonical handle 을 반환하므로
    // 그 문자열이 곧 SpecRegistry key — split + flatten 왕복 없이 set 으로 직접 좁힌다.
    const flatNames = new Set(handles);
    candidates = candidates.filter((n) => flatNames.has(n));
  }

  // remote-free 보장: cached-only loader 를 써서 캐시 (메모리 / 디스크 hydrate)
  // 에 있는 spec 만 검색한다. 미캐시 spec 은 결과에 빠지며, 사용자가 먼저
  // openapi_get 으로 가져와야 한다.
  const settled = await Promise.allSettled(
    candidates.map((name) => registry.loadSpecCachedOnly(name)),
  );
  const indexedSpecs: IndexedSpec[] = settled
    .filter(
      (r): r is PromiseFulfilledResult<IndexedSpec | null> =>
        r.status === "fulfilled",
    )
    .map((r) => r.value)
    .filter((v): v is IndexedSpec => v !== null);

  const merged = indexedSpecs.flatMap((ix) => ix.endpoints);
  const trimmedQuery = query?.trim();
  const filter = trimmedQuery ? { keyword: trimmedQuery } : {};
  const filtered = filterEndpoints(merged, filter);
  const truncated = filtered.slice(0, cap);

  return truncated.map((e) => ({
    spec: e.specName,
    operationId: e.operationId ?? e.syntheticOperationId,
    method: e.method,
    path: e.path,
    summary: e.summary,
    tags: e.tags,
    deprecated: e.deprecated,
  }));
}

export interface SwaggerSearchMatch {
  spec: string;
  operationId: string;
  method: string;
  path: string;
  summary?: string;
  tags?: string[];
  deprecated: boolean;
}

/** registry 트리를 평면 (host, env, spec, url, baseUrl?, format?) 리스트로 반환. */
export function handleSwaggerEnvs(
  config: ToolkitConfig,
): OpenapiRegistryEntry[] {
  return listRegistry(config);
}

/** openapi_endpoint 입력 — operationId 단독 또는 method+path 페어. */
export interface SwaggerEndpointLocator {
  operationId?: string;
  method?: string;
  path?: string;
}

/** openapi_endpoint 결과 — full detail + baseUrl 합성된 fullUrl. */
export interface SwaggerEndpointResult {
  spec: string;
  environment: string;
  endpoint: EndpointDetail;
}

/**
 * 도구 핸들러: 단일 endpoint 의 풍부한 정보 (parameters / requestBody / responses /
 * examples / fullUrl) 를 반환한다. baseUrl 이 비어 있으면 fullUrl 은 path 자체.
 */
export async function handleSwaggerEndpoint(
  registry: SpecRegistry,
  input: string,
  locator: SwaggerEndpointLocator,
  toolkitRegistry?: OpenapiRegistry,
): Promise<SwaggerEndpointResult> {
  if (!locator.operationId && !(locator.method && locator.path)) {
    throw new Error(
      "openapi_endpoint requires either operationId or both method and path",
    );
  }
  const { specName, environment } = resolveSwaggerInput(
    registry,
    input,
    toolkitRegistry,
  );
  const env = registry.getEnvironment(specName, environment);
  const indexed = await registry.loadSpec(specName, environment);
  const ep = resolveEndpoint(indexed, locator);
  if (!ep) {
    const where = locator.operationId
      ? `operationId='${locator.operationId}'`
      : `${locator.method?.toUpperCase()} ${locator.path}`;
    throw new Error(`endpoint not found in spec '${specName}' for ${where}`);
  }
  const detail = buildEndpointDetail(indexed, ep, env.baseUrl);
  return { spec: specName, environment, endpoint: detail };
}

/** openapi_tags 결과 — IndexedSpec.tags 그대로 + 부가 메타. */
export interface SwaggerTagsResult {
  spec: string;
  environment: string;
  tags: TagSummary[];
}

/** 도구 핸들러: spec 의 tag 목록 + 각 tag 의 endpoint count. */
export async function handleSwaggerTags(
  registry: SpecRegistry,
  input: string,
  toolkitRegistry?: OpenapiRegistry,
): Promise<SwaggerTagsResult> {
  const { specName, environment } = resolveSwaggerInput(
    registry,
    input,
    toolkitRegistry,
  );
  const indexed = await registry.loadSpec(specName, environment);
  return { spec: specName, environment, tags: indexed.tags };
}
