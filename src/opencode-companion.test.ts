import { describe, expect, it } from 'bun:test';
import { parseArgs } from './opencode-companion';

describe('parseArgs', () => {
  it('은 서브커맨드를 첫 토큰에서 읽는다', () => {
    expect(parseArgs(['status']).command).toBe('status');
  });

  it('은 인자가 없으면 help 로 떨어진다', () => {
    expect(parseArgs([]).command).toBe('help');
  });

  it('은 --key value 를 문자열로 받는다', () => {
    expect(parseArgs(['task', '--worktree', '/repo/wt']).flags.worktree).toBe('/repo/wt');
  });

  it('은 --key=value 도 받는다', () => {
    expect(parseArgs(['task', '--model=anthropic/claude-sonnet-5']).flags.model).toBe(
      'anthropic/claude-sonnet-5',
    );
  });

  it('은 boolean 플래그가 뒤 토큰을 삼키지 않게 한다', () => {
    const parsed = parseArgs(['task', '--background', 'prompt-text']);
    expect(parsed.flags.background).toBe(true);
    expect(parsed.positionals).toEqual(['prompt-text']);
  });

  it('은 --continue 뒤 positional 을 보존한다', () => {
    const parsed = parseArgs(['task', '--continue', '--auto', '이어서 해줘']);
    expect(parsed.flags.continue).toBe(true);
    expect(parsed.flags.auto).toBe(true);
    expect(parsed.positionals).toEqual(['이어서 해줘']);
  });

  it('은 값 없는 마지막 플래그를 true 로 본다', () => {
    expect(parseArgs(['task', '--model']).flags.model).toBe(true);
  });

  it('은 플래그가 다른 플래그를 값으로 삼키지 않게 한다', () => {
    const parsed = parseArgs(['task', '--model', '--auto']);
    expect(parsed.flags.model).toBe(true);
    expect(parsed.flags.auto).toBe(true);
  });

  it('은 positional 을 순서대로 모은다', () => {
    expect(parseArgs(['result', 'oc-abc', 'extra']).positionals).toEqual(['oc-abc', 'extra']);
  });

  it('은 멀티 단어 프롬프트를 positional 로 모은다', () => {
    const parsed = parseArgs(['task', '--worktree', '/wt', '문서를', '갱신해줘']);
    expect(parsed.positionals).toEqual(['문서를', '갱신해줘']);
  });
});
