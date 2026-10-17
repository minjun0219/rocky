import { describe, expect, test } from 'bun:test';
import { ACCESS_MESSAGES } from './access';
import { createGate, GATE_MESSAGES, isSameOrigin, type RelayEnv } from './gate';
import { whoFrom } from './host';
import {
  AUD,
  CLIENT_ID,
  EMAIL,
  jwksOf,
  NOW,
  personClaims,
  rsaKey,
  serviceClaims,
  signToken,
  TEAM,
} from './test-support';

const KEY = await rsaKey();
const JWKS = await jwksOf({ k1: KEY });
const PERSON = await signToken(KEY, personClaims());
const SERVICE = await signToken(KEY, serviceClaims());
const HOST = 'board.example.com';

function setup(env: Partial<RelayEnv> = {}) {
  const forwarded: { name: string; request: Request }[] = [];
  const logs: string[] = [];
  const held: Promise<unknown>[] = [];
  const gate = createGate({
    fetchCerts: async () => JWKS,
    now: () => NOW * 1000,
    log: (line) => logs.push(line),
  });
  const fullEnv: RelayEnv = {
    RELAY: {
      getByName: (name) => ({
        fetch: async (request) => {
          forwarded.push({ name, request });
          return new Response('relay');
        },
      }),
    },
    ACCESS_TEAM: TEAM,
    ACCESS_AUD: AUD,
    ACCESS_EMAILS: `other@example.com, ${EMAIL}`,
    DAEMON_CLIENT_ID: CLIENT_ID,
    ...env,
  };
  return {
    forwarded,
    logs,
    held,
    /** 이 길로 온 소켓 업그레이드. */
    open(path: string, headers: Record<string, string> = {}, init: RequestInit = {}) {
      return gate(
        new Request(`https://${HOST}${path}`, {
          ...init,
          headers: { upgrade: 'websocket', connection: 'Upgrade', ...headers },
        }),
        fullEnv,
        { waitUntil: (promise) => held.push(promise) },
      );
    },
  };
}

async function errorOf(response: Response): Promise<string> {
  return ((await response.json()) as { error: string }).error;
}

describe('길과 업그레이드', () => {
  test('없는 길은 404, 웹소켓 업그레이드가 아니면 426이다', async () => {
    const s = setup();
    for (const path of ['/', '/api/ws/x', '/relay/', '/api/todos']) {
      expect((await s.open(path, { 'cf-access-jwt-assertion': SERVICE })).status).toBe(404);
    }
    const post = await s.open('/relay', { 'cf-access-jwt-assertion': SERVICE }, { method: 'POST' });
    expect(post.status).toBe(426);
    const plain = await s.open('/relay', { upgrade: '', 'cf-access-jwt-assertion': SERVICE });
    expect(plain.status).toBe(426);
    expect(await errorOf(plain)).toBe(GATE_MESSAGES.notUpgrade);
    expect(s.forwarded).toHaveLength(0);
  });

  test('secret이 비면 500으로 빈 이름을 알리고 소켓을 열지 않는다', async () => {
    const s = setup({ ACCESS_AUD: ' , ', DAEMON_CLIENT_ID: undefined });
    const response = await s.open('/relay', { 'cf-access-jwt-assertion': SERVICE });
    expect(response.status).toBe(500);
    expect(await errorOf(response)).toBe(GATE_MESSAGES.missing(['ACCESS_AUD', 'DAEMON_CLIENT_ID']));
    expect(s.forwarded).toHaveLength(0);
  });

  test('팀 이름 자리에 주소를 적으면 공개키를 받으러 가지 않는다', async () => {
    const s = setup({ ACCESS_TEAM: 'evil.test/x?' });
    const response = await s.open('/relay', { 'cf-access-jwt-assertion': SERVICE });
    expect(response.status).toBe(500);
    expect(await errorOf(response)).toBe(GATE_MESSAGES.badTeam);
  });

  test('Access 토큰이 없으면 소켓을 열지 않는다', async () => {
    const s = setup();
    const response = await s.open('/relay');
    expect(response.status).toBe(403);
    expect(await errorOf(response)).toBe(GATE_MESSAGES.noToken);
  });
});

describe('데몬 길(/relay)', () => {
  test('설정한 서비스 토큰이면 중계소 DO로 데몬 역할을 넘긴다', async () => {
    const s = setup();
    const response = await s.open('/relay?actor=minjun', { 'cf-access-jwt-assertion': SERVICE });
    expect(await response.text()).toBe('relay');
    expect(s.forwarded).toHaveLength(1);
    const [{ name, request }] = s.forwarded as [{ name: string; request: Request }];
    expect(name).toBe('relay');
    expect(whoFrom(request.url)).toEqual({ role: 'daemon' });
    // 처음 받는 공개키는 요청의 waitUntil에 맡긴다.
    expect(s.held).toHaveLength(1);
    // DO가 웹소켓을 돌려주려면 업그레이드 헤더가 그대로 가야 한다.
    expect(request.headers.get('upgrade')).toBe('websocket');
  });

  test('사람의 토큰, 다른 서비스 토큰, 틀린 서명은 거절하고 이유를 남긴다', async () => {
    const s = setup();
    const other = await signToken(KEY, serviceClaims({ common_name: 'other.access' }));
    const forged = `${SERVICE.slice(0, SERVICE.lastIndexOf('.'))}.${PERSON.split('.')[2]}`;
    const cases: [string, string][] = [
      [PERSON, ACCESS_MESSAGES.personOnRelay],
      [other, ACCESS_MESSAGES.otherService('other.access')],
      [forged, ACCESS_MESSAGES.signature],
    ];
    for (const [token, reason] of cases) {
      const response = await s.open('/relay', { 'cf-access-jwt-assertion': token });
      expect(response.status).toBe(403);
      expect(await errorOf(response)).toBe(reason);
    }
    expect(s.forwarded).toHaveLength(0);
    expect(s.logs).toEqual(
      cases.map(([, reason]) => `rocky relay: /relay 소켓을 열지 않음 — ${reason}`),
    );
  });
});

describe('폰 길(/api/ws)', () => {
  test('허용 이메일의 사람이면 소켓 주소의 actor를 붙여 넘긴다', async () => {
    const s = setup();
    await s.open('/api/ws?actor=%20%EB%AF%BC%EC%A4%80%20', { 'cf-access-jwt-assertion': PERSON });
    await s.open('/api/ws', { 'cf-access-jwt-assertion': PERSON });
    expect(s.forwarded.map(({ request }) => whoFrom(request.url))).toEqual([
      { role: 'phone', actor: '민준' },
      { role: 'phone', actor: 'unknown' },
    ]);
  });

  test('서비스 토큰과 허용 목록 밖의 사람은 거절한다', async () => {
    const s = setup();
    const stranger = await signToken(KEY, personClaims({ email: 'x@example.com' }));
    const service = await s.open('/api/ws', { 'cf-access-jwt-assertion': SERVICE });
    expect(await errorOf(service)).toBe(ACCESS_MESSAGES.serviceOnPhone);
    const other = await s.open('/api/ws', { 'cf-access-jwt-assertion': stranger });
    expect(await errorOf(other)).toBe(ACCESS_MESSAGES.notAllowed('x@example.com'));
    expect(s.forwarded).toHaveLength(0);
  });

  test('다른 출처의 페이지가 연 소켓은 토큰이 맞아도 열지 않는다', async () => {
    const s = setup();
    const crossOrigin: Record<string, string>[] = [
      { 'sec-fetch-site': 'cross-site' },
      // 같은 등록 도메인의 다른 서브도메인도 Access 쿠키를 싣고 온다.
      { 'sec-fetch-site': 'same-site', origin: 'https://other.example.com' },
      { origin: 'https://evil.test' },
      { origin: 'null' },
    ];
    for (const headers of crossOrigin) {
      const response = await s.open('/api/ws', { ...headers, 'cf-access-jwt-assertion': PERSON });
      expect(response.status).toBe(403);
      expect(await errorOf(response)).toBe(GATE_MESSAGES.crossOrigin);
    }
    await s.open('/api/ws', {
      'sec-fetch-site': 'same-origin',
      origin: `https://${HOST}`,
      'cf-access-jwt-assertion': PERSON,
    });
    expect(s.forwarded).toHaveLength(1);
  });
});

describe('isSameOrigin', () => {
  const headers = (init: Record<string, string>) => new Headers(init);

  test('Sec-Fetch-Site가 있으면 same-origin만 같은 출처다', () => {
    expect(isSameOrigin(headers({ 'sec-fetch-site': 'same-origin' }), HOST)).toBe(true);
    expect(isSameOrigin(headers({ 'sec-fetch-site': 'none' }), HOST)).toBe(false);
  });

  test('없으면 Origin의 호스트[:포트]가 요청 호스트와 같아야 하고, 둘 다 없으면 브라우저가 아니다', () => {
    expect(isSameOrigin(headers({ origin: `https://${HOST.toUpperCase()}` }), HOST)).toBe(true);
    expect(isSameOrigin(headers({ origin: `https://${HOST}:8443` }), HOST)).toBe(false);
    expect(isSameOrigin(headers({ origin: `https://${HOST}:8443` }), `${HOST}:8443`)).toBe(true);
    expect(isSameOrigin(headers({}), HOST)).toBe(true);
  });
});
