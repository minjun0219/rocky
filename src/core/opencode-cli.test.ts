import { describe, expect, it } from 'bun:test';
import {
  type OpencodeExecutor,
  type OpencodeRunResult,
  buildRunArgs,
  detectOpencode,
  opencodeBin,
  renderRunOutput,
  summarizeRunEvents,
} from './opencode-cli';
import type { JobRequest } from './opencode-jobs';

function fakeExecutor(impl: (args: string[]) => Partial<OpencodeRunResult>): OpencodeExecutor {
  return {
    async run(args) {
      return { stdout: '', stderr: '', exitCode: 0, ...impl(args) };
    },
  };
}

const BASE: JobRequest = { prompt: '작업해줘', worktree: '/repo/wt' };

describe('opencodeBin', () => {
  it('은 기본값이 opencode 다', () => {
    expect(opencodeBin({})).toBe('opencode');
  });

  it('은 ROCKY_OPENCODE_CLI 로 덮어쓴다', () => {
    expect(opencodeBin({ ROCKY_OPENCODE_CLI: '/opt/oc' })).toBe('/opt/oc');
  });

  it('은 공백뿐인 오버라이드를 무시한다', () => {
    expect(opencodeBin({ ROCKY_OPENCODE_CLI: '  ' })).toBe('opencode');
  });
});

describe('buildRunArgs', () => {
  it('은 run / json 포맷 / 작업 디렉터리를 항상 넣는다', () => {
    const args = buildRunArgs(BASE);
    expect(args.slice(0, 5)).toEqual(['run', '--format', 'json', '--dir', '/repo/wt']);
  });

  it('은 프롬프트를 맨 마지막 positional 로 둔다', () => {
    const args = buildRunArgs({ ...BASE, prompt: '여러 줄\n프롬프트 "따옴표" 포함' });
    expect(args[args.length - 1]).toBe('여러 줄\n프롬프트 "따옴표" 포함');
  });

  it('은 model / agent / variant 를 넘긴다', () => {
    const args = buildRunArgs({
      ...BASE,
      model: 'anthropic/claude-sonnet-5',
      agent: 'build',
      variant: 'high',
    });
    expect(args).toContain('--model');
    expect(args[args.indexOf('--model') + 1]).toBe('anthropic/claude-sonnet-5');
    expect(args[args.indexOf('--agent') + 1]).toBe('build');
    expect(args[args.indexOf('--variant') + 1]).toBe('high');
  });

  it('은 미지정 옵션을 아예 붙이지 않는다', () => {
    const args = buildRunArgs(BASE);
    expect(args).not.toContain('--model');
    expect(args).not.toContain('--agent');
    expect(args).not.toContain('--auto');
  });

  it('은 resumeSession 이 continueLast 보다 우선한다', () => {
    const args = buildRunArgs({ ...BASE, resumeSession: 'ses_1', continueLast: true });
    expect(args[args.indexOf('--session') + 1]).toBe('ses_1');
    expect(args).not.toContain('--continue');
  });

  it('은 continueLast 만 있으면 --continue 를 쓴다', () => {
    expect(buildRunArgs({ ...BASE, continueLast: true })).toContain('--continue');
  });

  it('은 auto 를 켜면 --auto 를 붙인다', () => {
    expect(buildRunArgs({ ...BASE, auto: true })).toContain('--auto');
  });

  it('은 attach URL 을 넘겨 콜드 스타트를 피한다', () => {
    const args = buildRunArgs({ ...BASE, attach: 'http://127.0.0.1:4096' });
    expect(args[args.indexOf('--attach') + 1]).toBe('http://127.0.0.1:4096');
  });
});

describe('detectOpencode', () => {
  it('은 정상 종료 시 available true 와 버전을 준다', async () => {
    const detection = await detectOpencode(fakeExecutor(() => ({ stdout: '1.18.5\n' })));
    expect(detection.available).toBe(true);
    expect(detection.version).toBe('1.18.5');
  });

  it('은 non-zero 종료를 미설치로 본다', async () => {
    const detection = await detectOpencode(fakeExecutor(() => ({ exitCode: 127 })));
    expect(detection.available).toBe(false);
    expect(detection.detail).toContain('127');
  });

  it('은 spawn 예외를 삼키고 미설치로 보고한다', async () => {
    const detection = await detectOpencode({
      async run() {
        throw new Error('ENOENT');
      },
    });
    expect(detection.available).toBe(false);
    expect(detection.detail).toContain('ENOENT');
  });
});

describe('summarizeRunEvents', () => {
  const ndjson = [
    '{"type":"session.start","sessionID":"ses_abc"}',
    '{"type":"message.part","part":{"type":"text","text":"안녕"}}',
    '{"type":"message.part","part":{"type":"text","text":" 세계"}}',
    '{"type":"session.idle"}',
  ].join('\n');

  it('은 텍스트 조각을 이어붙인다', () => {
    expect(summarizeRunEvents(ndjson).text).toBe('안녕 세계');
  });

  it('은 세션 id 를 뽑는다', () => {
    expect(summarizeRunEvents(ndjson).sessionId).toBe('ses_abc');
  });

  it('은 이벤트 수를 센다', () => {
    expect(summarizeRunEvents(ndjson).eventCount).toBe(4);
  });

  it('은 NDJSON 이 아니면 eventCount 0 이다', () => {
    const summary = summarizeRunEvents('그냥 평범한 텍스트 출력\n두 번째 줄');
    expect(summary.eventCount).toBe(0);
    expect(summary.text).toBe('');
  });

  it('은 깨진 줄을 건너뛴다', () => {
    const summary = summarizeRunEvents('{"type":"text","text":"ok"}\n{깨진 JSON\n');
    expect(summary.eventCount).toBe(1);
    expect(summary.text).toBe('ok');
  });

  it('은 에러 이벤트를 따로 모은다', () => {
    const summary = summarizeRunEvents('{"type":"error","error":{"message":"model not found"}}');
    expect(summary.errors).toEqual(['model not found']);
    expect(summary.text).toBe('');
  });

  // 아래 3건은 opencode 1.18.5 `run --format json` 실측 출력에서 그대로 가져온 모양이다.
  it('은 실측 NDJSON 의 텍스트와 세션 id 를 뽑는다', () => {
    const real = [
      '{"type":"step_start","timestamp":1784975019505,"sessionID":"ses_06732","part":{"id":"prt_1","type":"step-start"}}',
      '{"type":"text","timestamp":1784975019505,"sessionID":"ses_06732","part":{"id":"prt_2","type":"text","text":"PONG"}}',
      '{"type":"step_finish","timestamp":1784975019555,"sessionID":"ses_06732","part":{"id":"prt_3","type":"step-finish","reason":"stop"}}',
    ].join('\n');
    const summary = summarizeRunEvents(real);
    expect(summary.text).toBe('PONG');
    expect(summary.sessionId).toBe('ses_06732');
    expect(summary.errors).toEqual([]);
  });

  it('은 error.data.message 를 분류명보다 우선한다', () => {
    const real =
      '{"type":"error","timestamp":1784974908677,"sessionID":"ses_1","error":{"name":"UnknownError","data":{"message":"Unexpected server error.","ref":"err_5004953f"}}}';
    expect(summarizeRunEvents(real).errors).toEqual(['Unexpected server error.']);
  });

  it('은 같은 part 가 갱신되며 반복돼도 중복 누적하지 않는다', () => {
    const streaming = [
      '{"type":"text","part":{"id":"prt_9","text":"안"}}',
      '{"type":"text","part":{"id":"prt_9","text":"안녕"}}',
      '{"type":"text","part":{"id":"prt_9","text":"안녕하세요"}}',
    ].join('\n');
    expect(summarizeRunEvents(streaming).text).toBe('안녕하세요');
  });

  it('은 서로 다른 part 는 등장 순서대로 이어붙인다', () => {
    const multi = [
      '{"type":"text","part":{"id":"prt_1","text":"첫째 "}}',
      '{"type":"text","part":{"id":"prt_2","text":"둘째"}}',
    ].join('\n');
    expect(summarizeRunEvents(multi).text).toBe('첫째 둘째');
  });
});

describe('renderRunOutput', () => {
  const empty: OpencodeRunResult = { stdout: '', stderr: '', exitCode: 0 };

  it('은 파싱된 텍스트를 우선한다', () => {
    const rendered = renderRunOutput(
      { ...empty, stdout: 'raw' },
      summarizeRunEvents('{"type":"text","text":"파싱됨"}'),
    );
    expect(rendered).toBe('파싱됨');
  });

  it('은 NDJSON 이 아니면 stdout 원문으로 폴백한다', () => {
    const stdout = '평범한 출력';
    expect(renderRunOutput({ ...empty, stdout }, summarizeRunEvents(stdout))).toBe(stdout);
  });

  it('은 텍스트가 없고 에러만 있으면 에러를 보여준다', () => {
    const stdout = '{"type":"error","error":{"message":"boom"}}';
    expect(renderRunOutput({ ...empty, stdout }, summarizeRunEvents(stdout))).toBe('boom');
  });

  it('은 아무것도 없으면 stderr 로 폴백한다', () => {
    const result: OpencodeRunResult = { stdout: '', stderr: '치명적 오류', exitCode: 1 };
    expect(renderRunOutput(result, summarizeRunEvents(''))).toBe('치명적 오류');
  });
});
