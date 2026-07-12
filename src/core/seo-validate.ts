import { isIP } from 'node:net';
import { parse } from 'ogpeek';
import type { OgDebugResult, Warning } from 'ogpeek';
import { FetchError, fetchHtml, type RedirectHop } from 'ogpeek/fetch';
import type { SeoConfig } from './rocky-config';

/**
 * `seo_validate` 도구의 코어 — 단일 URL 을 fetch + ogpeek parse 해서 OG / Twitter Card /
 * JSON-LD / favicon 메타를 검증한다. OpenAPI 도메인과 독립적이며 `ogpeek` 하나에만 의존한다.
 */

export interface SeoValidateOptions {
  /** 검증 대상 URL. http / https 만 허용. */
  url: string;
  /** fetch timeout (ms). 호출 인자 우선; 없으면 ogpeek 기본 8000. 1..30000 으로 clamp. */
  timeoutMs?: number;
  /** true 면 SSRF 가드 비활성. 호출 인자 우선; 없으면 호출자 (`handleSeoValidate`) 가 config 기본을 채워 넣는다. */
  allowPrivateHosts?: boolean;
  /** 테스트 / DI 용 transport override. 기본 `globalThis.fetch`. */
  fetch?: (url: string, init: RequestInit) => Promise<Response>;
}

export interface SeoValidateSummary {
  /** redirect 까지 마친 최종 URL. */
  finalUrl: string;
  /** redirect 추적 결과 (없으면 빈 배열). */
  redirects: RedirectHop[];
  ogTitle: string | null;
  ogDescription: string | null;
  /** og:image 가 여러 개면 첫 번째만. */
  ogImage: string | null;
  ogType: string | null;
  ogUrl: string | null;
  canonical: string | null;
  /** ogpeek warnings 중 severity="error" 만. */
  errors: Warning[];
  /** ogpeek warnings 중 severity="warn" 만. */
  warnings: Warning[];
  /** ogpeek warnings 중 severity="info" 만. */
  info: Warning[];
  hasJsonLd: boolean;
  hasFavicon: boolean;
  iconCount: number;
}

export interface SeoValidateResult {
  /** 에이전트가 그대로 markdown 표 / 권고로 가공하기 좋은 요약. */
  summary: SeoValidateSummary;
  /** ogpeek `parse` 의 원본 결과 — twitter / typed / raw / icons / jsonld 까지 모두 포함. */
  raw: OgDebugResult;
}

/**
 * SSRF 가드 hit 시 surface 되는 ogpeek `FetchError` 의 code.
 * ogpeek `fetchHtml` 의 guard 슬롯은 throw 된 `FetchError` 를 그대로 다시 던지지만
 * 그 외 Error 는 `code: "GUARD_FAILED"` 로 wrapping 한다. 따라서 호출자에게 깔끔한
 * code 한 종류만 노출되도록 가드는 처음부터 `FetchError("BLOCKED", ...)` 를 던진다.
 */
export const SEO_BLOCKED_CODE = 'BLOCKED';

/** 도구 호출 인자 / config 양쪽에서 받는 timeout 의 상한. ogpeek 기본은 8000ms. */
export const SEO_MAX_TIMEOUT_MS = 30_000;

/**
 * RFC1918 / loopback / link-local / IPv6 ULA 범위 검사.
 * IP literal 만 차단한다 — DNS resolve 후 사설 IP 로 가는 외부 호스트는 막지 않는다 (그건
 * fetch 단의 socket-level guard 가 필요하고, MVP 범위 밖). hostname 이 호스트명일 때는
 * `localhost` / `*.localhost` 만 차단.
 */
export function isPrivateHost(hostname: string): boolean {
  if (!hostname) {
    return false;
  }
  // IPv6 literal 의 양 끝 대괄호 제거.
  const host = hostname.replace(/^\[/, '').replace(/\]$/, '').toLowerCase();
  if (host === 'localhost') {
    return true;
  }
  if (host.endsWith('.localhost')) {
    return true;
  }
  const ipVersion = isIP(host);
  if (ipVersion === 4) {
    // isIP === 4 는 유효한 dotted-quad 를 보장하므로 octet 재검증은 불필요.
    const [a, b] = host.split('.').map(Number) as [number, number, number, number];
    if (a === 0) {
      return true;
    }
    if (a === 10) {
      return true;
    }
    if (a === 127) {
      return true;
    }
    if (a === 169 && b === 254) {
      return true;
    }
    if (a === 172 && b >= 16 && b <= 31) {
      return true;
    }
    if (a === 192 && b === 168) {
      return true;
    }
    return false;
  }
  if (ipVersion === 6) {
    if (host === '::' || host === '::1') {
      return true;
    }
    // IPv4-mapped IPv6 (`::ffff:127.0.0.1`, WHATWG 정규화 시 `::ffff:7f00:1` hex 형태) 는
    // 임베디드 IPv4 를 꺼내 IPv4 규칙으로 재검사 — 그렇지 않으면 loopback / RFC1918 이
    // mapped 리터럴로 가드를 우회한다.
    const mappedIpv4 = extractMappedIpv4(host);
    if (mappedIpv4 !== null && isPrivateHost(mappedIpv4)) {
      return true;
    }
    const firstHextet = host.split(':')[0] ?? '';
    if (firstHextet.length > 0) {
      const value = Number.parseInt(firstHextet, 16);
      if (Number.isFinite(value)) {
        // link-local fe80::/10 → 첫 hextet 의 상위 10 bit 가 1111111010.
        if ((value & 0xffc0) === 0xfe80) {
          return true;
        }
        // ULA fc00::/7 → 첫 hextet 의 상위 7 bit 가 1111110.
        if ((value & 0xfe00) === 0xfc00) {
          return true;
        }
      }
    }
    return false;
  }
  return false;
}

/**
 * IPv4-mapped IPv6 리터럴에서 임베디드 IPv4 (`a.b.c.d`) 를 추출한다. mapped 가 아니면 null.
 * WHATWG URL 파서는 `::ffff:127.0.0.1` 을 hex 형태 `::ffff:7f00:1` 로 정규화하므로 두 표현
 * (dotted `::ffff:a.b.c.d`, hex `::ffff:HHHH:HHHH`) 을 모두 받는다.
 */
function extractMappedIpv4(host: string): string | null {
  const rest = /^::ffff:(.+)$/.exec(host)?.[1];
  if (rest === undefined) {
    return null;
  }
  // dotted 형태면 그대로 IPv4.
  if (isIP(rest) === 4) {
    return rest;
  }
  // hex 형태 `HHHH:HHHH` — 두 hextet 을 32bit 로 합쳐 4 octet 으로 분해.
  const parts = rest.split(':');
  if (parts.length !== 2) {
    return null;
  }
  const hi = Number.parseInt(parts[0] as string, 16);
  const lo = Number.parseInt(parts[1] as string, 16);
  if (
    !Number.isInteger(hi) ||
    !Number.isInteger(lo) ||
    hi < 0 ||
    hi > 0xffff ||
    lo < 0 ||
    lo > 0xffff
  ) {
    return null;
  }
  return `${(hi >> 8) & 0xff}.${hi & 0xff}.${(lo >> 8) & 0xff}.${lo & 0xff}`;
}

/**
 * ogpeek `fetchHtml` 의 `guard` 슬롯에 그대로 꽂을 수 있는 차단 함수.
 * 초기 호출과 모든 redirect hop 직전에 한 번씩 평가된다.
 *
 * `FetchError` 를 던지는 이유: ogpeek 의 `runGuard` 는 throw 된 값이 `FetchError`
 * 인스턴스면 그대로 re-throw 하지만, 그 외 Error 는 `code: "GUARD_FAILED"` 로 다시
 * 감싼다. 호출자에게 한결같은 `code: "BLOCKED"` 가 노출되도록 처음부터 FetchError
 * 로 던진다.
 */
export function makePrivateHostGuard(): (url: URL) => void {
  return (url: URL) => {
    if (isPrivateHost(url.hostname)) {
      throw new FetchError(
        SEO_BLOCKED_CODE,
        400,
        `Refusing to fetch private/loopback host ${url.hostname} — set seo.allowPrivateHosts=true in rocky.json (or pass allowPrivateHosts=true to seo_validate) to override for trusted internal scans.`,
      );
    }
  };
}

/**
 * URL 한 개를 fetch 하고 ogpeek 으로 파싱해 요약 + raw 결과를 돌려준다.
 * SSRF 가드는 `allowPrivateHosts` 가 true 가 아닐 때 자동 적용.
 */
export async function runSeoValidate(opts: SeoValidateOptions): Promise<SeoValidateResult> {
  if (typeof opts.url !== 'string' || opts.url.length === 0) {
    throw new Error('seo_validate: url must be a non-empty string');
  }
  let parsedUrl: URL;
  try {
    parsedUrl = new URL(opts.url);
  } catch {
    throw new Error(`seo_validate: invalid URL ${JSON.stringify(opts.url)}`);
  }
  if (parsedUrl.protocol !== 'http:' && parsedUrl.protocol !== 'https:') {
    throw new Error(`seo_validate: only http/https URLs are supported — got ${parsedUrl.protocol}`);
  }
  const timeoutMs = clampTimeout(opts.timeoutMs);
  const guard = opts.allowPrivateHosts ? undefined : makePrivateHostGuard();

  const { html, finalUrl, redirects } = await fetchHtml(opts.url, {
    timeoutMs,
    guard,
    fetch: opts.fetch,
  });
  const result = parse(html, { url: finalUrl });
  return {
    summary: buildSummary(result, finalUrl, redirects),
    raw: result,
  };
}

function clampTimeout(ms: number | undefined): number | undefined {
  if (ms === undefined) {
    return undefined;
  }
  if (!Number.isFinite(ms) || ms < 1) {
    throw new Error('seo_validate: timeoutMs must be a positive finite number');
  }
  return Math.min(Math.floor(ms), SEO_MAX_TIMEOUT_MS);
}

function buildSummary(
  r: OgDebugResult,
  finalUrl: string,
  redirects: RedirectHop[],
): SeoValidateSummary {
  return {
    finalUrl,
    redirects,
    ogTitle: r.ogp.title ?? null,
    ogDescription: r.ogp.description ?? null,
    ogImage: r.ogp.images[0]?.url ?? null,
    ogType: r.ogp.type ?? null,
    ogUrl: r.ogp.url ?? null,
    canonical: r.meta.canonical,
    errors: r.warnings.filter((w) => w.severity === 'error'),
    warnings: r.warnings.filter((w) => w.severity === 'warn'),
    info: r.warnings.filter((w) => w.severity === 'info'),
    hasJsonLd: r.jsonld.length > 0,
    hasFavicon: r.icons.length > 0,
    iconCount: r.icons.length,
  };
}

/**
 * `seo_validate` 도구 핸들러. 도구 호출 인자가 있으면 그것이 우선이고, 없으면
 * `rocky.json` 의 `seo` 섹션 기본값으로 채운다. 그 외 동작은 `runSeoValidate`
 * 에 전부 위임 — fetch / parse / SSRF 가드 / timeout clamp 까지.
 */
export async function handleSeoValidate(
  seoConfig: SeoConfig | undefined,
  args: {
    url: string;
    timeoutMs?: number;
    allowPrivateHosts?: boolean;
  },
  injectFetch?: SeoValidateOptions['fetch'],
): Promise<SeoValidateResult> {
  const seoCfg = seoConfig ?? {};
  return runSeoValidate({
    url: args.url,
    timeoutMs: args.timeoutMs ?? seoCfg.timeoutMs,
    allowPrivateHosts: args.allowPrivateHosts ?? seoCfg.allowPrivateHosts ?? false,
    fetch: injectFetch,
  });
}
