import path from "node:path";
import {
  createNoopDiskCache,
  type DiskCache,
  type DiskCacheEntry,
} from "./cache";
import { getLogger } from "./logger";
import type { FetcherOptions, SpecFetcher } from "./fetcher";
import type { OpenAPIV3 } from "openapi-types";
import { indexSpec, type IndexedSpec } from "./indexer";
import { parseSpecText } from "./parser";
import {
  DEFAULT_CACHE_TTL_SECONDS,
  type EnvironmentConfig,
  type OpenApiMcpConfig,
  type SpecConfig,
  type SpecSource,
} from "./schema";

/**
 * 메모리 + 디스크 캐시를 끼워둔 OpenAPI spec 레지스트리. SpecRegistry 가 caller (tool
 * handler / MCP server) 에 노출되는 단일 진입점이다.
 *
 * 동작 요약:
 *   1. `loadSpec(name, env?)` — 메모리 hit → in-flight promise → 디스크 hit hydrate
 *      → 새로 fetch + parse + index 순으로 fallback. fetch 시 etag/lastModified 를
 *      엔트리에 함께 저장.
 *   2. TTL 지난 메모리 hit 은 즉시 stale 데이터 반환 + conditional GET 재검증을
 *      백그라운드로 stage. 304 면 fetchedAt 만 갱신, 200 이면 새 indexed spec 으로 교체.
 *   3. `refresh(name?)` — 캐시 (메모리 + 디스크) 비우고 무조건 다시 fetch.
 */

export interface SpecCacheStatus {
  cached: boolean;
  fetchedAt?: string;
  ttlSeconds: number;
}

export interface SpecSummary {
  name: string;
  description?: string;
  environments: string[];
  cacheStatus: SpecCacheStatus;
}

export interface ResolvedEnvironment {
  name: string;
  baseUrl: string;
  description?: string;
}

interface CachedSpec {
  indexed: IndexedSpec;
  fetchedAt: string;
  source: SpecSource;
  document: object;
  detectedFormat: "openapi3" | "swagger2";
  etag?: string;
  lastModified?: string;
  ttlSeconds: number;
}

/**
 * loadSpec 결과가 어디서 왔는지. handler 가 사용자에게 "remote 호출 발생 여부"
 * 를 정확히 보고하기 위해 필요 — 디스크 hydrate 도 cache hit 으로 친다.
 */
export type LoadSource = "memory" | "disk" | "remote";

export interface DetailedLoadResult {
  indexed: IndexedSpec;
  source: LoadSource;
  /** 메모리 또는 디스크에서 왔으면 true (네트워크 fetch 가 일어나지 않았음). */
  fromCache: boolean;
}

export interface SpecRegistry {
  listSpecs(): SpecSummary[];
  listEnvironments(specName: string): ResolvedEnvironment[];
  loadSpec(specName: string, environment?: string): Promise<IndexedSpec>;
  /**
   * `loadSpec` 과 동일하게 spec 을 로드하되 "어디서 왔는지" 까지 보고한다.
   * remote fetch 를 일으키지 않은 호출인지 (memory / disk hit) 또는 remote
   * 였는지 caller 가 그대로 사용자에게 노출할 수 있다.
   */
  loadSpecDetailed(
    specName: string,
    environment?: string,
  ): Promise<DetailedLoadResult>;
  /**
   * 메모리 또는 디스크 캐시에서만 spec 을 로드. cache miss 면 null —
   * remote fetch 는 절대 일으키지 않는다. `openapi_search` 처럼 "remote 호출
   * 없음" 을 계약으로 보장해야 하는 경로에서 사용.
   */
  loadSpecCachedOnly(
    specName: string,
    environment?: string,
  ): Promise<IndexedSpec | null>;
  getEnvironment(specName: string, environment: string): EnvironmentConfig;
  refresh(specName?: string): Promise<RefreshOutcome[]>;
  hasSpec(specName: string): boolean;
  /**
   * 런타임에 spec entry 를 추가한다. agent-toolkit 의 `openapi_get(URL)` 처럼
   * config 에 없는 ad-hoc URL 을 받았을 때 사용. 이미 같은 이름이 등록돼 있으면
   * 새 entry 로 통째로 교체한다 (이름 단위 idempotent).
   */
  registerSpec(name: string, spec: SpecConfig): void;
}

export interface RefreshOutcome {
  spec: string;
  success: boolean;
  fetchedAt?: string;
  error?: string;
}

export class UnknownSpecError extends Error {
  constructor(specName: string) {
    super(`unknown spec '${specName}'`);
    this.name = "UnknownSpecError";
  }
}

export class UnknownEnvironmentError extends Error {
  constructor(specName: string, environment: string) {
    super(`unknown environment '${environment}' for spec '${specName}'`);
    this.name = "UnknownEnvironmentError";
  }
}

export interface SpecRegistryOptions {
  diskCache?: DiskCache;
  /**
   * 상대 경로 `file` source 를 해석하기 위한 디렉토리. 일반적으로 config 파일이
   * 위치한 디렉토리. 미지정 시 process.cwd() 로 떨어진다.
   */
  configDir?: string;
  /**
   * 외부 `$ref` 다운로드에 적용할 timeout / TLS 옵션. root spec fetcher 와 동일
   * 정책을 component 파일 / URL 에도 흘려보낼 때 caller (factory) 가 root fetcher
   * 와 같은 옵션을 그대로 넘긴다.
   */
  parseFetcherOptions?: FetcherOptions;
}

export function createSpecRegistry(
  config: OpenApiMcpConfig,
  fetcher: SpecFetcher,
  options: SpecRegistryOptions = {},
): SpecRegistry {
  return new InMemorySpecRegistry(
    config,
    fetcher,
    options.diskCache ?? createNoopDiskCache(),
    options.configDir,
    options.parseFetcherOptions,
  );
}

class InMemorySpecRegistry implements SpecRegistry {
  private readonly cache = new Map<string, CachedSpec>();
  private readonly inFlight = new Map<string, Promise<DetailedLoadResult>>();
  private readonly backgroundRefreshes = new Set<string>();

  constructor(
    private readonly config: OpenApiMcpConfig,
    private readonly fetcher: SpecFetcher,
    private readonly diskCache: DiskCache,
    private readonly configDir?: string,
    private readonly parseFetcherOptions?: FetcherOptions,
  ) {}

  hasSpec(specName: string): boolean {
    return Object.hasOwn(this.config.specs, specName);
  }

  registerSpec(name: string, spec: SpecConfig): void {
    this.config.specs[name] = spec;
    // 같은 이름 entry 의 기존 캐시는 새 spec 의 source 가 다를 수 있으니 정리.
    for (const key of Array.from(this.cache.keys())) {
      if (key.startsWith(`${name}::`)) {
        this.cache.delete(key);
        this.inFlight.delete(key);
        this.backgroundRefreshes.delete(key);
      }
    }
  }

  listSpecs(): SpecSummary[] {
    return Object.entries(this.config.specs).map(([name, spec]) => ({
      name,
      ...(spec.description !== undefined
        ? { description: spec.description }
        : {}),
      environments: Object.keys(spec.environments),
      cacheStatus: this.cacheStatus(name, spec),
    }));
  }

  listEnvironments(specName: string): ResolvedEnvironment[] {
    const spec = this.requireSpec(specName);
    return Object.entries(spec.environments).map(([name, env]) => ({
      name,
      baseUrl: env.baseUrl,
      ...(env.description !== undefined
        ? { description: env.description }
        : {}),
    }));
  }

  getEnvironment(specName: string, environment: string): EnvironmentConfig {
    const spec = this.requireSpec(specName);
    const env = spec.environments[environment];
    if (!env) throw new UnknownEnvironmentError(specName, environment);
    return env;
  }

  async loadSpec(specName: string, environment?: string): Promise<IndexedSpec> {
    return (await this.loadSpecDetailed(specName, environment)).indexed;
  }

  async loadSpecDetailed(
    specName: string,
    environment?: string,
  ): Promise<DetailedLoadResult> {
    const spec = this.requireSpec(specName);
    const source = this.resolveSource(specName, spec, environment);
    const ttlSeconds = spec.cacheTtlSeconds ?? DEFAULT_CACHE_TTL_SECONDS;
    const key = this.cacheKey(specName, source);

    const memHit = this.cache.get(key);
    if (memHit) {
      if (this.isStale(memHit))
        this.scheduleBackgroundRefresh(specName, source, ttlSeconds);
      return { indexed: memHit.indexed, source: "memory", fromCache: true };
    }

    const inFlight = this.inFlight.get(key);
    if (inFlight) return inFlight;

    const promise = this.hydrateOrFetch(specName, source, ttlSeconds).finally(
      () => {
        this.inFlight.delete(key);
      },
    );
    this.inFlight.set(key, promise);
    return promise;
  }

  async loadSpecCachedOnly(
    specName: string,
    environment?: string,
  ): Promise<IndexedSpec | null> {
    const spec = this.requireSpec(specName);
    const source = this.resolveSource(specName, spec, environment);
    const ttlSeconds = spec.cacheTtlSeconds ?? DEFAULT_CACHE_TTL_SECONDS;
    const key = this.cacheKey(specName, source);

    const memHit = this.cache.get(key);
    if (memHit) return memHit.indexed;

    const hydrated = await this.hydrateFromDisk(specName, source, ttlSeconds);
    return hydrated?.indexed ?? null;
  }

  async refresh(specName?: string): Promise<RefreshOutcome[]> {
    const targets = specName ? [specName] : Object.keys(this.config.specs);
    // 각 spec 의 refresh 는 독립적이므로 모두 병렬로 처리한다. 한 spec 안의 여러
    // source (default + env override) 도 같이 병렬 — 한 spec 의 source 하나가
    // 실패해도 같은 spec 의 다른 source 까지 끌어내리지 않게 try/catch 는 spec
    // 단위로 둔다.
    const settled = await Promise.all(
      targets.map((name) => this.refreshOne(name)),
    );
    return settled;
  }

  private async refreshOne(name: string): Promise<RefreshOutcome> {
    try {
      const spec = this.requireSpec(name);
      const ttlSeconds = spec.cacheTtlSeconds ?? DEFAULT_CACHE_TTL_SECONDS;
      // 상대 `file` source 는 configDir 기준으로 절대 경로화 — loadSpec 의
      // resolveSource 와 동일한 처리. 빠뜨리면 첫 load 는 성공해도 refresh 시
      // process.cwd() 기준이 되어 같은 파일을 못 찾는 회귀가 생긴다.
      const sourcesByKey = new Map<string, SpecSource>();
      const baseSource = this.resolveFilePath(spec.source);
      sourcesByKey.set(this.cacheKey(name, baseSource), baseSource);
      for (const env of Object.values(spec.environments)) {
        if (env.source) {
          const envSource = this.resolveFilePath(env.source);
          sourcesByKey.set(this.cacheKey(name, envSource), envSource);
        }
      }
      // source 별로 캐시 정리 + 재다운로드를 병렬로 — 같은 spec 안에서도 여러
      // env override 가 있으면 모두 동시에 fetch.
      await Promise.all(
        Array.from(sourcesByKey.entries()).map(async ([key, source]) => {
          this.cache.delete(key);
          this.inFlight.delete(key);
          this.backgroundRefreshes.delete(key);
          await this.diskCache.delete(key);
          await this.fetchAndStore(name, source, ttlSeconds);
        }),
      );
      // fetchedAt 는 sourcesByKey 의 첫 entry 기준 — 보고 용도라 정확성보다 일관성 우선.
      const firstKey = sourcesByKey.keys().next().value;
      const fetchedAt =
        (firstKey ? this.cache.get(firstKey)?.fetchedAt : undefined) ??
        new Date().toISOString();
      return { spec: name, success: true, fetchedAt };
    } catch (err) {
      return {
        spec: name,
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  private async hydrateOrFetch(
    specName: string,
    source: SpecSource,
    ttlSeconds: number,
  ): Promise<DetailedLoadResult> {
    const hydrated = await this.hydrateFromDisk(specName, source, ttlSeconds);
    if (hydrated)
      return { indexed: hydrated.indexed, source: "disk", fromCache: true };
    const indexed = await this.fetchAndStore(specName, source, ttlSeconds);
    return { indexed, source: "remote", fromCache: false };
  }

  /**
   * 디스크 캐시에서만 hydrate 시도. cache miss 또는 깨진 entry 면 null —
   * remote fetch 는 절대 트리거하지 않는다. `loadSpec` (fall-through to fetch)
   * 과 `loadSpecCachedOnly` (no fetch) 두 경로의 공통 본문.
   */
  private async hydrateFromDisk(
    specName: string,
    source: SpecSource,
    ttlSeconds: number,
  ): Promise<{ indexed: IndexedSpec } | null> {
    const key = this.cacheKey(specName, source);
    const disk = await this.diskCache.read(key);
    if (!disk) return null;
    try {
      // 디스크 캐시는 이미 deref 된 OpenAPI 3.x document 를 보관하므로, 다시
      // SwaggerParser.dereference 를 돌리지 않고 곧장 indexSpec 으로 넘긴다 —
      // 큰 spec 일수록 deref 비용이 가장 무거우므로 hydrate 경로의 핵심 최적화.
      const document = disk.document as OpenAPIV3.Document;
      const indexed = indexSpec(specName, document);
      const cached: CachedSpec = {
        indexed,
        fetchedAt: disk.cachedAt,
        source,
        document,
        detectedFormat: disk.detectedFormat,
        ...(disk.etag !== undefined ? { etag: disk.etag } : {}),
        ...(disk.lastModified !== undefined
          ? { lastModified: disk.lastModified }
          : {}),
        ttlSeconds,
      };
      this.cache.set(key, cached);
      if (this.isStale(cached))
        this.scheduleBackgroundRefresh(specName, source, ttlSeconds);
      return { indexed };
    } catch (err) {
      getLogger().warn(
        { err, spec: specName },
        "disk cache hydrate failed; falling back to fresh fetch",
      );
      await this.diskCache.delete(key);
      return null;
    }
  }

  private async fetchAndStore(
    specName: string,
    source: SpecSource,
    ttlSeconds: number,
  ): Promise<IndexedSpec> {
    const key = this.cacheKey(specName, source);
    const fetched = await this.fetcher.fetch(source);
    if (fetched.notModified) {
      throw new Error(
        `unexpected 304 response for spec '${specName}' on initial load`,
      );
    }
    const parsed = await parseSpecText(fetched.body, source.format, {
      sourceLocation: this.sourceLocationOf(source),
      ...(this.parseFetcherOptions
        ? { fetcherOptions: this.parseFetcherOptions }
        : {}),
    });
    const indexed = indexSpec(specName, parsed.document);
    const cached: CachedSpec = {
      indexed,
      fetchedAt: fetched.fetchedAt,
      source,
      document: parsed.document,
      detectedFormat: parsed.detectedFormat,
      ...(fetched.etag !== undefined ? { etag: fetched.etag } : {}),
      ...(fetched.lastModified !== undefined
        ? { lastModified: fetched.lastModified }
        : {}),
      ttlSeconds,
    };
    this.cache.set(key, cached);
    await this.diskCache.write(key, this.toDiskEntry(cached));
    return indexed;
  }

  private scheduleBackgroundRefresh(
    specName: string,
    source: SpecSource,
    ttlSeconds: number,
  ): void {
    const key = this.cacheKey(specName, source);
    if (this.backgroundRefreshes.has(key)) return;
    this.backgroundRefreshes.add(key);
    void this.runBackgroundRefresh(specName, source, ttlSeconds, key).finally(
      () => {
        this.backgroundRefreshes.delete(key);
      },
    );
  }

  private async runBackgroundRefresh(
    specName: string,
    source: SpecSource,
    ttlSeconds: number,
    key: string,
  ): Promise<void> {
    const existing = this.cache.get(key);
    if (!existing) return;
    try {
      const conditional: { etag?: string; lastModified?: string } = {};
      if (existing.etag) conditional.etag = existing.etag;
      if (existing.lastModified)
        conditional.lastModified = existing.lastModified;
      const fetched = await this.fetcher.fetch(source, conditional);
      const current = this.cache.get(key);
      if (!current) return;
      if (fetched.notModified) {
        const refreshed: CachedSpec = {
          ...current,
          fetchedAt: fetched.fetchedAt,
          ttlSeconds,
        };
        this.cache.set(key, refreshed);
        await this.diskCache.write(key, this.toDiskEntry(refreshed));
        return;
      }
      const parsed = await parseSpecText(fetched.body, source.format, {
        sourceLocation: this.sourceLocationOf(source),
      });
      if (Date.parse(fetched.fetchedAt) < Date.parse(current.fetchedAt)) {
        return;
      }
      const indexed = indexSpec(specName, parsed.document);
      const refreshed: CachedSpec = {
        indexed,
        fetchedAt: fetched.fetchedAt,
        source,
        document: parsed.document,
        detectedFormat: parsed.detectedFormat,
        ...(fetched.etag !== undefined ? { etag: fetched.etag } : {}),
        ...(fetched.lastModified !== undefined
          ? { lastModified: fetched.lastModified }
          : {}),
        ttlSeconds,
      };
      this.cache.set(key, refreshed);
      await this.diskCache.write(key, this.toDiskEntry(refreshed));
    } catch (err) {
      getLogger().warn(
        { err, spec: specName },
        "background refresh failed; serving stale cache",
      );
    }
  }

  private toDiskEntry(cached: CachedSpec): DiskCacheEntry {
    return {
      schemaVersion: 1,
      cachedAt: cached.fetchedAt,
      ...(cached.etag !== undefined ? { etag: cached.etag } : {}),
      ...(cached.lastModified !== undefined
        ? { lastModified: cached.lastModified }
        : {}),
      source: cached.source,
      detectedFormat: cached.detectedFormat,
      document: cached.document,
    };
  }

  private isStale(cached: CachedSpec): boolean {
    const age = (Date.now() - Date.parse(cached.fetchedAt)) / 1000;
    return age >= cached.ttlSeconds;
  }

  private cacheStatus(specName: string, spec: SpecConfig): SpecCacheStatus {
    const ttlSeconds = spec.cacheTtlSeconds ?? DEFAULT_CACHE_TTL_SECONDS;
    const defaultSource = this.resolveSource(specName, spec);
    const cached = this.cache.get(this.cacheKey(specName, defaultSource));
    if (cached) {
      return { cached: true, fetchedAt: cached.fetchedAt, ttlSeconds };
    }
    return { cached: false, ttlSeconds };
  }

  private resolveSource(
    specName: string,
    spec: SpecConfig,
    environment?: string,
  ): SpecSource {
    let source: SpecSource = spec.source;
    if (environment) {
      const env = spec.environments[environment];
      if (!env) throw new UnknownEnvironmentError(specName, environment);
      if (env.source) source = env.source;
    }
    return this.resolveFilePath(source);
  }

  private resolveFilePath(source: SpecSource): SpecSource {
    if (source.type !== "file" || path.isAbsolute(source.path)) return source;
    const baseDir = this.configDir ?? process.cwd();
    return { ...source, path: path.resolve(baseDir, source.path) };
  }

  /**
   * SwaggerParser.dereference 의 base 로 쓰기 위한 절대 경로 / URL 문자열.
   * `file` source 는 `path` (이미 resolveFilePath 를 통과해 절대 경로),
   * `url` source 는 `url` 자체. 외부 / 상대 `$ref` 가 있는 spec 의 deref 가
   * 정확한 base 위에서 동작하도록.
   */
  private sourceLocationOf(source: SpecSource): string {
    return source.type === "file" ? source.path : source.url;
  }

  private cacheKey(specName: string, source: SpecSource): string {
    const target = source.type === "url" ? source.url : source.path;
    const format = source.format ?? "auto";
    return `${specName}::${source.type}::${target}::${format}`;
  }

  private requireSpec(specName: string): SpecConfig {
    const spec = this.config.specs[specName];
    if (!spec) throw new UnknownSpecError(specName);
    return spec;
  }
}
