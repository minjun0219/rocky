/**
 * 중계소 Worker의 문. 길(`/api/ws` 폰, `/relay` 데몬)을 가리고, 웹소켓 업그레이드인지, 폰이면 같은 출처의 페이지가 연
 * 소켓인지 보고, Access 토큰을 검증해 그 길에 들일 주인일 때만 중계소 DO로 넘긴다. 하나라도 아니면 소켓을 열지 않는다.
 * DO에는 Worker가 정한 역할과 actor만 주소로 넘긴다(`hostUrl`). 설계는 스펙 3번 조각 "인증".
 */
import {
  type AccessConfig,
  AccessError,
  AccessVerifier,
  admit,
  type RelayRole,
  type VerifierOptions,
} from './access';
import { hostUrl, type Who } from './host';

export interface RelayNamespace {
  getByName(name: string): { fetch(request: Request): Promise<Response> };
}

/** Worker 바인딩. 개인 값 넷은 secret으로 둔다. 공개 레포라 `wrangler.jsonc`에 적지 않는다. */
export interface RelayEnv {
  RELAY: RelayNamespace;
  /** 팀 이름. `<team>.cloudflareaccess.com`의 `<team>`. */
  ACCESS_TEAM?: string;
  /** Access 애플리케이션의 AUD 태그. 여럿이면 쉼표로. */
  ACCESS_AUD?: string;
  /** 폰으로 들일 이메일. 여럿이면 쉼표로. */
  ACCESS_EMAILS?: string;
  /** 데몬 연결기의 서비스 토큰 Client ID. */
  DAEMON_CLIENT_ID?: string;
}

/** Worker `fetch`의 셋째 인자 가운데 쓰는 것. */
export interface RequestContext {
  waitUntil(promise: Promise<unknown>): void;
}

export interface RelaySettings {
  access: AccessConfig;
  emails: string[];
  daemonClientId: string;
}

export const GATE_MESSAGES = {
  notFound: '중계소에 없는 길이다 — 폰은 /api/ws, 데몬은 /relay',
  notUpgrade: '웹소켓 업그레이드(GET, Upgrade: websocket)만 받는다',
  crossOrigin: '다른 출처의 페이지가 연 소켓은 받지 않는다(CSRF 방지)',
  noToken:
    'Access 토큰(Cf-Access-Jwt-Assertion)이 없다 — 이 주소가 Access 애플리케이션 뒤에 있는지 본다',
  missing: (names: string[]) =>
    `중계소 설정이 비어 있다: ${names.join(', ')} — wrangler secret put 으로 넣는다`,
  badTeam: 'ACCESS_TEAM 에는 팀 이름만 적는다(<team>.cloudflareaccess.com 의 <team>)',
};

/** 길 → 역할. */
const ROUTES: Readonly<Record<string, RelayRole>> = { '/api/ws': 'phone', '/relay': 'daemon' };

/** 중계소 DO는 하나다. 오너 한 명의 보드 하나를 잇는다. */
const RELAY_NAME = 'relay';

function list(value: string | undefined): string[] {
  return (value ?? '')
    .split(',')
    .map((item) => item.trim())
    .filter((item) => item !== '');
}

/** secret에서 설정을 읽는다. 하나라도 비면 그 이유. */
export function readSettings(env: RelayEnv): RelaySettings | { problem: string } {
  const team = (env.ACCESS_TEAM ?? '').trim();
  const aud = list(env.ACCESS_AUD);
  const emails = list(env.ACCESS_EMAILS);
  const daemonClientId = (env.DAEMON_CLIENT_ID ?? '').trim();
  const missing = [
    team === '' && 'ACCESS_TEAM',
    aud.length === 0 && 'ACCESS_AUD',
    emails.length === 0 && 'ACCESS_EMAILS',
    daemonClientId === '' && 'DAEMON_CLIENT_ID',
  ].filter((name): name is string => typeof name === 'string');
  if (missing.length > 0) {
    return { problem: GATE_MESSAGES.missing(missing) };
  }
  // 팀 이름은 공개키 주소의 호스트가 된다. 다른 호스트로 새지 않게 모양을 본다.
  if (!/^[A-Za-z0-9-]+$/.test(team)) {
    return { problem: GATE_MESSAGES.badTeam };
  }
  return { access: { team, aud }, emails, daemonClientId };
}

/**
 * 같은 출처의 페이지가 연 소켓인가. 데몬의 `is_same_origin_request`와 같다. `Sec-Fetch-Site`가 있으면 `same-origin`만,
 * 없으면 `Origin`의 호스트[:포트]가 요청 호스트와 같아야 한다. 둘 다 없으면 브라우저가 아니다. Access 쿠키는 같은 등록
 * 도메인의 다른 서브도메인 페이지가 연 소켓에도 실려 오므로, 쓰기(`tx`)를 받는 폰 소켓은 출처까지 같아야 한다.
 */
export function isSameOrigin(headers: Headers, host: string): boolean {
  const site = headers.get('sec-fetch-site');
  if (site !== null) {
    return site.trim().toLowerCase() === 'same-origin';
  }
  const origin = headers.get('origin');
  if (origin === null) {
    return true;
  }
  const authority = (origin.trim().split('://')[1] ?? '').split(/[/?#]/)[0]?.toLowerCase() ?? '';
  return authority !== '' && authority === host.toLowerCase();
}

/** 데몬 소켓(`rockyd::daemon::ws_handler`)과 같다. `?actor=`를 다듬고, 비면 `unknown`. */
function actorOf(url: URL): string {
  return url.searchParams.get('actor')?.trim() || 'unknown';
}

function refuse(status: number, error: string): Response {
  return Response.json({ error }, { status });
}

/** Worker의 `fetch`. 공개키 캐시를 품는다. */
export function createGate(
  options: VerifierOptions,
): (request: Request, env: RelayEnv, ctx?: RequestContext) => Promise<Response> {
  const log = options.log ?? ((line: string) => console.warn(line));
  let cached: { key: string; verifier: AccessVerifier } | null = null;
  const verifierFor = (config: AccessConfig): AccessVerifier => {
    const key = `${config.team}\n${config.aud.join(',')}`;
    if (cached?.key !== key) {
      cached = { key, verifier: new AccessVerifier(config, options) };
    }
    return cached.verifier;
  };

  return async (request, env, ctx) => {
    const url = new URL(request.url);
    const role = Object.hasOwn(ROUTES, url.pathname) ? ROUTES[url.pathname] : undefined;
    if (role === undefined) {
      return refuse(404, GATE_MESSAGES.notFound);
    }
    if (request.method !== 'GET' || request.headers.get('upgrade')?.toLowerCase() !== 'websocket') {
      return refuse(426, GATE_MESSAGES.notUpgrade);
    }
    const settings = readSettings(env);
    if ('problem' in settings) {
      log(`rocky relay: ${settings.problem}`);
      return refuse(500, settings.problem);
    }
    if (role === 'phone' && !isSameOrigin(request.headers, url.host)) {
      return refuse(403, GATE_MESSAGES.crossOrigin);
    }
    const token = request.headers.get('cf-access-jwt-assertion');
    if (!token) {
      return refuse(403, GATE_MESSAGES.noToken);
    }
    try {
      const identity = await verifierFor(settings.access).identify(token, (promise) =>
        ctx?.waitUntil(promise),
      );
      admit(identity, role, settings);
    } catch (error) {
      if (!(error instanceof AccessError)) {
        throw error;
      }
      log(`rocky relay: ${url.pathname} 소켓을 열지 않음 — ${error.message}`);
      return refuse(403, error.message);
    }
    const who: Who = role === 'daemon' ? { role } : { role, actor: actorOf(url) };
    return env.RELAY.getByName(RELAY_NAME).fetch(new Request(hostUrl(who), request));
  };
}
