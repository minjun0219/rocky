import { describe, expect, it } from 'bun:test';
import {
  kstDate,
  latestNightly,
  nightlyTag,
  parseLsRemoteTags,
  parseNightlyTag,
  planNightly,
  prunableTags,
} from './nightly';

const HEAD = 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678';
const OLD = 'fdbf9dd0000000000000000000000000000000ff';
const green = { status: 'completed', conclusion: 'success' };

describe('parseNightlyTag', () => {
  it('nightly 태그만 읽는다', () => {
    expect(parseNightlyTag('v0.54.0-nightly.20261029')).toEqual({
      tag: 'v0.54.0-nightly.20261029',
      version: '0.54.0',
      date: '20261029',
      seq: 1,
    });
    expect(parseNightlyTag('v0.54.0-nightly.20261029.2')?.seq).toBe(2);
    expect(parseNightlyTag('v0.54.0')).toBeNull();
    expect(parseNightlyTag('v0.24.0-next.3')).toBeNull();
  });
});

describe('kstDate', () => {
  it('16:30 UTC 의 cron 은 KST 다음 날짜를 단다', () => {
    expect(kstDate(new Date('2026-10-28T16:30:00Z'))).toBe('20261029');
    expect(kstDate(new Date('2026-10-28T14:59:00Z'))).toBe('20261028');
  });
});

describe('nightlyTag', () => {
  it('같은 날 이미 있으면 순번을 붙인다', () => {
    expect(nightlyTag('0.54.0', '20261029', [])).toBe('v0.54.0-nightly.20261029');
    expect(nightlyTag('0.54.0', '20261029', ['v0.54.0-nightly.20261029'])).toBe(
      'v0.54.0-nightly.20261029.2',
    );
    expect(
      nightlyTag('0.54.0', '20261029', ['v0.54.0-nightly.20261029', 'v0.54.0-nightly.20261029.2']),
    ).toBe('v0.54.0-nightly.20261029.3');
    expect(nightlyTag('0.55.0', '20261029', ['v0.54.0-nightly.20261029'])).toBe(
      'v0.55.0-nightly.20261029',
    );
  });

  it('프리릴리스 버전 위에는 달지 않는다', () => {
    expect(() => nightlyTag('0.24.0-next.1', '20261029', [])).toThrow(/x\.y\.z/);
  });
});

describe('parseLsRemoteTags', () => {
  it('annotated 태그는 ^{} 줄의 커밋을 쓴다', () => {
    const tags = parseLsRemoteTags(
      [
        `${OLD}\trefs/tags/v0.54.0-nightly.20261028`,
        `1111111111111111111111111111111111111111\trefs/tags/v0.54.0-nightly.20261029`,
        `${HEAD}\trefs/tags/v0.54.0-nightly.20261029^{}`,
        '',
      ].join('\n'),
    );
    expect(tags.get('v0.54.0-nightly.20261028')).toBe(OLD);
    expect(tags.get('v0.54.0-nightly.20261029')).toBe(HEAD);
  });
});

describe('latestNightly', () => {
  it('버전이 아니라 날짜·순번으로 고른다', () => {
    const tags = new Map([
      ['v0.55.0-nightly.20261027', OLD],
      ['v0.54.0-nightly.20261029', HEAD],
      ['v0.54.0', OLD],
    ]);
    expect(latestNightly(tags)).toEqual({ tag: 'v0.54.0-nightly.20261029', sha: HEAD });
    expect(latestNightly(new Map([['v0.54.0', OLD]]))).toBeNull();
  });
});

describe('planNightly', () => {
  const base = {
    head: HEAD,
    version: '0.54.0',
    date: '20261029',
    tags: new Map([['v0.54.0-nightly.20261028', OLD]]),
    ci: green,
  };

  it('새 커밋이고 CI 가 초록이면 빌드한다', () => {
    expect(planNightly(base)).toEqual({
      build: true,
      tag: 'v0.54.0-nightly.20261029',
      version: '0.54.0-nightly.20261029',
      reason: 'main a1b2c3d → v0.54.0-nightly.20261029',
    });
  });

  it('지난 nightly 와 같은 커밋이면 건너뛴다 — 수동 실행은 다시 낸다', () => {
    const same = { ...base, tags: new Map([['v0.54.0-nightly.20261028', HEAD]]) };
    expect(planNightly(same).build).toBe(false);
    expect(planNightly({ ...same, force: true }).build).toBe(true);
  });

  it('CI 가 초록이 아니면 force 여도 건너뛴다', () => {
    for (const ci of [
      null,
      { status: 'in_progress', conclusion: null },
      { status: 'completed', conclusion: 'failure' },
    ]) {
      expect(planNightly({ ...base, ci }).build).toBe(false);
      expect(planNightly({ ...base, ci, force: true }).build).toBe(false);
    }
  });

  it('pre 모드(-next) 버전이면 빌드하지 않고 이유를 남긴다', () => {
    const plan = planNightly({ ...base, version: '0.55.0-next.0' });
    expect(plan).toEqual({
      build: false,
      reason: '버전 0.55.0-next.0 이 프리릴리스라 nightly 를 달지 않는다',
    });
  });
});

describe('prunableTags', () => {
  it('최근 keep 개만 남기고 nightly 만 고른다', () => {
    const tags = [
      'v0.54.0',
      'v0.24.0-next.3',
      'v0.54.0-nightly.20261025',
      'v0.54.0-nightly.20261029',
      'v0.54.0-nightly.20261026',
      'v0.54.0-nightly.20261029.2',
    ];
    expect(prunableTags(tags, 2)).toEqual(['v0.54.0-nightly.20261025', 'v0.54.0-nightly.20261026']);
    expect(prunableTags(tags, 7)).toEqual([]);
  });
});
