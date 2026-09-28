import { describe, expect, it } from 'bun:test';
import {
  botVerdict,
  parseArgs,
  reactionsToRemove,
  summarizeThreads,
  type ThreadNode,
} from './pr-threads';

const ME = 'minjun0219';

function thread(
  over: Partial<ThreadNode> & { reactions?: Array<[string, string]> } = {},
): ThreadNode {
  const { reactions = [], ...rest } = over;
  return {
    id: 'PRRT_1',
    isResolved: false,
    isOutdated: false,
    path: 'a.rs',
    line: 3,
    comments: {
      nodes: [
        {
          id: 'PRRC_1',
          author: { login: 'chatgpt-codex-connector' },
          body: 'P1',
          createdAt: '2026-09-28T01:00:00Z',
          reactions: { nodes: reactions.map(([content, login]) => ({ content, user: { login } })) },
        },
      ],
    },
    ...rest,
  };
}

describe('summarizeThreads', () => {
  it('미해결만 남기고 첫 코멘트 id 와 내 상태 리액션을 편다', () => {
    const out = summarizeThreads(
      [
        thread({
          reactions: [
            ['EYES', ME],
            ['THUMBS_UP', 'someone-else'],
            ['HEART', ME],
          ],
        }),
        thread({ id: 'PRRT_2', isResolved: true }),
      ],
      ME,
    );
    expect(out).toHaveLength(1);
    expect(out[0]?.commentId).toBe('PRRC_1');
    // 남의 리액션과 상태 아닌 리액션(HEART)은 세지 않는다.
    expect(out[0]?.mine).toEqual(['EYES']);
    expect(out[0]?.author).toBe('chatgpt-codex-connector');
  });

  it('코멘트가 없는 스레드는 건너뛴다', () => {
    expect(summarizeThreads([thread({ comments: { nodes: [] } })], ME)).toEqual([]);
  });
});

describe('reactionsToRemove', () => {
  it('목표 외의 내 상태 리액션만 — 옛 👍 도 뗀다', () => {
    expect(reactionsToRemove(['EYES', 'THUMBS_UP', 'HEART'], 'ROCKET')).toEqual([
      'EYES',
      'THUMBS_UP',
    ]);
    expect(reactionsToRemove(['ROCKET'], 'ROCKET')).toEqual([]);
    expect(reactionsToRemove([], 'EYES')).toEqual([]);
  });
});

describe('botVerdict', () => {
  const head = '2026-09-28T01:00:00Z';
  const bot = (submittedAt: string | null) => ({
    author: { login: 'chatgpt-codex-connector' },
    submittedAt,
  });
  const thumbs = (login: string, createdAt: string, content = 'THUMBS_UP') => ({
    content,
    createdAt,
    user: { login },
  });

  it('head 이후 봇 리뷰 제출 = findings — 이전 리뷰·사람 리뷰·미제출은 아니다', () => {
    expect(botVerdict([bot('2026-09-28T01:05:00Z')], [], head)).toBe('findings');
    expect(botVerdict([bot('2026-09-28T00:55:00Z')], [], head)).toBe('pending');
    expect(botVerdict([bot(null)], [], head)).toBe('pending');
    expect(
      botVerdict(
        [{ author: { login: 'minjun0219' }, submittedAt: '2026-09-28T02:00:00Z' }],
        [],
        head,
      ),
    ).toBe('pending');
    expect(botVerdict([{ author: null, submittedAt: '2026-09-28T02:00:00Z' }], [], head)).toBe(
      'pending',
    );
  });

  it('본문의 봇 👍 = clean — 사람 👍·👀·head 이전 것은 아니다', () => {
    expect(botVerdict([], [thumbs('chatgpt-codex-connector', '2026-09-28T01:03:00Z')], head)).toBe(
      'clean',
    );
    // 실제 GraphQL 은 리액션 user 를 `[bot]` 접미사로 준다(#158 실측).
    expect(
      botVerdict([], [thumbs('chatgpt-codex-connector[bot]', '2026-09-28T01:03:00Z')], head),
    ).toBe('clean');
    expect(botVerdict([], [thumbs('minjun0219', '2026-09-28T01:03:00Z')], head)).toBe('pending');
    expect(
      botVerdict([], [thumbs('chatgpt-codex-connector', '2026-09-28T01:03:00Z', 'EYES')], head),
    ).toBe('pending');
    expect(botVerdict([], [thumbs('chatgpt-codex-connector', '2026-09-28T00:30:00Z')], head)).toBe(
      'pending',
    );
    expect(
      botVerdict(
        [],
        [{ content: 'THUMBS_UP', createdAt: '2026-09-28T02:00:00Z', user: null }],
        head,
      ),
    ).toBe('pending');
  });

  it('리뷰와 👍 가 둘 다 있으면 findings 가 이긴다', () => {
    expect(
      botVerdict(
        [bot('2026-09-28T01:05:00Z')],
        [thumbs('chatgpt-codex-connector', '2026-09-28T01:06:00Z')],
        head,
      ),
    ).toBe('findings');
  });
});

describe('parseArgs', () => {
  it('list / watch 는 PR 번호 생략 가능, --timeout 은 watch 에만 의미', () => {
    expect(parseArgs(['list'])).toEqual({ cmd: 'list', timeoutSec: 300 });
    expect(parseArgs(['watch', '154', '--timeout', '60'])).toEqual({
      cmd: 'watch',
      pr: 154,
      timeoutSec: 60,
    });
  });

  it('react 는 코멘트 id 와 상태 리액션 둘 다 필요하고 👍 는 거부한다', () => {
    expect(parseArgs(['react', 'PRRC_1', 'EYES'])).toMatchObject({
      cmd: 'react',
      commentId: 'PRRC_1',
      reaction: 'EYES',
    });
    expect(() => parseArgs(['react', 'PRRC_1'])).toThrow('둘 다');
    expect(() => parseArgs(['react', 'PRRC_1', 'THUMBS_UP'])).toThrow('EYES|ROCKET');
  });

  it('잘못된 입력은 조용히 넘기지 않는다', () => {
    expect(() => parseArgs(['list', 'abc'])).toThrow('PR 번호');
    expect(() => parseArgs(['watch', '--timeout', '0'])).toThrow('--timeout');
    expect(() => parseArgs(['list', '--nope'])).toThrow('모르는 옵션');
    expect(() => parseArgs([])).toThrow('사용법');
  });
});
