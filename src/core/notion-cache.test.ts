import { describe, it, expect, beforeEach } from 'bun:test';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { NotionCache, resolveCacheKey, notionToMarkdown, contentHash } from './notion-cache';

const PAGE = '1234abcd1234abcd1234abcd1234abcd';
const PAGE_DASHED = '1234abcd-1234-abcd-1234-abcd1234abcd';

let dir: string;
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'agent-toolkit-cache-'));
});

describe('resolveCacheKey', () => {
  it('extracts page id from raw 32-char hex', () => {
    expect(resolveCacheKey(PAGE).pageId).toBe(PAGE_DASHED);
  });
  it('extracts page id from notion url', () => {
    const url = `https://www.notion.so/team/Some-Title-${PAGE}?pvs=4`;
    expect(resolveCacheKey(url).pageId).toBe(PAGE_DASHED);
  });
  it('accepts dash-form input', () => {
    expect(resolveCacheKey(PAGE_DASHED).pageId).toBe(PAGE_DASHED);
  });
  it('rejects garbage input', () => {
    expect(() => resolveCacheKey('not-a-page')).toThrow();
  });
});

describe('notionToMarkdown', () => {
  it('uses markdown field when present', () => {
    expect(notionToMarkdown({ id: 'x', title: 't', markdown: '# Hello' })).toBe('# Hello');
  });
  it('renders a few common block types', () => {
    const md = notionToMarkdown({
      id: 'x',
      title: 't',
      blocks: [
        { type: 'heading_1', text: 'Title' },
        { type: 'paragraph', text: 'Hello' },
        { type: 'bulleted_list_item', text: 'a' },
        { type: 'to_do', text: 'b' },
      ],
    });
    expect(md).toContain('# Title');
    expect(md).toContain('Hello');
    expect(md).toContain('- a');
    expect(md).toContain('- [ ] b');
  });
});

describe('NotionCache', () => {
  it('returns null for missing pages', async () => {
    const cache = new NotionCache({ baseDir: dir, defaultTtlSeconds: 60 });
    expect(await cache.read(PAGE)).toBeNull();
  });

  it('writes and reads back, with stable contentHash', async () => {
    const cache = new NotionCache({ baseDir: dir, defaultTtlSeconds: 60 });
    const w = await cache.write(PAGE, {
      id: PAGE,
      title: 'T',
      markdown: '# T',
    });
    expect(w.entry.title).toBe('T');
    const r = await cache.read(PAGE);
    expect(r?.markdown).toBe('# T');
    expect(r?.entry.contentHash).toBe(contentHash('# T'));
  });

  it('treats expired entries as miss but still reports them via status', async () => {
    const cache = new NotionCache({ baseDir: dir, defaultTtlSeconds: 60 });
    await cache.write(PAGE, { id: PAGE, title: 'T', markdown: '# T' });
    expect(await cache.invalidate(PAGE)).toBe(true);
    expect(await cache.read(PAGE)).toBeNull();
    const s = await cache.status(PAGE);
    expect(s.exists).toBe(true);
    expect(s.expired).toBe(true);
  });

  it('status reports exists=false when only .json exists (missing .md)', async () => {
    const cache = new NotionCache({ baseDir: dir, defaultTtlSeconds: 60 });
    await cache.write(PAGE, { id: PAGE, title: 'T', markdown: '# T' });
    rmSync(join(dir, `${PAGE_DASHED}.md`));
    const s = await cache.status(PAGE);
    expect(s.exists).toBe(false);
    expect(await cache.read(PAGE)).toBeNull();
  });

  it('status reports exists=false when only .md exists (missing .json)', async () => {
    const cache = new NotionCache({ baseDir: dir, defaultTtlSeconds: 60 });
    await cache.write(PAGE, { id: PAGE, title: 'T', markdown: '# T' });
    rmSync(join(dir, `${PAGE_DASHED}.json`));
    const s = await cache.status(PAGE);
    expect(s.exists).toBe(false);
  });

  it('treats corrupt json as miss without throwing', async () => {
    const cache = new NotionCache({ baseDir: dir, defaultTtlSeconds: 60 });
    await cache.write(PAGE, { id: PAGE, title: 'T', markdown: '# T' });
    writeFileSync(join(dir, `${PAGE_DASHED}.json`), '{ not json', 'utf8');
    expect(await cache.read(PAGE)).toBeNull();
    const s = await cache.status(PAGE);
    expect(s.exists).toBe(false);
  });
});
