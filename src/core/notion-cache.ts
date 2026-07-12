import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

/**
 * Notion 페이지 캐시 + 정규화를 한 파일에 모은다.
 *
 * 디스크 레이아웃 (한 페이지당 두 파일):
 *   <baseDir>/<pageId>.json   메타데이터 (NotionCacheEntry)
 *   <baseDir>/<pageId>.md     normalize 된 markdown 본문
 *
 * baseDir 기본값은 `~/.config/rocky/notion-pages` — rocky 의 다른 상태 (`~/.config/rocky/`)
 * 와 같은 곳에 둔다. 프로젝트별로 격리하고 싶으면 `ROCKY_NOTION_CACHE_DIR` 로 덮어쓴다.
 */

export const NOTION_DEFAULT_CACHE_DIR = join(homedir(), '.config', 'rocky', 'notion-pages');
export const NOTION_DEFAULT_TTL_SECONDS = 60 * 60 * 24; // 24h

export interface NotionCacheEntry {
  pageId: string;
  url: string;
  cachedAt: string;
  ttlSeconds: number;
  contentHash: string;
  title: string;
}

export interface NotionCacheStatus {
  pageId: string;
  exists: boolean;
  expired: boolean;
  cachedAt?: string;
  ttlSeconds?: number;
  ageSeconds?: number;
  title?: string;
}

export interface NotionPageResult {
  entry: NotionCacheEntry;
  markdown: string;
  fromCache: boolean;
  diff?: import('./notion-diff').NotionMarkdownDiff;
}

export interface RawNotionPage {
  id: string;
  title: string;
  markdown?: string;
  blocks?: Array<{ type: string; text?: string }>;
}

export interface NotionCacheOptions {
  baseDir?: string;
  defaultTtlSeconds?: number;
}

/**
 * Notion page id / url 에서 정규 page id 와 디스크 키를 뽑는다.
 *
 * - 32자 hex (dash 유무 무관) 추출 → dash 포함 8-4-4-4-12 형식으로 정규화.
 * - 추출 실패 시 던진다 (호출 측에서 4xx 로 매핑 가능하도록 메시지에 입력값 포함).
 */
export function resolveCacheKey(input: string): { pageId: string; key: string } {
  if (!input || typeof input !== 'string') {
    throw new Error('resolveCacheKey: input must be a non-empty string');
  }
  const trimmed = input.trim();
  const hexWithDash = trimmed.match(
    /[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/,
  );
  const hexNoDash = trimmed.match(/[0-9a-fA-F]{32}/);
  const raw = hexWithDash ? hexWithDash[0].replace(/-/g, '') : hexNoDash ? hexNoDash[0] : undefined;
  if (raw?.length !== 32) {
    throw new Error(`resolveCacheKey: cannot extract Notion page id from input "${input}"`);
  }
  const lower = raw.toLowerCase();
  const pageId = `${lower.slice(0, 8)}-${lower.slice(8, 12)}-${lower.slice(12, 16)}-${lower.slice(
    16,
    20,
  )}-${lower.slice(20)}`;
  return { pageId, key: pageId };
}

/** 짧은(앞 16자) sha256 — 본문 동일성 비교용. */
export function contentHash(content: string): string {
  return createHash('sha256').update(content, 'utf8').digest('hex').slice(0, 16);
}

/**
 * Notion raw payload → markdown.
 * markdown 필드가 있으면 그대로, 아니면 blocks 를 최소 변환.
 */
export function notionToMarkdown(page: RawNotionPage): string {
  if (page.markdown && page.markdown.trim().length > 0) {
    return page.markdown.trim();
  }
  if (!page.blocks || page.blocks.length === 0) {
    return '';
  }
  const lines: string[] = [];
  for (const block of page.blocks) {
    const text = (block.text ?? '').trim();
    if (!text) {
      continue;
    }
    switch (block.type) {
      case 'heading_1':
        lines.push(`# ${text}`);
        break;
      case 'heading_2':
        lines.push(`## ${text}`);
        break;
      case 'heading_3':
        lines.push(`### ${text}`);
        break;
      case 'bulleted_list_item':
        lines.push(`- ${text}`);
        break;
      case 'numbered_list_item':
        lines.push(`1. ${text}`);
        break;
      case 'to_do':
        lines.push(`- [ ] ${text}`);
        break;
      case 'quote':
        lines.push(`> ${text}`);
        break;
      case 'code':
        lines.push('```', text, '```');
        break;
      default:
        lines.push(text);
        break;
    }
    lines.push('');
  }
  return lines.join('\n').trim();
}

/**
 * 파일시스템 기반 Notion 페이지 캐시.
 *
 * 외부에 노출되는 메서드는 read / readAny / write / status / invalidate.
 *
 * read 와 status 둘 다 `.json` 과 `.md` 가 모두 존재해야 hit 으로 간주한다 —
 * 한 쪽만 남아있으면 손상 상태로 보고 cache miss 처리.
 */
export class NotionCache {
  private readonly dir: string;
  private readonly defaultTtl: number;

  constructor(options: NotionCacheOptions = {}) {
    this.dir = resolve(options.baseDir ?? NOTION_DEFAULT_CACHE_DIR);
    this.defaultTtl = options.defaultTtlSeconds ?? NOTION_DEFAULT_TTL_SECONDS;
  }

  getDir(): string {
    return this.dir;
  }

  /** hit 이면 entry+markdown, miss/만료/손상이면 null. */
  async read(input: string): Promise<{ entry: NotionCacheEntry; markdown: string } | null> {
    const cached = await this.readAny(input);
    if (!cached || this.isExpired(cached.entry)) {
      return null;
    }
    return cached;
  }

  /** ttl 만료 여부와 무관하게 캐시 파일이 있으면 읽는다. refresh diff 계산용. */
  async readAny(input: string): Promise<{ entry: NotionCacheEntry; markdown: string } | null> {
    const { key } = resolveCacheKey(input);
    const jsonPath = join(this.dir, `${key}.json`);
    const mdPath = join(this.dir, `${key}.md`);
    if (!existsSync(jsonPath) || !existsSync(mdPath)) {
      return null;
    }
    try {
      const entry = JSON.parse(await readFile(jsonPath, 'utf8')) as NotionCacheEntry;
      const markdown = await readFile(mdPath, 'utf8');
      return { entry, markdown };
    } catch {
      return null;
    }
  }

  /**
   * raw 페이지를 normalize 후 캐시에 기록.
   * 주의: 호출 측이 raw.id 와 요청 page id 의 일치를 사전 검증해야 한다 (이 클래스는 검증하지 않음).
   */
  async write(
    input: string,
    page: RawNotionPage,
    ttlSeconds?: number,
  ): Promise<{ entry: NotionCacheEntry; markdown: string }> {
    const { pageId, key } = resolveCacheKey(input);
    const markdown = notionToMarkdown(page);
    const entry: NotionCacheEntry = {
      pageId,
      url: input,
      cachedAt: new Date().toISOString(),
      ttlSeconds: ttlSeconds ?? this.defaultTtl,
      contentHash: contentHash(markdown),
      title: page.title || '(untitled)',
    };
    await mkdir(this.dir, { recursive: true });
    await writeFile(join(this.dir, `${key}.json`), `${JSON.stringify(entry, null, 2)}\n`, 'utf8');
    await writeFile(join(this.dir, `${key}.md`), markdown, 'utf8');
    return { entry, markdown };
  }

  /**
   * 캐시 메타 + 만료 여부.
   * `.json` 또는 `.md` 한쪽이라도 없으면 exists=false 로 보고한다 — read 와 일관.
   */
  async status(input: string): Promise<NotionCacheStatus> {
    const { pageId, key } = resolveCacheKey(input);
    const jsonPath = join(this.dir, `${key}.json`);
    const mdPath = join(this.dir, `${key}.md`);
    if (!existsSync(jsonPath) || !existsSync(mdPath)) {
      return { pageId, exists: false, expired: false };
    }
    try {
      const entry = JSON.parse(await readFile(jsonPath, 'utf8')) as NotionCacheEntry;
      const ageSeconds = Math.max(
        0,
        Math.floor((Date.now() - new Date(entry.cachedAt).getTime()) / 1000),
      );
      return {
        pageId,
        exists: true,
        expired: this.isExpired(entry),
        cachedAt: entry.cachedAt,
        ttlSeconds: entry.ttlSeconds,
        ageSeconds,
        title: entry.title,
      };
    } catch {
      return { pageId, exists: false, expired: false };
    }
  }

  /** ttl 을 0 으로 갱신해 다음 read 에서 miss 처리. */
  async invalidate(input: string): Promise<boolean> {
    const status = await this.status(input);
    if (!status.exists) {
      return false;
    }
    const { key } = resolveCacheKey(input);
    const jsonPath = join(this.dir, `${key}.json`);
    const entry = JSON.parse(await readFile(jsonPath, 'utf8')) as NotionCacheEntry;
    entry.ttlSeconds = 0;
    await writeFile(jsonPath, `${JSON.stringify(entry, null, 2)}\n`, 'utf8');
    return true;
  }

  private isExpired(entry: NotionCacheEntry): boolean {
    if (entry.ttlSeconds <= 0) {
      return true;
    }
    const cachedAtMs = new Date(entry.cachedAt).getTime();
    if (!Number.isFinite(cachedAtMs)) {
      return true;
    }
    return Date.now() >= cachedAtMs + entry.ttlSeconds * 1000;
  }
}

/** `ROCKY_NOTION_CACHE_DIR` / `ROCKY_NOTION_CACHE_TTL` 로 캐시를 만든다. */
export function createNotionCacheFromEnv(): NotionCache {
  const baseDir = process.env.ROCKY_NOTION_CACHE_DIR;
  const ttlRaw = process.env.ROCKY_NOTION_CACHE_TTL;
  const ttl = ttlRaw ? Number.parseInt(ttlRaw, 10) : undefined;
  return new NotionCache({
    baseDir,
    defaultTtlSeconds: Number.isFinite(ttl) && (ttl as number) > 0 ? ttl : undefined,
  });
}
