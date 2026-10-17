/**
 * 클라우드 중계소 Worker의 입구. 요청은 문(`./gate`)이 Access 토큰을 보고 중계소 DO 하나로 넘기고, DO는 소켓을 받아
 * `Relay`에 잇는다(`./host`). Workers 런타임에 묶인 것(`cloudflare:workers`, `WebSocketPair`)만 여기 두고 판정은 옆
 * 모듈에 둔다. 이 파일은 Bun 테스트가 읽지 못한다. 배포와 실측은 `docs/features/sync.md`의 "배포와 실측".
 */
import { DurableObject } from 'cloudflare:workers';
import { httpCerts } from './access';
import { createGate, type RelayEnv, type RequestContext } from './gate';
import { type HostContext, type HostSocket, PING, PONG, RelayHost, whoFrom } from './host';

/** Workers 런타임의 전역. 쓰는 모양만 적는다. */
declare const WebSocketPair: new () => { 0: HostSocket; 1: HostSocket };
declare const WebSocketRequestResponsePair: new (request: string, response: string) => unknown;

type RelayContext = HostContext & { setWebSocketAutoResponse(pair: unknown): void };

export class RelayObject extends DurableObject<RelayEnv> {
  private readonly host: RelayHost;

  constructor(ctx: RelayContext, env: RelayEnv) {
    super(ctx, env);
    // 웹과 데몬 연결기의 살았나 묻기에는 DO를 깨우지 않고 답한다.
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair(PING, PONG));
    this.host = new RelayHost(ctx);
  }

  async fetch(request: Request): Promise<Response> {
    const who = whoFrom(request.url);
    if (who === null) {
      return new Response(null, { status: 400 });
    }
    const pair = new WebSocketPair();
    this.host.accept(pair[1], who);
    return new Response(null, { status: 101, webSocket: pair[0] } as ResponseInit);
  }

  webSocketMessage(ws: HostSocket, message: string | ArrayBuffer): void {
    this.host.message(ws, message);
  }

  // 닫힘 답은 런타임이 대신한다(호환 날짜 2026-04-07 이후의 `web_socket_auto_reply_to_close`).
  webSocketClose(ws: HostSocket): void {
    this.host.close(ws);
  }

  webSocketError(ws: HostSocket): void {
    this.host.close(ws);
  }
}

const gate = createGate({ fetchCerts: httpCerts });

export default {
  fetch: (request: Request, env: RelayEnv, ctx: RequestContext): Promise<Response> =>
    gate(request, env, ctx),
};
