#!/usr/bin/env bun
/**
 * 텔레그램 알림 브릿지 — `rocky.json` 의 `pr.notifiers[]` 에 등록한다.
 *
 * ```json
 * { "pr": { "notifiers": [
 *   { "name": "telegram", "command": ["bun", "/path/to/rocky/bridges/telegram/notify.ts",
 *                                     "--op", "op://Agent Vault/<item-uuid>/credential", "--chat", "123456789"],
 *     "timeoutMs": 15000 }
 * ] } }
 * ```
 *
 * 데몬이 전이 한 건을 stdin 에 JSON 으로 준다(`{ kind, repo, number, title, url, heading, text }`).
 * 이 브릿지는 `heading` + `text` 를 Bot API `sendMessage` 로 보낸다. 토큰은 `--op REF` 로 `op read`
 * (1Password Agent Vault; 서비스 계정 토큰은 `~/.config/op/service-account-token`), 없으면 env
 * `ROCKY_TELEGRAM_TOKEN`(테스트·CI 용). 토큰은 어디에도 찍지 않는다 — URL 은 프로세스 안에서만 조립된다.
 *
 * chat id 는 봇에게 먼저 아무 말이나 보낸 뒤 `getUpdates` 로 확인한다(`docs/board.md` "PR 감시").
 * 실패는 exit 1 + stderr 한 줄 — 데몬이 브릿지 이름과 함께 로그에 남긴다.
 */

type Payload = {
  kind: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  heading: string;
  text: string;
};

function fail(message: string): never {
  process.stderr.write(`telegram: ${message}\n`);
  process.exit(1);
}

function parseArgs(argv: string[]) {
  const out: { op?: string; chat?: string; api: string } = { api: 'https://api.telegram.org' };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const value = argv[i + 1];
    if (arg === '--op') {
      out.op = value;
      i += 1;
    } else if (arg === '--chat') {
      out.chat = value;
      i += 1;
    } else if (arg === '--api') {
      out.api = value;
      i += 1;
    } else {
      fail(`모르는 인자: ${arg}`);
    }
  }
  return out;
}

/** `--op` 가 있으면 op read, 없으면 env. 값은 반환만 하고 어디에도 찍지 않는다. */
async function readToken(opRef: string | undefined): Promise<string> {
  if (opRef) {
    const proc = Bun.spawn(['op', 'read', opRef], { stdout: 'pipe', stderr: 'pipe' });
    const timer = setTimeout(() => proc.kill(), 5000);
    const [code, stdout, stderr] = await Promise.all([
      proc.exited,
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
    ]);
    clearTimeout(timer);
    if (code !== 0) {
      // op 의 stderr 는 값이 아니라 사유(참조 문법·권한)라 첫 줄만 그대로 전한다.
      const reason = stderr.trim().split('\n')[0] || `exit ${code}`;
      fail(`op read 실패: ${reason}`);
    }
    const token = stdout.trim();
    if (!token) {
      fail('op read 가 빈 값을 냈다');
    }
    return token;
  }
  const token = (process.env.ROCKY_TELEGRAM_TOKEN ?? '').trim();
  if (!token) {
    fail('토큰이 없다 — --op REF 또는 ROCKY_TELEGRAM_TOKEN');
  }
  return token;
}

/** stdin 의 전이 JSON. 모양이 틀리면 실패 — 반쯤 보내지 않는다. */
export function parsePayload(raw: string): Payload {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    fail('stdin 이 JSON 이 아니다');
  }
  const v = value as Record<string, unknown>;
  if (typeof v?.heading !== 'string' || typeof v?.text !== 'string') {
    fail('stdin 에 heading/text 가 없다');
  }
  return v as Payload;
}

/** 텔레그램 본문 — 배너와 같은 두 줄. HTML 파싱을 켜지 않으니 이스케이프가 없다. */
export function messageOf(payload: Payload): string {
  return `${payload.heading}\n${payload.text}`;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.chat) {
    fail('--chat <id> 가 필요하다');
  }
  const payload = parsePayload(await new Response(Bun.stdin.stream()).text());
  const token = await readToken(args.op);
  const body = new URLSearchParams({
    chat_id: args.chat,
    text: messageOf(payload),
    disable_web_page_preview: 'true',
  });
  let response: Response;
  try {
    response = await fetch(`${args.api}/bot${token}/sendMessage`, {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body,
      signal: AbortSignal.timeout(8000),
    });
  } catch (error) {
    fail(`sendMessage 요청 실패: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (!response.ok) {
    // 응답 본문에는 토큰이 없다(설명 문구뿐) — 첫 줄만.
    const text = (await response.text()).trim().split('\n')[0];
    fail(`sendMessage HTTP ${response.status}: ${text}`);
  }
}

if (import.meta.main) {
  await main();
}
