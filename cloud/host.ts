/**
 * 중계소 DO의 소켓 배선. Worker가 정한 역할(데몬·폰)로 소켓을 받아 `Relay`에 잇는다. DO 런타임에 묶인 것
 * (`cloudflare:workers`, `WebSocketPair`)은 `worker.ts`에 두고, 여기는 그 모양만 받아 테스트가 가짜로 갈아끼운다.
 *
 * 상태는 `Relay`처럼 SQLite와 소켓 첨부(`serializeAttachment`)에만 둔다. DO가 잠들었다 깨면 이 객체도 새로 만들어진다.
 */
import { type Peer, type PeerState, Relay } from './relay/relay';
import type { RelayStorage } from './relay/storage';

/**
 * 웹(`web/sync/client.ts`)과 데몬 연결기가 살았나 묻는 프레임 그대로. DO가 깨지 않고 `PONG`으로 답한다
 * (`setWebSocketAutoResponse`). 글자 하나라도 다르면 자동 응답이 받지 않아 물을 때마다 DO가 깬다.
 */
export const PING = '{"t":"ping"}';
export const PONG = '{"t":"pong"}';

/** 소켓의 주인. Worker가 Access 토큰으로 정한다. */
export type Who = { role: 'daemon' } | { role: 'phone'; actor: string };

/** Worker가 DO에 넘기는 요청의 주소. DO는 Worker 바인딩으로만 닿으니 이 주소를 믿는다. */
const HOST_ORIGIN = 'https://relay.internal';

export function hostUrl(who: Who): string {
  return who.role === 'daemon'
    ? `${HOST_ORIGIN}/daemon`
    : `${HOST_ORIGIN}/phone?actor=${encodeURIComponent(who.actor)}`;
}

/** `hostUrl`의 반대. 모르는 주소면 null. */
export function whoFrom(url: string): Who | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.pathname === '/daemon') {
    return { role: 'daemon' };
  }
  const actor = parsed.searchParams.get('actor');
  return parsed.pathname === '/phone' && actor ? { role: 'phone', actor } : null;
}

/** DO의 소켓에서 쓰는 부분. */
export interface HostSocket {
  send(message: string): void;
  close(code?: number, reason?: string): void;
  serializeAttachment(value: unknown): void;
  deserializeAttachment(): unknown;
}

/** DO의 `ctx`(DurableObjectState)에서 쓰는 부분. */
export interface HostContext {
  readonly storage: RelayStorage;
  acceptWebSocket(ws: HostSocket, tags?: string[]): void;
  getWebSockets(tag?: string): HostSocket[];
}

/** DO 소켓을 `Relay`가 보는 연결로. 같은 소켓이면 매번 새로 만들어도 첨부로 같은 연결이 된다. */
function peerOf(ws: HostSocket): Peer {
  return {
    send: (text) => ws.send(text),
    close: (code, reason) => ws.close(code, reason),
    state: () => (ws.deserializeAttachment() as PeerState | null) ?? null,
    setState: (state) => ws.serializeAttachment(state),
  };
}

export class RelayHost {
  private readonly relay: Relay;

  constructor(private readonly ctx: HostContext) {
    this.relay = new Relay(ctx.storage, { peers: () => ctx.getWebSockets().map(peerOf) });
  }

  /** 새 소켓을 받는다. 잠들어도 닫히지 않게 `acceptWebSocket`으로 받고 `Relay`에 연다. */
  accept(server: HostSocket, who: Who): void {
    this.ctx.acceptWebSocket(server);
    this.relay.open(peerOf(server), who);
  }

  /** 데몬 소켓(`rockyd::ws`)처럼 텍스트 프레임만 읽는다. */
  message(ws: HostSocket, message: string | ArrayBuffer): void {
    if (typeof message === 'string') {
      this.relay.message(peerOf(ws), message);
    }
  }

  close(ws: HostSocket): void {
    this.relay.close(peerOf(ws));
  }
}
