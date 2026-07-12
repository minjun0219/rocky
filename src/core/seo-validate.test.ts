import { afterEach, beforeEach, describe, expect, it } from 'bun:test';
import { FetchError } from 'ogpeek/fetch';
import {
  handleSeoValidate,
  isPrivateHost,
  runSeoValidate,
  SEO_BLOCKED_CODE,
  SEO_MAX_TIMEOUT_MS,
} from './seo-validate';

describe('isPrivateHost', () => {
  it('recognizes localhost names', () => {
    expect(isPrivateHost('localhost')).toBe(true);
    expect(isPrivateHost('LocalHost')).toBe(true);
    expect(isPrivateHost('api.localhost')).toBe(true);
    expect(isPrivateHost('not-localhost.example')).toBe(false);
  });

  it('recognizes RFC1918 + loopback + link-local IPv4', () => {
    expect(isPrivateHost('127.0.0.1')).toBe(true);
    expect(isPrivateHost('127.255.255.254')).toBe(true);
    expect(isPrivateHost('10.0.0.1')).toBe(true);
    expect(isPrivateHost('172.16.0.1')).toBe(true);
    expect(isPrivateHost('172.31.255.255')).toBe(true);
    expect(isPrivateHost('172.32.0.1')).toBe(false);
    expect(isPrivateHost('172.15.0.1')).toBe(false);
    expect(isPrivateHost('192.168.1.1')).toBe(true);
    expect(isPrivateHost('169.254.1.1')).toBe(true);
    expect(isPrivateHost('0.0.0.0')).toBe(true);
  });

  it('does not flag public IPv4', () => {
    expect(isPrivateHost('8.8.8.8')).toBe(false);
    expect(isPrivateHost('1.1.1.1')).toBe(false);
    expect(isPrivateHost('172.32.0.1')).toBe(false);
    expect(isPrivateHost('169.255.0.1')).toBe(false);
  });

  it('recognizes IPv6 loopback / ULA / link-local', () => {
    expect(isPrivateHost('::1')).toBe(true);
    expect(isPrivateHost('::')).toBe(true);
    expect(isPrivateHost('fe80::1')).toBe(true);
    expect(isPrivateHost('fc00::1')).toBe(true);
    expect(isPrivateHost('fd12:3456::1')).toBe(true);
  });

  it('recognizes the full fe80::/10 link-local range (not just fe80::/16)', () => {
    // fe80–febf 는 모두 link-local. 이전 구현은 fe80: prefix 만 잡아 fe90+ 를 뚫었다.
    expect(isPrivateHost('fe80::1')).toBe(true);
    expect(isPrivateHost('fe90::1')).toBe(true);
    expect(isPrivateHost('fea0::1')).toBe(true);
    expect(isPrivateHost('feb0::1')).toBe(true);
    expect(isPrivateHost('febf::1')).toBe(true);
    // fec0:: 는 link-local 이 아니다 (deprecated site-local) — 차단 대상 아님.
    expect(isPrivateHost('fec0::1')).toBe(false);
  });

  it('blocks IPv4-mapped IPv6 that embeds a private / loopback IPv4', () => {
    // WHATWG URL 은 [::ffff:127.0.0.1] 을 ::ffff:7f00:1 로 정규화한다. dotted / hex 양쪽 검증.
    expect(isPrivateHost('::ffff:127.0.0.1')).toBe(true);
    expect(isPrivateHost('::ffff:7f00:1')).toBe(true); // 127.0.0.1
    expect(isPrivateHost('::ffff:a00:1')).toBe(true); // 10.0.0.1
    expect(isPrivateHost('::ffff:c0a8:101')).toBe(true); // 192.168.1.1
    // URL 파서를 거친 실제 hostname 형태도 확인.
    expect(isPrivateHost(new URL('http://[::ffff:127.0.0.1]/').hostname)).toBe(true);
    expect(isPrivateHost(new URL('http://[::ffff:10.0.0.1]/').hostname)).toBe(true);
  });

  it('does not flag public IPv6', () => {
    expect(isPrivateHost('2001:4860:4860::8888')).toBe(false);
    expect(isPrivateHost('2606:4700:4700::1111')).toBe(false);
    // 공인 IPv4 를 담은 mapped 는 통과시켜야 한다.
    expect(isPrivateHost('::ffff:8.8.8.8')).toBe(false);
    expect(isPrivateHost('::ffff:808:808')).toBe(false); // 8.8.8.8
  });

  it('does not flag external hostnames', () => {
    expect(isPrivateHost('example.com')).toBe(false);
    expect(isPrivateHost('ogp.me')).toBe(false);
  });
});

describe('runSeoValidate', () => {
  let server: ReturnType<typeof Bun.serve>;
  let nextResponse: { html: string; status?: number; contentType?: string };
  let baseUrl: string;

  beforeEach(() => {
    nextResponse = { html: '' };
    server = Bun.serve({
      port: 0,
      hostname: '127.0.0.1',
      fetch() {
        return new Response(nextResponse.html, {
          status: nextResponse.status ?? 200,
          headers: {
            'content-type': nextResponse.contentType ?? 'text/html; charset=utf-8',
          },
        });
      },
    });
    baseUrl = `http://${server.hostname}:${server.port}/`;
  });

  afterEach(() => {
    server.stop(true);
  });

  it('rejects non-http(s) URLs', async () => {
    await expect(
      runSeoValidate({ url: 'ftp://example.com', allowPrivateHosts: true }),
    ).rejects.toThrow(/only http\/https/);
  });

  it('rejects empty / malformed URLs', async () => {
    await expect(runSeoValidate({ url: '' })).rejects.toThrow(/non-empty/);
    await expect(runSeoValidate({ url: 'not-a-url', allowPrivateHosts: true })).rejects.toThrow(
      /invalid URL/,
    );
  });

  it('rejects negative / non-finite timeoutMs', async () => {
    await expect(
      runSeoValidate({ url: baseUrl, timeoutMs: 0, allowPrivateHosts: true }),
    ).rejects.toThrow(/positive finite/);
    await expect(
      runSeoValidate({ url: baseUrl, timeoutMs: Number.NaN, allowPrivateHosts: true }),
    ).rejects.toThrow(/positive finite/);
  });

  it('clamps absurdly large timeoutMs to SEO_MAX_TIMEOUT_MS', async () => {
    nextResponse.html = '<html><head><title>x</title></head></html>';
    // 호출이 성공하면 clamp 가 동작했다는 뜻 (음수 / NaN 만 reject — 큰 수는 silent clamp).
    const result = await runSeoValidate({
      url: baseUrl,
      timeoutMs: SEO_MAX_TIMEOUT_MS * 10,
      allowPrivateHosts: true,
    });
    expect(result.summary.finalUrl).toBe(baseUrl);
  });

  it('blocks private host fetch when allowPrivateHosts is unset', async () => {
    nextResponse.html = '<html></html>';
    let caught: unknown;
    try {
      await runSeoValidate({ url: baseUrl });
    } catch (err) {
      caught = err;
    }
    expect(caught).toBeInstanceOf(FetchError);
    expect((caught as FetchError).code).toBe(SEO_BLOCKED_CODE);
    expect((caught as FetchError).message).toMatch(/Refusing to fetch private/);
  });

  it('blocks private host fetch when allowPrivateHosts is explicitly false', async () => {
    nextResponse.html = '<html></html>';
    let caught: unknown;
    try {
      await runSeoValidate({ url: baseUrl, allowPrivateHosts: false });
    } catch (err) {
      caught = err;
    }
    expect(caught).toBeInstanceOf(FetchError);
    expect((caught as FetchError).code).toBe(SEO_BLOCKED_CODE);
  });

  it('allows private host when allowPrivateHosts: true', async () => {
    nextResponse.html = `<!doctype html>
<html prefix="og: https://ogp.me/ns#">
<head>
  <title>Hello</title>
  <meta property="og:title" content="Hello OG" />
  <meta property="og:type" content="website" />
  <meta property="og:image" content="${baseUrl}cover.png" />
  <meta property="og:url" content="${baseUrl}" />
  <meta property="og:description" content="OG desc" />
  <link rel="canonical" href="${baseUrl}" />
  <link rel="icon" href="${baseUrl}favicon.ico" />
</head>
<body></body>
</html>`;
    const result = await runSeoValidate({ url: baseUrl, allowPrivateHosts: true });
    expect(result.summary.ogTitle).toBe('Hello OG');
    expect(result.summary.ogType).toBe('website');
    expect(result.summary.ogImage).toBe(`${baseUrl}cover.png`);
    expect(result.summary.ogDescription).toBe('OG desc');
    expect(result.summary.ogUrl).toBe(baseUrl);
    expect(result.summary.canonical).toBe(baseUrl);
    expect(result.summary.errors).toEqual([]);
    expect(result.summary.hasFavicon).toBe(true);
    expect(result.summary.iconCount).toBeGreaterThanOrEqual(1);
    expect(result.raw.ogp.title).toBe('Hello OG');
  });

  it('surfaces OG_TITLE_MISSING when og:title is absent', async () => {
    nextResponse.html = `<!doctype html>
<html prefix="og: https://ogp.me/ns#">
<head>
  <title>Plain</title>
  <meta property="og:type" content="website" />
</head>
<body></body>
</html>`;
    const result = await runSeoValidate({ url: baseUrl, allowPrivateHosts: true });
    const codes = result.summary.errors.map((w) => w.code);
    expect(codes).toContain('OG_TITLE_MISSING');
  });

  it('returns redirects array (empty for direct hits)', async () => {
    nextResponse.html = '<html><head><title>x</title></head></html>';
    const result = await runSeoValidate({ url: baseUrl, allowPrivateHosts: true });
    expect(result.summary.redirects).toEqual([]);
  });
});

describe('handleSeoValidate', () => {
  let server: ReturnType<typeof Bun.serve>;
  let baseUrl: string;

  beforeEach(() => {
    server = Bun.serve({
      port: 0,
      hostname: '127.0.0.1',
      fetch() {
        return new Response('<html><head><title>x</title></head></html>', {
          headers: { 'content-type': 'text/html; charset=utf-8' },
        });
      },
    });
    baseUrl = `http://${server.hostname}:${server.port}/`;
  });

  afterEach(() => {
    server.stop(true);
  });

  it('falls back to config seo.allowPrivateHosts when the arg is omitted', async () => {
    // config 가 private host 를 허용 → 인자 없이도 통과.
    const result = await handleSeoValidate({ allowPrivateHosts: true }, { url: baseUrl });
    expect(result.summary.finalUrl).toBe(baseUrl);
  });

  it('blocks private host when neither arg nor config allows it', async () => {
    await expect(handleSeoValidate(undefined, { url: baseUrl })).rejects.toBeInstanceOf(FetchError);
  });

  it('tool arg overrides config default', async () => {
    // config 는 막지만 (allowPrivateHosts:false) 도구 인자가 true 로 이긴다.
    const result = await handleSeoValidate(
      { allowPrivateHosts: false },
      { url: baseUrl, allowPrivateHosts: true },
    );
    expect(result.summary.finalUrl).toBe(baseUrl);
  });
});
