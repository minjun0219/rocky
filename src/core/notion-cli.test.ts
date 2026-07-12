import { describe, expect, it } from 'bun:test';
import {
  createBunNotionCli,
  detectNotionCli,
  type NotionCliExecutor,
  NotionCliCommandError,
  NotionCliNotInstalledError,
  notionCliFetch,
  parseNtnPayload,
} from './notion-cli';

const PAGE = '1234abcd1234abcd1234abcd1234abcd';
const PAGE_DASHED = '1234abcd-1234-abcd-1234-abcd1234abcd';

/** args 를 받아 지정한 결과를 돌려주는 fake executor. */
function fakeExec(
  responder: (args: string[]) => { stdout?: string; stderr?: string; exitCode?: number } | Error,
): NotionCliExecutor & { calls: string[][] } {
  const calls: string[][] = [];
  return {
    calls,
    async run(args) {
      calls.push(args);
      const result = responder(args);
      if (result instanceof Error) {
        throw result;
      }
      return {
        stdout: result.stdout ?? '',
        stderr: result.stderr ?? '',
        exitCode: result.exitCode ?? 0,
      };
    },
  };
}

describe('detectNotionCli', () => {
  it('true when --version exits 0', async () => {
    const exec = fakeExec(() => ({ stdout: 'ntn 0.19.0', exitCode: 0 }));
    expect(await detectNotionCli(exec)).toBe(true);
    expect(exec.calls[0]).toEqual(['--version']);
  });
  it('false when binary is missing', async () => {
    const exec = fakeExec(() => new NotionCliNotInstalledError('ntn'));
    expect(await detectNotionCli(exec)).toBe(false);
  });
  it('false when --version exits non-zero', async () => {
    const exec = fakeExec(() => ({ exitCode: 1 }));
    expect(await detectNotionCli(exec)).toBe(false);
  });
});

describe('parseNtnPayload', () => {
  it('maps { id, title, markdown } JSON', () => {
    const raw = parseNtnPayload(
      JSON.stringify({ id: PAGE_DASHED, title: '기획서', markdown: '# 제목\n\n본문' }),
      PAGE,
      PAGE_DASHED,
    );
    expect(raw).toEqual({ id: PAGE_DASHED, title: '기획서', markdown: '# 제목\n\n본문' });
  });
  it('accepts alternate content key + derives id from input', () => {
    const raw = parseNtnPayload(JSON.stringify({ content: '# Body' }), PAGE, PAGE_DASHED);
    expect(raw.markdown).toBe('# Body');
    expect(raw.id).toBe(PAGE_DASHED);
    expect(raw.title).toBe('Body');
  });
  it('reads title from Notion property object', () => {
    const raw = parseNtnPayload(
      JSON.stringify({
        markdown: 'body',
        properties: { Name: { title: [{ plain_text: 'Prop Title' }] } },
      }),
      PAGE,
      PAGE_DASHED,
    );
    expect(raw.title).toBe('Prop Title');
  });
  it('falls back to raw stdout when JSON has no known body key (no silent content loss)', () => {
    const stdout = JSON.stringify({ unexpected_schema: true, note: '내용 있음' });
    const raw = parseNtnPayload(stdout, PAGE, PAGE_DASHED);
    expect(raw.markdown).toBe(stdout);
    expect(raw.id).toBe(PAGE_DASHED);
  });
  it('falls back to plain markdown when stdout is not JSON', () => {
    const raw = parseNtnPayload('---\ntitle: x\n---\n# Heading\n\ntext', PAGE, PAGE_DASHED);
    expect(raw.markdown).toContain('# Heading');
    expect(raw.title).toBe('Heading');
    expect(raw.id).toBe(PAGE_DASHED);
  });
  it('throws on empty output', () => {
    expect(() => parseNtnPayload('   ', PAGE, PAGE_DASHED)).toThrow();
  });
});

describe('notionCliFetch', () => {
  it('invokes `pages get <input> --json` and normalizes', async () => {
    const exec = fakeExec(() => ({
      stdout: JSON.stringify({ id: PAGE_DASHED, title: 'T', markdown: '# T' }),
      exitCode: 0,
    }));
    const raw = await notionCliFetch(exec, PAGE);
    expect(exec.calls[0]).toEqual(['pages', 'get', PAGE, '--json']);
    expect(raw.title).toBe('T');
    expect(raw.markdown).toBe('# T');
  });
  it('throws NotionCliCommandError on non-zero exit', async () => {
    const exec = fakeExec(() => ({ stderr: 'page not found', exitCode: 1 }));
    await expect(notionCliFetch(exec, PAGE)).rejects.toBeInstanceOf(NotionCliCommandError);
  });
});

// 실제 Bun.spawn 백엔드로 hang 엣지를 재현한다 (gemini 지적 → 어드바이저 검증 → 수정 확정).
// hard-timeout race + SIGKILL 이 없으면 아래 두 케이스는 timeoutMs 를 훨씬 넘겨 매달린다.
describe('createBunNotionCli hard timeout', () => {
  it('times out (124) even when the child ignores SIGTERM', async () => {
    const exec = createBunNotionCli('sh');
    const t0 = Date.now();
    const err = await exec.run(['-c', 'trap "" TERM; sleep 3'], { timeoutMs: 200 }).catch((e) => e);
    expect(err).toBeInstanceOf(NotionCliCommandError);
    expect((err as NotionCliCommandError).exitCode).toBe(124);
    expect(Date.now() - t0).toBeLessThan(1500);
    // 에러 메시지는 실제 실행 인자만 담고 env 기본 bin(ntn)을 거짓으로 찍지 않는다.
    expect((err as Error).message).not.toContain('ntn');
    expect((err as Error).message).toContain('sleep 3');
  });

  it('times out (124) even when a grandchild keeps the stdout pipe open', async () => {
    const exec = createBunNotionCli('sh');
    const t0 = Date.now();
    // sh 는 즉시 종료하지만 백그라운드 손자 sleep 이 stdout 파이프를 상속해 EOF 를 막는다.
    const err = await exec.run(['-c', 'sleep 3 &'], { timeoutMs: 200 }).catch((e) => e);
    expect(err).toBeInstanceOf(NotionCliCommandError);
    expect((err as NotionCliCommandError).exitCode).toBe(124);
    expect(Date.now() - t0).toBeLessThan(1500);
  });

  it('returns normally well within the timeout for a fast command', async () => {
    const exec = createBunNotionCli('sh');
    const result = await exec.run(['-c', 'printf hello'], { timeoutMs: 2000 });
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toBe('hello');
  });
});
