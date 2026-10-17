/**
 * Cloudflare Access 토큰 검증 — Worker 판. 규칙은 데몬의 `rocky_core::access::verify_token`과 같다. RS256만, 팀 공개키로
 * 서명을 먼저 보고, 그다음 `iss`·`aud`·`exp`·`nbf`(60초 허용)를 맞춘다. 헤더 이름만 보고 믿지 않는다.
 *
 * 데몬과 다른 점은 하나다. 데몬은 사람만 받지만 중계소는 서비스 토큰(데몬 연결기)도 받는다. 그래서 토큰의 주인을
 * 사람(`email`)과 서비스 토큰(`common_name`에 실린 Client ID)으로 가르고, 길마다 누구를 들일지는 `admit`이 정한다. 설계는
 * `docs/design/specs/2026-10-15-dual-structure-sync-design.md`의 3번 조각 "인증".
 *
 * 공개키는 데몬(`rockyd::access::AccessGate`)처럼 한 시간 동안 쓰고, 모르는 `kid`가 오면 다시 받되 1분에 한 번까지다.
 */

/** 토큰 검증에 쓰는 Worker 설정. */
export interface AccessConfig {
  /** 팀 이름. 발급자(`iss`)와 공개키 주소가 `<team>.cloudflareaccess.com`에서 나온다. */
  team: string;
  /** Access 애플리케이션의 AUD 태그. 하나라도 토큰의 `aud`에 있어야 한다. */
  aud: string[];
}

/** 서명과 클레임이 맞은 토큰의 주인. 서비스 토큰은 `common_name`에 Client ID가 실린다. */
export type AccessIdentity =
  | { kind: 'person'; email: string }
  | { kind: 'service'; clientId: string };

/** 중계소의 두 길. 폰은 `/api/ws`, 데몬은 `/relay`로 붙는다. */
export type RelayRole = 'phone' | 'daemon';

/** 길마다 들일 주인. */
export interface Admission {
  /** 폰으로 들일 이메일(ASCII 대소문자 무시). */
  emails: string[];
  /** 데몬 연결기의 서비스 토큰 Client ID. */
  daemonClientId: string;
}

/** 검증 실패. 로그와 응답에 남길 수 있게 토큰 내용은 담지 않는다. */
export class AccessError extends Error {}

/** 데몬(`rocky_core::access::AccessError`)과 같은 실패는 같은 문구다. */
export const ACCESS_MESSAGES = {
  malformed: '토큰 모양이 JWT 가 아니다',
  algorithm: (alg: string) => `서명 방식 ${alg} 은 받지 않는다(RS256 만)`,
  unknownKey: (kid: string) => `공개키 ${kid} 를 모른다`,
  signature: '서명이 맞지 않는다',
  issuer: (iss: string) => `발급자가 다르다: ${iss}`,
  audience: 'Access 애플리케이션(aud)이 다르다',
  expired: '만료된 토큰이다',
  notYetValid: '아직 유효하지 않은 토큰이다',
  notAllowed: (email: string) => `허용 목록에 없는 사람이다: ${email}`,
  noIdentity: '토큰에 사람(email)도 서비스 토큰(common_name)도 없다',
  serviceOnPhone: '서비스 토큰으로는 /api/ws 에 붙지 않는다 — 폰은 Access 로그인으로 붙는다',
  personOnRelay: '사람의 토큰으로는 /relay 에 붙지 않는다 — 데몬은 서비스 토큰으로 붙는다',
  otherService: (clientId: string) => `설정한 데몬의 서비스 토큰이 아니다: ${clientId}`,
};

/** 시계가 조금 어긋나도 받아 주는 폭(초). */
const CLOCK_SKEW_SECS = 60;
/** 데몬의 `ring::signature::RSA_PKCS1_2048_8192_SHA256`과 같은 키 길이만 받는다. */
const MIN_MODULUS_BITS = 2048;
const MAX_MODULUS_BITS = 8192;
const RSA = { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' };

/** 토큰의 `iss`가 되어야 하는 값. */
export function issuer(team: string): string {
  return `https://${team}.cloudflareaccess.com`;
}

/** 공개키 목록 주소. */
export function certsUrl(team: string): string {
  return `${issuer(team)}/cdn-cgi/access/certs`;
}

function malformed(): AccessError {
  return new AccessError(ACCESS_MESSAGES.malformed);
}

/**
 * ASCII 글자만 소문자로 접는다. 데몬의 `eq_ignore_ascii_case`와 같게 — `toLowerCase`는 켈빈 기호(U+212A)를 `k`로 접어 다른
 * 이메일을 허용 목록의 것으로 볼 수 있다.
 */
function asciiLower(value: string): string {
  return value.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
}

/** base64url(끝의 `=`는 떼고 본다). */
function b64url(part: string): Uint8Array<ArrayBuffer> {
  const bare = part.replace(/=+$/, '');
  if (!/^[A-Za-z0-9_-]*$/.test(bare) || bare.length % 4 === 1) {
    throw malformed();
  }
  const padded = bare
    .replaceAll('-', '+')
    .replaceAll('_', '/')
    .padEnd(Math.ceil(bare.length / 4) * 4, '=');
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** JWT 조각 하나를 JSON 객체로. */
function jsonPart(part: string): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(b64url(part)));
  } catch {
    throw malformed();
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw malformed();
  }
  return value as Record<string, unknown>;
}

/** 모듈러스의 비트 수(앞의 0 바이트는 빼고). */
function modulusBits(n: Uint8Array): number {
  let start = 0;
  while (start < n.length && n[start] === 0) {
    start++;
  }
  const top = n[start];
  return top === undefined ? 0 : (n.length - start - 1) * 8 + (32 - Math.clz32(top));
}

/**
 * JWKS 문서(`{"keys":[{kid,kty,n,e,…}]}`)에서 RSA 키만 읽어 들인다. 모양이 틀렸거나 길이가 2048-8192비트 밖인 항목은
 * 건너뛴다.
 */
export async function importJwks(body: string): Promise<Map<string, CryptoKey>> {
  const keys = new Map<string, CryptoKey>();
  let doc: unknown;
  try {
    doc = JSON.parse(body);
  } catch {
    return keys;
  }
  const entries = (doc as { keys?: unknown } | null)?.keys;
  if (!Array.isArray(entries)) {
    return keys;
  }
  for (const entry of entries) {
    const { kid, kty, n, e } = (entry ?? {}) as Record<string, unknown>;
    if (
      kty !== 'RSA' ||
      typeof kid !== 'string' ||
      typeof n !== 'string' ||
      typeof e !== 'string'
    ) {
      continue;
    }
    try {
      const bits = modulusBits(b64url(n));
      if (bits < MIN_MODULUS_BITS || bits > MAX_MODULUS_BITS) {
        continue;
      }
      const jwk = { kty: 'RSA', n: n.replace(/=+$/, ''), e: e.replace(/=+$/, '') };
      keys.set(kid, await crypto.subtle.importKey('jwk', jwk, RSA, false, ['verify']));
    } catch {
      // 읽지 못한 키는 없는 키다
    }
  }
  return keys;
}

/** 토큰 머리의 `kid`. 공개키를 고르거나, 모르는 키면 목록을 다시 받을지 정한다. */
export function tokenKid(token: string): string | null {
  const [header = ''] = token.trim().split('.');
  try {
    const { kid } = jsonPart(header);
    return typeof kid === 'string' ? kid : null;
  } catch {
    return null;
  }
}

/** 토큰을 검증하고 주인을 돌려준다. `now`는 유닉스 초. 실패는 `AccessError`. */
export async function verifyToken(
  token: string,
  keys: ReadonlyMap<string, CryptoKey>,
  config: AccessConfig,
  now: number,
): Promise<AccessIdentity> {
  const [headerB64, payloadB64, signatureB64, ...rest] = token.trim().split('.');
  if (headerB64 === undefined || payloadB64 === undefined || signatureB64 === undefined) {
    throw malformed();
  }
  if (rest.length > 0) {
    throw malformed();
  }
  const header = jsonPart(headerB64);
  const alg = typeof header.alg === 'string' ? header.alg : '';
  if (alg !== 'RS256') {
    throw new AccessError(ACCESS_MESSAGES.algorithm(alg));
  }
  if (typeof header.kid !== 'string') {
    throw malformed();
  }
  const key = keys.get(header.kid);
  if (!key) {
    throw new AccessError(ACCESS_MESSAGES.unknownKey(header.kid));
  }
  // 서명 먼저 — 클레임은 서명이 맞은 뒤에야 믿을 수 있다.
  const signature = b64url(signatureB64);
  const signed = new TextEncoder().encode(`${headerB64}.${payloadB64}`);
  if (!(await crypto.subtle.verify(RSA.name, key, signature, signed))) {
    throw new AccessError(ACCESS_MESSAGES.signature);
  }
  const claims = jsonPart(payloadB64);
  const iss = typeof claims.iss === 'string' ? claims.iss : '';
  if (iss !== issuer(config.team)) {
    throw new AccessError(ACCESS_MESSAGES.issuer(iss));
  }
  const audiences =
    typeof claims.aud === 'string'
      ? [claims.aud]
      : Array.isArray(claims.aud)
        ? claims.aud.filter((aud): aud is string => typeof aud === 'string')
        : [];
  if (!audiences.some((aud) => config.aud.includes(aud))) {
    throw new AccessError(ACCESS_MESSAGES.audience);
  }
  const { exp, nbf } = claims;
  if (typeof exp !== 'number' || !Number.isSafeInteger(exp)) {
    throw malformed();
  }
  if (now > exp + CLOCK_SKEW_SECS) {
    throw new AccessError(ACCESS_MESSAGES.expired);
  }
  if (typeof nbf === 'number' && Number.isSafeInteger(nbf) && now + CLOCK_SKEW_SECS < nbf) {
    throw new AccessError(ACCESS_MESSAGES.notYetValid);
  }
  const email = typeof claims.email === 'string' ? claims.email.trim() : '';
  if (email !== '') {
    return { kind: 'person', email };
  }
  const clientId = typeof claims.common_name === 'string' ? claims.common_name.trim() : '';
  if (clientId !== '') {
    return { kind: 'service', clientId };
  }
  throw new AccessError(ACCESS_MESSAGES.noIdentity);
}

/**
 * 이 길에 들일 주인인가. 폰은 허용 이메일의 사람만, 데몬은 설정한 Client ID의 서비스 토큰만 들인다. 사람의 토큰으로
 * `/relay`에, 서비스 토큰으로 `/api/ws`에 온 것은 소켓을 열지 않는다. 아니면 `AccessError`.
 */
export function admit(identity: AccessIdentity, role: RelayRole, admission: Admission): void {
  if (role === 'phone') {
    if (identity.kind !== 'person') {
      throw new AccessError(ACCESS_MESSAGES.serviceOnPhone);
    }
    const email = asciiLower(identity.email);
    if (!admission.emails.some((allowed) => asciiLower(allowed) === email)) {
      throw new AccessError(ACCESS_MESSAGES.notAllowed(identity.email));
    }
    return;
  }
  if (identity.kind !== 'service') {
    throw new AccessError(ACCESS_MESSAGES.personOnRelay);
  }
  if (identity.clientId !== admission.daemonClientId) {
    throw new AccessError(ACCESS_MESSAGES.otherService(identity.clientId));
  }
}

/** 공개키 문서를 받아 온다(주소 → 본문). 테스트가 갈아 끼운다. */
export type CertsFetcher = (url: string) => Promise<string>;

/** 기본 공개키 받기. 5초 제한. */
export const httpCerts: CertsFetcher = async (url) => {
  const response = await fetch(url, { signal: AbortSignal.timeout(5_000) });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.text();
};

const KEYS_TTL_MS = 60 * 60 * 1000;
const MIN_REFETCH_MS = 60 * 1000;

export interface VerifierOptions {
  fetchCerts: CertsFetcher;
  now?: () => number;
  log?: (line: string) => void;
}

/** 요청이 끝나거나 손님이 끊겨도 마칠 일을 맡긴다. Workers의 `ctx.waitUntil`. */
export type WaitUntil = (promise: Promise<unknown>) => void;

/** 공개키를 받아 두고 토큰을 검증한다. Worker 격리 하나에 하나씩 둔다. */
export class AccessVerifier {
  private keys: ReadonlyMap<string, CryptoKey> = new Map();
  private fetchedAt: number | null = null;
  private attemptedAt: number | null = null;
  /** 받는 중인 공개키. 동시에 온 요청이 같이 받으러 가지 않게 하나만 둔다. */
  private loading: Promise<void> | null = null;
  private readonly now: () => number;
  private readonly log: (line: string) => void;

  constructor(
    private readonly config: AccessConfig,
    private readonly options: VerifierOptions,
  ) {
    this.now = options.now ?? Date.now;
    this.log = options.log ?? ((line) => console.warn(line));
  }

  /** 토큰을 검증하고 주인을 돌려준다. 실패는 `AccessError`. */
  async identify(token: string, waitUntil?: WaitUntil): Promise<AccessIdentity> {
    const kid = tokenKid(token);
    if (kid === null) {
      throw malformed();
    }
    const now = this.now();
    const stale = this.fetchedAt === null || now - this.fetchedAt > KEYS_TTL_MS;
    if (stale || !this.keys.has(kid)) {
      // 아무 `kid`나 지어 보내는 요청이 매번 Cloudflare를 두드리지 않게 다시 받기는 1분에 한 번까지.
      const mayTry = this.attemptedAt === null || now - this.attemptedAt > MIN_REFETCH_MS;
      if (this.loading === null && mayTry) {
        this.attemptedAt = now;
        this.loading = this.load().finally(() => {
          this.loading = null;
        });
        // Workers는 손님이 끊긴 요청의 일을 거둔다. 받기가 거둬지면 같이 기다리던 요청이 풀리지 않으니 끝까지 붙든다.
        waitUntil?.(this.loading);
      }
      if (this.loading !== null) {
        await this.loading;
      }
    }
    return verifyToken(token, this.keys, this.config, Math.floor(this.now() / 1000));
  }

  private async load(): Promise<void> {
    const url = certsUrl(this.config.team);
    try {
      const keys = await importJwks(await this.options.fetchCerts(url));
      if (keys.size === 0) {
        this.log(`rocky relay: Access 공개키 문서에 RSA 키가 없다 — ${url}`);
        return;
      }
      this.keys = keys;
      this.fetchedAt = this.now();
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      this.log(`rocky relay: Access 공개키를 못 받았다 — ${url}: ${reason}`);
    }
  }
}
