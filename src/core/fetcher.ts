import { readFile } from "node:fs/promises";
import path from "node:path";
import type { SpecSource } from "./schema";

/**
 * spec 소스 (URL 또는 file path) 에서 raw 본문을 가져온다. URL 모드에서는 etag /
 * lastModified conditional GET 을 지원해 304 응답을 그대로 흘린다.
 *
 * HTTP transport 는 Bun 의 `fetch` 를 그대로 쓴다. 초기 구현은 undici 를 거쳐
 * `Agent({ connect: { rejectUnauthorized } })` 로 TLS 옵션을 주입했지만, Bun /
 * undici v7 조합에서 self-signed 서버에 대해 dispatcher 가 실제 핸드셰이크에
 * 적용되지 않는 회귀가 확인됐다 (dongle-agent#65 reproducer). Bun 의 `fetch` 는
 * `tls.rejectUnauthorized` / `tls.ca` 등을 표준 옵션으로 받으므로 그쪽이 더
 * 신뢰성 있다.
 */

/**
 * Bun 의 fetch 가 표준 RequestInit 위에 추가로 받는 옵션. 타입 정의가 lib.dom 에
 * 없으므로 한 단 끼워서 type-safe 하게 호출한다.
 */
interface BunRequestInit extends RequestInit {
  tls?: {
    rejectUnauthorized?: boolean;
    ca?: string | string[];
  };
}

export class SpecFetchError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
    public override readonly cause?: unknown,
  ) {
    super(message);
    this.name = "SpecFetchError";
  }
}

export interface FetchResult {
  body: string;
  etag?: string;
  lastModified?: string;
  fetchedAt: string;
  notModified: false;
  source: SpecSource;
}

export interface NotModifiedResult {
  notModified: true;
  fetchedAt: string;
  source: SpecSource;
}

export type FetchOutcome = FetchResult | NotModifiedResult;

export interface ConditionalHeaders {
  etag?: string;
  lastModified?: string;
}

export interface FetcherOptions {
  timeoutMs?: number;
  insecureTls?: boolean;
  extraCaCerts?: string[];
}

export interface SpecFetcher {
  fetch(
    source: SpecSource,
    conditional?: ConditionalHeaders,
  ): Promise<FetchOutcome>;
}

export function createFetcher(options: FetcherOptions = {}): SpecFetcher {
  return new DefaultSpecFetcher(options);
}

class DefaultSpecFetcher implements SpecFetcher {
  /** TLS 옵션이 한 번 빌드되면 캐시 — extraCaCerts 의 파일 read 를 매 요청마다 하지 않음. */
  private tlsInitCache: BunRequestInit["tls"] | undefined;
  private tlsInitResolved = false;

  constructor(private readonly options: FetcherOptions) {}

  async fetch(
    source: SpecSource,
    conditional?: ConditionalHeaders,
  ): Promise<FetchOutcome> {
    if (source.type === "file") {
      return this.fetchFile(source);
    }
    return this.fetchUrl(source, conditional);
  }

  private async fetchFile(
    source: Extract<SpecSource, { type: "file" }>,
  ): Promise<FetchResult> {
    const absolute = path.resolve(source.path);
    try {
      const body = await readFile(absolute, "utf8");
      return {
        body,
        fetchedAt: new Date().toISOString(),
        notModified: false,
        source,
      };
    } catch (err) {
      const reason = err instanceof Error ? err.message : String(err);
      throw new SpecFetchError(
        `failed to read spec file ${absolute}: ${reason}`,
        undefined,
        err,
      );
    }
  }

  private async fetchUrl(
    source: Extract<SpecSource, { type: "url" }>,
    conditional?: ConditionalHeaders,
  ): Promise<FetchOutcome> {
    const headers: Record<string, string> = {
      Accept:
        "application/json, application/yaml;q=0.9, text/yaml;q=0.9, */*;q=0.1",
    };
    if (conditional?.etag) headers["If-None-Match"] = conditional.etag;
    if (conditional?.lastModified)
      headers["If-Modified-Since"] = conditional.lastModified;

    const controller = new AbortController();
    const timeout = setTimeout(
      () => controller.abort(),
      this.options.timeoutMs ?? 10_000,
    );

    try {
      const tls = await this.resolveTlsInit();
      const init: BunRequestInit = {
        method: "GET",
        headers,
        signal: controller.signal,
        // redirect 는 follow (기본) — spec serve 가 흔히 redirect 를 끼므로.
        redirect: "follow",
        ...(tls ? { tls } : {}),
      };
      const response = await fetch(source.url, init);

      const fetchedAt = new Date().toISOString();
      if (response.status === 304) {
        await drainBody(response);
        return { notModified: true, fetchedAt, source };
      }
      if (!response.ok) {
        await drainBody(response);
        throw new SpecFetchError(
          `unexpected HTTP ${response.status} fetching ${source.url}`,
          response.status,
        );
      }

      const body = await response.text();
      const etag = response.headers.get("etag") ?? undefined;
      const lastModified = response.headers.get("last-modified") ?? undefined;
      return {
        body,
        ...(etag !== undefined ? { etag } : {}),
        ...(lastModified !== undefined ? { lastModified } : {}),
        fetchedAt,
        notModified: false,
        source,
      };
    } catch (err) {
      if (err instanceof SpecFetchError) throw err;
      const reason = err instanceof Error ? err.message : String(err);
      throw new SpecFetchError(
        `failed to fetch ${source.url}: ${reason}`,
        undefined,
        err,
      );
    } finally {
      clearTimeout(timeout);
    }
  }

  /**
   * Bun fetch 의 `tls` 옵션을 만든다.
   *   - `insecureTls=true` → `{ rejectUnauthorized: false }`
   *   - `extraCaCerts` → 파일 본문을 읽어 `{ ca: [...] }`
   *   - 둘 다 없으면 undefined → fetch 가 시스템 기본 TLS 정책으로 동작.
   *
   * `extraCaCerts` 의 파일 read 는 첫 호출에서만 일어나고 결과 객체를 캐시한다.
   */
  private async resolveTlsInit(): Promise<BunRequestInit["tls"] | undefined> {
    if (this.tlsInitResolved) return this.tlsInitCache;
    if (this.options.insecureTls === true) {
      this.tlsInitCache = { rejectUnauthorized: false };
    } else if (
      this.options.extraCaCerts !== undefined &&
      this.options.extraCaCerts.length > 0
    ) {
      const cas = await Promise.all(
        this.options.extraCaCerts.map(async (p) => {
          try {
            return await readFile(p, "utf8");
          } catch (err) {
            const reason = err instanceof Error ? err.message : String(err);
            throw new SpecFetchError(
              `failed to read extraCaCerts entry '${p}': ${reason}`,
              undefined,
              err,
            );
          }
        }),
      );
      this.tlsInitCache = { ca: cas };
    } else {
      this.tlsInitCache = undefined;
    }
    this.tlsInitResolved = true;
    return this.tlsInitCache;
  }
}

/** 응답 본문을 수동으로 비우는 helper — 상태 코드만 보고 끝낼 때 누수 방지. */
async function drainBody(response: Response): Promise<void> {
  if (response.body) {
    try {
      await response.body.cancel();
    } catch {
      // 이미 닫혔거나 한 번 읽혔으면 무시.
    }
  }
}
