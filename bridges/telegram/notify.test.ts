import { afterAll, describe, expect, test } from 'bun:test';
import { join } from 'node:path';

/**
 * 텔레그램 알림 브릿지 — 진짜 API 대신 로컬 가짜 서버(`--api`)로 요청 모양을 본다. `op read` 경로는
 * 자격이 필요해 테스트에 넣지 않는다 — 대신 "토큰이 없으면 exit 1 + stderr 한 줄" 만 고정한다.
 */
const script = join(import.meta.dir, 'notify.ts');

type Seen = { path: string; body: string };
const seen: Seen[] = [];
const server = Bun.serve({
  port: 0,
  hostname: '127.0.0.1',
  async fetch(req) {
    const path = new URL(req.url).pathname;
    const body = await req.text();
    seen.push({ path, body });
    if (path.includes('/botBAD/')) {
      return new Response('{"ok":false,"description":"Unauthorized"}', { status: 401 });
    }
    return new Response('{"ok":true}');
  },
});
afterAll(() => server.stop(true));

const payload = JSON.stringify({
  kind: 'ready',
  repo: 'o/r',
  number: 7,
  title: 'PR 7',
  url: 'https://github.com/o/r/pull/7',
  heading: 'rocky · o/r',
  text: '#7 확인·머지해도 된다 — PR 7',
});

async function run(args: string[], stdin: string, env: Record<string, string> = {}) {
  const proc = Bun.spawn({
    cmd: ['bun', script, '--api', `http://127.0.0.1:${server.port}`, ...args],
    stdin: new Response(stdin),
    stdout: 'pipe',
    stderr: 'pipe',
    env: { PATH: process.env.PATH ?? '', HOME: '/nonexistent', ...env },
  });
  const [code, out, err] = await Promise.all([
    proc.exited,
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ]);
  return { code, out, err };
}

describe('bridges/telegram/notify.ts', () => {
  test('stdin 의 전이를 sendMessage 로 보낸다 — heading + text, chat_id, 토큰은 URL 에만', async () => {
    seen.length = 0;
    const r = await run(['--chat', '42'], payload, { ROCKY_TELEGRAM_TOKEN: '123:abc\n' });
    expect(r.err).toBe('');
    expect(r.code).toBe(0);
    expect(r.out).toBe('');
    expect(seen).toHaveLength(1);
    expect(seen[0].path).toBe('/bot123:abc/sendMessage');
    const form = new URLSearchParams(seen[0].body);
    expect(form.get('chat_id')).toBe('42');
    expect(form.get('text')).toBe('rocky · o/r\n#7 확인·머지해도 된다 — PR 7');
    expect(form.get('disable_web_page_preview')).toBe('true');
  });

  test('토큰이 없으면 exit 1 + stderr 한 줄, 요청 없음', async () => {
    seen.length = 0;
    const r = await run(['--chat', '42'], payload);
    expect(r.code).toBe(1);
    expect(r.err.trim().split('\n')).toHaveLength(1);
    expect(r.err).toContain('토큰이 없다');
    expect(seen).toHaveLength(0);
  });

  test('API 가 거부하면 exit 1 — 사유에 상태 코드, 토큰은 없다', async () => {
    const r = await run(['--chat', '42'], payload, { ROCKY_TELEGRAM_TOKEN: 'BAD' });
    expect(r.code).toBe(1);
    expect(r.err).toContain('HTTP 401');
    expect(r.err).not.toContain('/botBAD');
  });

  test('stdin 이 규약 모양이 아니면 보내지 않는다', async () => {
    seen.length = 0;
    const r = await run(['--chat', '42'], '{"kind":"ready"}', { ROCKY_TELEGRAM_TOKEN: 'x' });
    expect(r.code).toBe(1);
    expect(r.err).toContain('heading/text');
    expect(seen).toHaveLength(0);
    const r2 = await run([], payload, { ROCKY_TELEGRAM_TOKEN: 'x' });
    expect(r2.code).toBe(1);
    expect(r2.err).toContain('--chat');
  });
});
