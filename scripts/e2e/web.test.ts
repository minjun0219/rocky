import { describe, expect, test } from 'bun:test';
import { dedupe, exitCodeOf, type Finding, parseArgs } from './web';

const finding = (kind: string, detail = 'x'): Finding => ({
  env: 'phone',
  step: '01',
  kind,
  detail,
});

describe('parseArgs', () => {
  test('기본값', () => {
    expect(parseArgs([])).toEqual({ strict: false, build: true, headed: false });
  });
  test('옵션을 읽는다', () => {
    expect(parseArgs(['--strict', '--no-build', '--out', 'shots', '--headed'])).toEqual({
      strict: true,
      build: false,
      headed: true,
      out: 'shots',
    });
  });
  test('값 없는 --out 과 모르는 옵션은 거절한다', () => {
    expect(() => parseArgs(['--out'])).toThrow('--out');
    expect(() => parseArgs(['--nope'])).toThrow('--nope');
  });
});

describe('exitCodeOf', () => {
  test('기능 실패가 있으면 1', () => {
    expect(exitCodeOf([finding('실패')], false)).toBe(1);
    expect(exitCodeOf([finding('JS 에러')], false)).toBe(1);
    expect(exitCodeOf([finding('콘솔 에러')], false)).toBe(1);
  });
  test('화면 점검 발견만 있으면 0, --strict 면 1', () => {
    expect(exitCodeOf([finding('24px 미만 터치 타깃')], false)).toBe(0);
    expect(exitCodeOf([finding('24px 미만 터치 타깃')], true)).toBe(1);
    expect(exitCodeOf([], true)).toBe(0);
  });
});

test('dedupe 는 같은 발견을 한 번만 남긴다', () => {
  expect(dedupe([finding('실패'), finding('실패'), finding('실패', 'y')])).toHaveLength(2);
});
