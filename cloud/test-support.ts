/**
 * Worker 테스트 도구. Access 토큰은 테스트마다 만든 RSA 키로 서명하고, DO의 소켓과 `ctx`는 가짜로 흉내 낸다.
 */
import { issuer } from './access';
import type { HostContext, HostSocket } from './host';
import { memoryStorage } from './relay/test-support';

export const TEAM = 'rocky-test';
export const AUD = 'aud-relay';
export const EMAIL = 'minjun@example.com';
export const CLIENT_ID = 'client-1.access';
/** 토큰 시각의 기준(유닉스 초). */
export const NOW = 1_800_000_000;

export function b64url(data: Uint8Array | string): string {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
}

export function rsaKey(bits = 2048): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey(
    {
      name: 'RSASSA-PKCS1-v1_5',
      modulusLength: bits,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: 'SHA-256',
    },
    true,
    ['sign', 'verify'],
  );
}

/** Cloudflare 공개키 문서 모양(`/cdn-cgi/access/certs`). */
export async function jwksOf(entries: Record<string, CryptoKeyPair>): Promise<string> {
  const keys = [];
  for (const [kid, pair] of Object.entries(entries)) {
    const { n, e } = await crypto.subtle.exportKey('jwk', pair.publicKey);
    keys.push({ kid, kty: 'RSA', alg: 'RS256', use: 'sig', n, e });
  }
  return JSON.stringify({ keys });
}

export async function signToken(
  pair: CryptoKeyPair,
  claims: Record<string, unknown>,
  header: Record<string, unknown> = { alg: 'RS256', kid: 'k1' },
): Promise<string> {
  const input = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(claims))}`;
  const signature = await crypto.subtle.sign(
    'RSASSA-PKCS1-v1_5',
    pair.privateKey,
    new TextEncoder().encode(input),
  );
  return `${input}.${b64url(new Uint8Array(signature))}`;
}

/** Access 로그인으로 받은 사람의 토큰 클레임. */
export function personClaims(over: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    type: 'app',
    iss: issuer(TEAM),
    aud: [AUD],
    iat: NOW,
    exp: NOW + 600,
    sub: 'user-1',
    email: EMAIL,
    ...over,
  };
}

/** 서비스 토큰으로 받은 토큰 클레임. `email`이 없고 `common_name`에 Client ID가 실린다. */
export function serviceClaims(over: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    type: 'app',
    iss: issuer(TEAM),
    aud: [AUD],
    iat: NOW,
    exp: NOW + 600,
    sub: '',
    common_name: CLIENT_ID,
    ...over,
  };
}

/** DO 소켓. 첨부는 DO처럼 직렬화해 둔다. */
export class FakeSocket implements HostSocket {
  sent: string[] = [];
  closed: { code?: number; reason?: string } | null = null;
  private attachment: string | null = null;

  send(message: string): void {
    if (this.closed) {
      throw new Error('socket closed');
    }
    this.sent.push(message);
  }

  close(code?: number, reason?: string): void {
    this.closed = { code, reason };
  }

  serializeAttachment(value: unknown): void {
    this.attachment = JSON.stringify(value);
  }

  deserializeAttachment(): unknown {
    return this.attachment === null ? null : JSON.parse(this.attachment);
  }

  /** 지금까지 받은 프레임을 꺼내고 비운다. */
  take(): Record<string, unknown>[] {
    const out = this.sent.map((text) => JSON.parse(text) as Record<string, unknown>);
    this.sent = [];
    return out;
  }
}

/** DO의 `ctx`. 저장소와 받은 소켓은 DO가 잠들었다 깨도 남는다. */
export function fakeContext(): HostContext & { sockets: FakeSocket[] } {
  const sockets: FakeSocket[] = [];
  return {
    sockets,
    storage: memoryStorage(),
    acceptWebSocket: (ws) => {
      sockets.push(ws as FakeSocket);
    },
    getWebSockets: () => sockets.filter((socket) => socket.closed === null),
  };
}
