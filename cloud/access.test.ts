import { describe, expect, test } from 'bun:test';
import {
  ACCESS_MESSAGES,
  type AccessConfig,
  AccessError,
  AccessVerifier,
  admit,
  certsUrl,
  importJwks,
  verifyToken,
} from './access';
import {
  AUD,
  b64url,
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

const CONFIG: AccessConfig = { team: TEAM, aud: [AUD] };
const KEY = await rsaKey();
const OTHER_KEY = await rsaKey();
const KEYS = await importJwks(await jwksOf({ k1: KEY }));

/** 거절 이유. 거절하지 않으면 실패한다. */
async function refusal(work: Promise<unknown> | (() => unknown)): Promise<string> {
  try {
    await (typeof work === 'function' ? work() : work);
  } catch (error) {
    expect(error).toBeInstanceOf(AccessError);
    return (error as AccessError).message;
  }
  throw new Error('거절하지 않았다');
}

const verify = (token: string, now = NOW, config = CONFIG) => verifyToken(token, KEYS, config, now);

describe('verifyToken', () => {
  test('사람 토큰은 email로, 서비스 토큰은 common_name으로 주인을 가린다', async () => {
    expect(await verify(await signToken(KEY, personClaims()))).toEqual({
      kind: 'person',
      email: EMAIL,
    });
    expect(await verify(await signToken(KEY, serviceClaims()))).toEqual({
      kind: 'service',
      clientId: CLIENT_ID,
    });
    // 서비스 토큰에는 email이 없다. email이 있으면 사람이다.
    expect(await verify(await signToken(KEY, personClaims({ common_name: CLIENT_ID })))).toEqual({
      kind: 'person',
      email: EMAIL,
    });
  });

  test('주인이 없는 토큰은 거절한다', async () => {
    const token = await signToken(KEY, personClaims({ email: ' ', common_name: '' }));
    expect(await refusal(verify(token))).toBe(ACCESS_MESSAGES.noIdentity);
  });

  test('aud는 문자열이어도 배열이어도 설정한 것 하나만 맞으면 된다', async () => {
    expect(await verify(await signToken(KEY, personClaims({ aud: AUD })))).toMatchObject({
      kind: 'person',
    });
    const both = { team: TEAM, aud: ['aud-old', AUD] };
    const token = await signToken(KEY, personClaims({ aud: ['aud-x', AUD] }));
    expect(await verify(token, NOW, both)).toMatchObject({ kind: 'person' });
    const other = await signToken(KEY, personClaims({ aud: ['aud-x'] }));
    expect(await refusal(verify(other))).toBe(ACCESS_MESSAGES.audience);
  });

  test('RS256이 아니면 키를 찾기 전에 거절한다', async () => {
    const hs = await signToken(KEY, personClaims(), { alg: 'HS256', kid: 'k1' });
    expect(await refusal(verify(hs))).toBe(ACCESS_MESSAGES.algorithm('HS256'));
    const none = await signToken(KEY, personClaims(), { alg: 'none', kid: 'nope' });
    expect(await refusal(verify(none))).toBe(ACCESS_MESSAGES.algorithm('none'));
  });

  test('모르는 kid는 거절한다', async () => {
    const token = await signToken(KEY, personClaims(), { alg: 'RS256', kid: 'k9' });
    expect(await refusal(verify(token))).toBe(ACCESS_MESSAGES.unknownKey('k9'));
  });

  test('서명이 틀리면 클레임을 보지 않고 거절한다', async () => {
    const [header, , signature] = (await signToken(KEY, personClaims())).split('.');
    const forged = `${header}.${b64url(JSON.stringify(personClaims({ email: 'x@evil.test' })))}.${signature}`;
    expect(await refusal(verify(forged))).toBe(ACCESS_MESSAGES.signature);
    // 같은 kid를 단 다른 키의 서명. 클레임이 틀렸어도(발급자) 서명 실패가 먼저다.
    const other = await signToken(OTHER_KEY, personClaims({ iss: 'https://evil.test' }));
    expect(await refusal(verify(other))).toBe(ACCESS_MESSAGES.signature);
  });

  test('발급자가 팀과 다르면 거절한다', async () => {
    const token = await signToken(KEY, personClaims({ iss: 'https://other.cloudflareaccess.com' }));
    expect(await refusal(verify(token))).toBe(
      ACCESS_MESSAGES.issuer('https://other.cloudflareaccess.com'),
    );
  });

  test('만료와 nbf는 60초를 봐준다', async () => {
    const token = await signToken(KEY, personClaims({ exp: NOW, nbf: NOW }));
    expect(await verify(token, NOW + 60)).toMatchObject({ kind: 'person' });
    expect(await refusal(verify(token, NOW + 61))).toBe(ACCESS_MESSAGES.expired);
    expect(await verify(token, NOW - 60)).toMatchObject({ kind: 'person' });
    expect(await refusal(verify(token, NOW - 61))).toBe(ACCESS_MESSAGES.notYetValid);
  });

  test('exp가 없거나 정수가 아니면 모양이 틀린 토큰이다', async () => {
    for (const exp of [undefined, '1800000600', NOW + 0.5]) {
      const token = await signToken(KEY, personClaims({ exp }));
      expect(await refusal(verify(token))).toBe(ACCESS_MESSAGES.malformed);
    }
  });

  test('JWT 모양이 아니면 거절한다', async () => {
    const token = await signToken(KEY, personClaims());
    const [header, payload, signature] = token.split('.');
    for (const bad of [
      '',
      `${header}.${payload}`,
      `${token}.${signature}`,
      `${header}+.${payload}.${signature}`,
      `${b64url('not json')}.${payload}.${signature}`,
      `${b64url('["RS256"]')}.${payload}.${signature}`,
    ]) {
      expect(await refusal(verify(bad))).toBe(ACCESS_MESSAGES.malformed);
    }
  });
});

describe('importJwks', () => {
  test('RSA 키 가운데 모양이 맞고 2048-8192비트인 것만 읽는다', async () => {
    const small = await rsaKey(1024);
    const { keys } = JSON.parse(await jwksOf({ k1: KEY, small })) as {
      keys: Record<string, unknown>[];
    };
    const body = JSON.stringify({
      keys: [
        ...keys,
        { kid: 'ec', kty: 'EC', crv: 'P-256', x: 'AA', y: 'AA' },
        { kty: 'RSA', n: keys[0]?.n, e: 'AQAB' },
        { kid: 'bad-n', kty: 'RSA', n: 'not+base64url', e: 'AQAB' },
        null,
      ],
    });
    expect([...(await importJwks(body)).keys()]).toEqual(['k1']);
  });

  test('문서가 JSON이 아니거나 keys가 없으면 빈 목록이다', async () => {
    expect((await importJwks('<html>')).size).toBe(0);
    expect((await importJwks('{"keys":{}}')).size).toBe(0);
    expect((await importJwks('null')).size).toBe(0);
  });
});

describe('admit', () => {
  const admission = { emails: ['Minjun@Example.com'], daemonClientId: CLIENT_ID };

  test('폰 길은 허용 이메일의 사람만 들인다(ASCII 대소문자만 무시)', async () => {
    admit({ kind: 'person', email: EMAIL }, 'phone', admission);
    expect(
      await refusal(() => admit({ kind: 'person', email: 'x@example.com' }, 'phone', admission)),
    ).toBe(ACCESS_MESSAGES.notAllowed('x@example.com'));
    // 켈빈 기호(U+212A)는 toLowerCase로 접으면 k가 된다. 데몬처럼 다른 글자로 본다.
    const kelvin = { emails: ['kim@example.com'], daemonClientId: CLIENT_ID };
    expect(
      await refusal(() =>
        admit({ kind: 'person', email: '\u212Aim@example.com' }, 'phone', kelvin),
      ),
    ).toBe(ACCESS_MESSAGES.notAllowed('\u212Aim@example.com'));
  });

  test('데몬 길은 설정한 Client ID의 서비스 토큰만 들인다', async () => {
    admit({ kind: 'service', clientId: CLIENT_ID }, 'daemon', admission);
    expect(
      await refusal(() =>
        admit({ kind: 'service', clientId: 'other.access' }, 'daemon', admission),
      ),
    ).toBe(ACCESS_MESSAGES.otherService('other.access'));
  });

  test('사람의 토큰으로 /relay에, 서비스 토큰으로 /api/ws에 온 것은 들이지 않는다', async () => {
    expect(await refusal(() => admit({ kind: 'person', email: EMAIL }, 'daemon', admission))).toBe(
      ACCESS_MESSAGES.personOnRelay,
    );
    expect(
      await refusal(() => admit({ kind: 'service', clientId: CLIENT_ID }, 'phone', admission)),
    ).toBe(ACCESS_MESSAGES.serviceOnPhone);
  });
});

describe('AccessVerifier', () => {
  function setup(documents: (() => Promise<string>)[]) {
    let clock = NOW * 1000;
    const fetched: string[] = [];
    const logs: string[] = [];
    const verifier = new AccessVerifier(CONFIG, {
      fetchCerts: (url) => {
        fetched.push(url);
        const next = documents[Math.min(fetched.length, documents.length) - 1];
        return next ? next() : Promise.reject(new Error('no document'));
      },
      now: () => clock,
      log: (line) => logs.push(line),
    });
    return {
      verifier,
      fetched,
      logs,
      tick(ms: number) {
        clock += ms;
      },
    };
  }
  const jwks = (keys: Record<string, CryptoKeyPair>) => () => jwksOf(keys);
  /** 그 kid를 머리에 단 사람 토큰. 10분 동안 유효하다. */
  const tokenOf = (kid: string, pair = KEY) =>
    signToken(pair, personClaims(), { alg: 'RS256', kid });

  test('공개키는 팀 주소에서 한 번 받아 한 시간 쓴다', async () => {
    const s = setup([jwks({ k1: KEY })]);
    const token = await tokenOf('k1');
    expect(await s.verifier.identify(token)).toMatchObject({ kind: 'person' });
    expect(await s.verifier.identify(token)).toMatchObject({ kind: 'person' });
    expect(s.fetched).toEqual([certsUrl(TEAM)]);
    expect(certsUrl(TEAM)).toBe('https://rocky-test.cloudflareaccess.com/cdn-cgi/access/certs');
    s.tick(60 * 60 * 1000 + 1);
    await s.verifier.identify(await signToken(KEY, personClaims({ exp: NOW + 7200 })));
    expect(s.fetched).toHaveLength(2);
  });

  test('모르는 kid가 오면 다시 받되 1분에 한 번까지다', async () => {
    const k2 = await rsaKey();
    const s = setup([jwks({ k1: KEY }), jwks({ k1: KEY, k2 })]);
    await s.verifier.identify(await tokenOf('k1'));
    // Cloudflare가 키를 돌렸다. 처음 보는 kid라 다시 받는다(지난번 받기에서 1분이 지났다).
    s.tick(60 * 1000 + 1);
    expect(await s.verifier.identify(await tokenOf('k2', k2))).toMatchObject({
      kind: 'person',
    });
    expect(s.fetched).toHaveLength(2);
    // 지어낸 kid. 1분 안에는 다시 받지 않는다.
    const made = await tokenOf('k9');
    expect(await refusal(s.verifier.identify(made))).toBe(ACCESS_MESSAGES.unknownKey('k9'));
    expect(s.fetched).toHaveLength(2);
    s.tick(60 * 1000 + 1);
    expect(await refusal(s.verifier.identify(made))).toBe(ACCESS_MESSAGES.unknownKey('k9'));
    expect(s.fetched).toHaveLength(3);
  });

  test('동시에 온 요청은 공개키를 한 번만 받고, 받으러 간 요청이 받기를 끝까지 붙든다', async () => {
    const s = setup([jwks({ k1: KEY })]);
    const token = await tokenOf('k1');
    const held: Promise<unknown>[] = [];
    const all = await Promise.all(
      [1, 2, 3].map(() => s.verifier.identify(token, (promise) => held.push(promise))),
    );
    expect(all.map((identity) => identity.kind)).toEqual(['person', 'person', 'person']);
    expect(s.fetched).toHaveLength(1);
    expect(held).toHaveLength(1);
  });

  test('공개키를 못 받으면 로그에 남기고 다음에 다시 받는다', async () => {
    const s = setup([() => Promise.reject(new Error('HTTP 503')), jwks({ k1: KEY })]);
    const token = await tokenOf('k1');
    expect(await refusal(s.verifier.identify(token))).toBe(ACCESS_MESSAGES.unknownKey('k1'));
    expect(s.logs).toEqual([
      `rocky relay: Access 공개키를 못 받았다 — ${certsUrl(TEAM)}: HTTP 503`,
    ]);
    s.tick(60 * 1000 + 1);
    expect(await s.verifier.identify(token)).toMatchObject({ kind: 'person' });
  });

  test('모양이 틀린 토큰은 공개키를 받으러 가지 않는다', async () => {
    const s = setup([jwks({ k1: KEY })]);
    expect(await refusal(s.verifier.identify('garbage'))).toBe(ACCESS_MESSAGES.malformed);
    expect(s.fetched).toHaveLength(0);
  });
});
