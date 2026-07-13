import { beforeEach, describe, expect, it } from 'bun:test';
import { appendFileSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import {
  AgentJournal,
  createJournalFromEnv,
  defaultProjectKey,
  expandTilde,
  JOURNAL_FILE,
} from './journal';
import {
  handleJournalAppend,
  handleJournalRead,
  handleJournalSearch,
  handleJournalStatus,
} from './journal-handlers';

const PAGE = '1234abcd1234abcd1234abcd1234abcd';
const PAGE_DASHED = '1234abcd-1234-abcd-1234-abcd1234abcd';
const OTHER_PAGE = 'abcd1234abcd1234abcd1234abcd1234';

let dir: string;
let journal: AgentJournal;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'rocky-journal-'));
  journal = new AgentJournal({ baseDir: dir });
});

describe('AgentJournal.append', () => {
  it('writes a JSONL line with normalized fields', async () => {
    const entry = await journal.append({
      content: '  decided to use Bun  ',
      kind: 'decision',
      tags: [' notion ', '', 'infra'],
      pageId: PAGE,
    });
    expect(entry.content).toBe('decided to use Bun');
    expect(entry.kind).toBe('decision');
    expect(entry.tags).toEqual(['notion', 'infra']);
    expect(entry.pageId).toBe(PAGE_DASHED);
    expect(entry.id).toMatch(/^\d+-[0-9a-f]{6}$/);
    expect(entry.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });

  it("defaults kind to 'note' and tags to []", async () => {
    const entry = await journal.append({ content: 'blocker' });
    expect(entry.kind).toBe('note');
    expect(entry.tags).toEqual([]);
    expect(entry.pageId).toBeUndefined();
  });

  it('rejects empty / whitespace content', async () => {
    await expect(journal.append({ content: '' })).rejects.toThrow(/non-empty/i);
    await expect(journal.append({ content: '   ' })).rejects.toThrow(/non-empty/i);
  });

  it('rejects an invalid pageId string via resolveCacheKey', async () => {
    await expect(journal.append({ content: 'x', pageId: 'not-a-page' })).rejects.toThrow(
      /Notion page id/,
    );
  });
});

describe('AgentJournal.read', () => {
  it('returns most recent first up to limit (default 20)', async () => {
    for (let i = 0; i < 25; i += 1) {
      await journal.append({ content: `entry ${i}` });
    }
    const recent = await journal.read();
    expect(recent.length).toBe(20);
    expect(recent[0]?.content).toBe('entry 24');
    expect(recent[19]?.content).toBe('entry 5');
  });

  it('filters by kind', async () => {
    await journal.append({ content: 'a', kind: 'decision' });
    await journal.append({ content: 'b', kind: 'blocker' });
    await journal.append({ content: 'c', kind: 'decision' });
    const r = await journal.read({ kind: 'decision' });
    expect(r.map((e) => e.content)).toEqual(['c', 'a']);
  });

  it('filters by tag (exact membership)', async () => {
    await journal.append({ content: 'a', tags: ['api', 'review'] });
    await journal.append({ content: 'b', tags: ['review'] });
    await journal.append({ content: 'c', tags: ['api'] });
    const r = await journal.read({ tag: 'api' });
    expect(r.map((e) => e.content)).toEqual(['c', 'a']);
  });

  it('filters by pageId after normalization', async () => {
    await journal.append({ content: 'a', pageId: PAGE });
    await journal.append({ content: 'b', pageId: OTHER_PAGE });
    await journal.append({ content: 'c', pageId: PAGE_DASHED });
    const r = await journal.read({
      pageId: `https://www.notion.so/team/Title-${PAGE}`,
    });
    expect(r.map((e) => e.content)).toEqual(['c', 'a']);
    expect(r.every((e) => e.pageId === PAGE_DASHED)).toBe(true);
  });

  it('filters by since (strictly after)', async () => {
    const a = await journal.append({ content: 'before' });
    await new Promise((r) => setTimeout(r, 5));
    const after = new Date().toISOString();
    await new Promise((r) => setTimeout(r, 5));
    await journal.append({ content: 'after-1' });
    await journal.append({ content: 'after-2' });
    const r = await journal.read({ since: after });
    expect(r.map((e) => e.content)).toEqual(['after-2', 'after-1']);
    expect(r.every((e) => e.id !== a.id)).toBe(true);
  });

  it('returns [] when journal does not exist yet', async () => {
    expect(await journal.read()).toEqual([]);
  });
});

describe('AgentJournal.search', () => {
  beforeEach(async () => {
    await journal.append({ content: 'Decided to use Bun', kind: 'decision' });
    await journal.append({ content: 'Blocked on auth', kind: 'blocker' });
    await journal.append({
      content: 'User confirmed PRD',
      kind: 'answer',
      tags: ['prd'],
    });
    await journal.append({ content: 'linked page', pageId: PAGE });
  });

  it('matches content substring case-insensitively', async () => {
    const r = await journal.search('BUN');
    expect(r.length).toBe(1);
    expect(r[0]?.content).toBe('Decided to use Bun');
  });

  it('matches by tag', async () => {
    const r = await journal.search('prd');
    expect(r.length).toBe(1);
    expect(r[0]?.kind).toBe('answer');
  });

  it('matches by pageId substring', async () => {
    const r = await journal.search(PAGE_DASHED.slice(0, 8));
    expect(r.length).toBe(1);
    expect(r[0]?.pageId).toBe(PAGE_DASHED);
  });

  it('kind filter scopes the pool before substring match', async () => {
    const r = await journal.search('', { kind: 'blocker' });
    expect(r.length).toBe(1);
    expect(r[0]?.content).toBe('Blocked on auth');
  });

  it('respects limit', async () => {
    for (let i = 0; i < 30; i += 1) {
      await journal.append({ content: `noise ${i}` });
    }
    const r = await journal.search('noise', { limit: 3 });
    expect(r.length).toBe(3);
  });
});

describe('AgentJournal.status', () => {
  it('reports exists=false before any writes', async () => {
    const s = await journal.status();
    expect(s.exists).toBe(false);
    expect(s.totalEntries).toBe(0);
    expect(s.sizeBytes).toBe(0);
    expect(s.lastEntryAt).toBeUndefined();
    // 출처 힌트는 write 전에도 항상 노출된다.
    expect(s.dirSource).toBe('config');
    expect(s.wikiDirSource).toBe('unset');
  });

  it('reports totalEntries / lastEntryAt after writes', async () => {
    await journal.append({ content: 'a' });
    const last = await journal.append({ content: 'b' });
    const s = await journal.status();
    expect(s.exists).toBe(true);
    expect(s.totalEntries).toBe(2);
    expect(s.sizeBytes).toBeGreaterThan(0);
    expect(s.lastEntryAt).toBe(last.timestamp);
  });

  it('surfaces wikiDir / projectKey / lastCurateAt for the curate workflow', async () => {
    const withWiki = new AgentJournal({
      baseDir: dir,
      wikiDir: '/tmp/vault',
      projectKey: 'myproj-deadbeef',
    });
    await withWiki.append({ content: 'a decision', kind: 'decision' });
    const before = await withWiki.status();
    expect(before.wikiDir).toBe(resolve('/tmp/vault'));
    expect(before.projectKey).toBe('myproj-deadbeef');
    expect(before.lastCurateAt).toBeUndefined();

    const mark = await withWiki.append({ content: 'curated 1 page', kind: 'curate' });
    const after = await withWiki.status();
    expect(after.lastCurateAt).toBe(mark.timestamp);
  });

  it('includes projectKey even before any writes', async () => {
    const j = new AgentJournal({ baseDir: dir, projectKey: 'x-12345678' });
    const s = await j.status();
    expect(s.exists).toBe(false);
    expect(s.projectKey).toBe('x-12345678');
  });

  it('surfaces explicit dirSource / wikiDirSource unchanged before and after writes', async () => {
    const j = new AgentJournal({
      baseDir: dir,
      wikiDir: '/tmp/vault',
      dirSource: 'env',
      wikiDirSource: 'config',
    });
    const before = await j.status();
    expect(before.dirSource).toBe('env');
    expect(before.wikiDirSource).toBe('config');
    await j.append({ content: 'a' });
    const after = await j.status();
    expect(after.dirSource).toBe('env');
    expect(after.wikiDirSource).toBe('config');
  });

  it('infers dirSource=default / wikiDirSource=unset when neither is provided', async () => {
    const j = new AgentJournal({ projectKey: 'x-12345678' });
    const s = await j.status();
    expect(s.dirSource).toBe('default');
    expect(s.wikiDirSource).toBe('unset');
    expect(s.wikiDir).toBeUndefined();
  });

  it('clamps wikiDirSource=unset to config when wikiDir is present (invariant)', async () => {
    const j = new AgentJournal({ baseDir: dir, wikiDir: '/tmp/vault', wikiDirSource: 'unset' });
    const s = await j.status();
    expect(s.wikiDir).toBe(resolve('/tmp/vault'));
    // wikiDir 이 있으면 'unset' 은 불가능 — 'config' 로 교정된다.
    expect(s.wikiDirSource).toBe('config');
  });

  it('clamps wikiDirSource=env to unset when wikiDir is absent (invariant)', async () => {
    const j = new AgentJournal({ baseDir: dir, wikiDirSource: 'env' });
    const s = await j.status();
    expect(s.wikiDir).toBeUndefined();
    // wikiDir 이 없으면 넘어온 값 무시하고 항상 'unset'.
    expect(s.wikiDirSource).toBe('unset');
  });

  it('clamps dirSource=default to config when baseDir is present (invariant)', async () => {
    const j = new AgentJournal({ baseDir: dir, dirSource: 'default' });
    const s = await j.status();
    // baseDir 이 있으면 'default' 는 불가능 — 'config' 로 교정된다.
    expect(s.dirSource).toBe('config');
  });

  it('preserves a valid explicit env source (no over-clamping)', async () => {
    const j = new AgentJournal({
      baseDir: dir,
      wikiDir: '/tmp/vault',
      dirSource: 'env',
      wikiDirSource: 'env',
    });
    const s = await j.status();
    expect(s.dirSource).toBe('env');
    expect(s.wikiDirSource).toBe('env');
  });
});

describe('expandTilde', () => {
  it('expands a leading ~ / ~/ to the home directory', () => {
    expect(expandTilde('~')).toBe(homedir());
    expect(expandTilde('~/Obsidian/vault')).toBe(join(homedir(), 'Obsidian/vault'));
  });

  it('leaves absolute / relative / mid-string tildes untouched', () => {
    expect(expandTilde('/abs/path')).toBe('/abs/path');
    expect(expandTilde('rel/path')).toBe('rel/path');
    expect(expandTilde('/x/~y')).toBe('/x/~y');
  });

  it('is applied to baseDir / wikiDir so ~ paths resolve under home', () => {
    const j = new AgentJournal({ baseDir: '~/rocky-j-test', wikiDir: '~/rocky-v-test' });
    expect(j.getDir()).toBe(join(homedir(), 'rocky-j-test'));
    expect(j.getWikiDir()).toBe(join(homedir(), 'rocky-v-test'));
  });
});

describe('createJournalFromEnv', () => {
  it('lets ROCKY_JOURNAL_DIR / ROCKY_JOURNAL_WIKI_DIR win over config', async () => {
    const prevDir = process.env.ROCKY_JOURNAL_DIR;
    const prevWiki = process.env.ROCKY_JOURNAL_WIKI_DIR;
    try {
      process.env.ROCKY_JOURNAL_DIR = dir;
      process.env.ROCKY_JOURNAL_WIKI_DIR = '/tmp/env-vault';
      const j = createJournalFromEnv({ dir: '/tmp/config-dir', wikiDir: '/tmp/config-vault' });
      expect(j.getDir()).toBe(resolve(dir));
      expect(j.getWikiDir()).toBe(resolve('/tmp/env-vault'));
      const s = await j.status();
      expect(s.dirSource).toBe('env');
      expect(s.wikiDirSource).toBe('env');
    } finally {
      restoreEnv('ROCKY_JOURNAL_DIR', prevDir);
      restoreEnv('ROCKY_JOURNAL_WIKI_DIR', prevWiki);
    }
  });

  it('falls back to config when env is unset', async () => {
    const prevDir = process.env.ROCKY_JOURNAL_DIR;
    const prevWiki = process.env.ROCKY_JOURNAL_WIKI_DIR;
    try {
      delete process.env.ROCKY_JOURNAL_DIR;
      delete process.env.ROCKY_JOURNAL_WIKI_DIR;
      const j = createJournalFromEnv({ dir });
      expect(j.getDir()).toBe(resolve(dir));
      const s = await j.status();
      expect(s.dirSource).toBe('config');
      // wikiDir 은 config 에도 없으니 unset.
      expect(s.wikiDirSource).toBe('unset');
    } finally {
      restoreEnv('ROCKY_JOURNAL_DIR', prevDir);
      restoreEnv('ROCKY_JOURNAL_WIKI_DIR', prevWiki);
    }
  });

  it('reports dirSource=default when neither env nor config is set', async () => {
    const prevDir = process.env.ROCKY_JOURNAL_DIR;
    const prevWiki = process.env.ROCKY_JOURNAL_WIKI_DIR;
    try {
      delete process.env.ROCKY_JOURNAL_DIR;
      delete process.env.ROCKY_JOURNAL_WIKI_DIR;
      const j = createJournalFromEnv();
      const s = await j.status();
      expect(s.dirSource).toBe('default');
      expect(s.wikiDirSource).toBe('unset');
    } finally {
      restoreEnv('ROCKY_JOURNAL_DIR', prevDir);
      restoreEnv('ROCKY_JOURNAL_WIKI_DIR', prevWiki);
    }
  });
});

describe('defaultProjectKey', () => {
  it('is a stable sanitized-basename + short hash', () => {
    const a = defaultProjectKey('/Users/x/my project!/app');
    const b = defaultProjectKey('/Users/x/my project!/app');
    expect(a).toBe(b);
    expect(a).toMatch(/^app-[0-9a-f]{8}$/);
  });

  it('distinguishes same basename under different paths', () => {
    expect(defaultProjectKey('/a/app')).not.toBe(defaultProjectKey('/b/app'));
  });
});

describe('journal handlers (thin wrappers)', () => {
  it('append / read / search / status delegate to the journal', async () => {
    await handleJournalAppend(journal, { content: 'via handler', kind: 'decision' });
    const read = await handleJournalRead(journal, { kind: 'decision' });
    expect(read[0]?.content).toBe('via handler');
    const found = await handleJournalSearch(journal, 'handler');
    expect(found.length).toBe(1);
    const status = await handleJournalStatus(journal);
    expect(status.totalEntries).toBe(1);
  });
});

describe('graceful degradation', () => {
  it('skips corrupt JSON lines but keeps valid ones', async () => {
    await journal.append({ content: 'first' });
    appendFileSync(join(dir, JOURNAL_FILE), '{ this is not json\n', 'utf8');
    await journal.append({ content: 'second' });
    const r = await journal.read();
    expect(r.map((e) => e.content)).toEqual(['second', 'first']);
  });

  it('skips entries missing required fields', async () => {
    await journal.append({ content: 'valid' });
    appendFileSync(
      join(dir, JOURNAL_FILE),
      `${JSON.stringify({ id: 'x', timestamp: 'now' })}\n`,
      'utf8',
    );
    const r = await journal.read();
    expect(r.map((e) => e.content)).toEqual(['valid']);
  });

  it('tolerates a trailing partial line (no newline)', async () => {
    await journal.append({ content: 'first' });
    appendFileSync(
      join(dir, JOURNAL_FILE),
      `${JSON.stringify({ id: 'p', timestamp: 'now', kind: 'note', content: 'partial' }).slice(0, 20)}`,
      'utf8',
    );
    const r = await journal.read();
    expect(r.map((e) => e.content)).toEqual(['first']);
  });

  it('does not concatenate a new entry onto an unterminated last line', async () => {
    appendFileSync(join(dir, JOURNAL_FILE), '{"id":"crashed","timestamp":"', 'utf8');
    const fresh = await journal.append({ content: 'after-restart' });
    const r = await journal.read();
    expect(r.length).toBe(1);
    expect(r[0]?.id).toBe(fresh.id);
    expect(r[0]?.content).toBe('after-restart');
  });

  it('returns [] on entirely garbage file without throwing', async () => {
    writeFileSync(join(dir, JOURNAL_FILE), 'garbage\n}{also garbage\n', 'utf8');
    expect(await journal.read()).toEqual([]);
    const s = await journal.status();
    expect(s.exists).toBe(true);
    expect(s.totalEntries).toBe(0);
  });

  it('rethrows non-ENOENT IO errors instead of silently returning [] (Copilot)', async () => {
    // journal.jsonl 경로를 디렉터리로 만들어 readFile 이 EISDIR(비-ENOENT)로 실패하게 한다.
    // 실제 권한/IO 오류를 [] 로 삼키면 저널이 사라진 것처럼 보이므로 표면화되어야 한다.
    mkdirSync(join(dir, JOURNAL_FILE));
    await expect(journal.read()).rejects.toThrow();
    await expect(journal.search('x')).rejects.toThrow();
    await expect(journal.status()).rejects.toThrow();
  });
});

function restoreEnv(key: string, prior: string | undefined): void {
  if (prior === undefined) {
    delete process.env[key];
  } else {
    process.env[key] = prior;
  }
}
