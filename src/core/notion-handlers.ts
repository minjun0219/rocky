import {
  type NotionCache,
  type NotionCacheStatus,
  type NotionPageResult,
  resolveCacheKey,
} from './notion-cache';
import {
  chunkNotionMarkdown,
  extractActionItems,
  type NotionActionExtraction,
  type NotionChunkSummary,
  summarizeNotionChunks,
} from './notion-chunking';
import { type NotionCliExecutor, notionCliFetch } from './notion-cli';
import { diffMarkdownBySection } from './notion-diff';

/**
 * notion_* 도구가 공유하는 handler 묶음 (openapi 의 `handlers.ts` 와 동일 역할).
 *
 * 진입점 (`src/index.ts`) 이 캐시 인스턴스 + CLI executor 를 주입해 호출한다. fetch 는
 * 전부 `notionCliFetch` (= `ntn pages get`) 를 통해서만 일어나고, 캐시는 파일시스템에만
 * 존재한다. remote 호출 정책: read → 캐시 hit 이면 CLI 미호출, miss / 만료면 1회 fetch.
 */

/**
 * remote 페이지를 fetch 해서 캐시에 기록한다. remote 가 요청과 다른 페이지를 돌려주면
 * (id 불일치) 잘못된 키 아래 캐시되는 사고를 막기 위해 거부한다.
 */
async function fetchAndCache(
  cache: NotionCache,
  exec: NotionCliExecutor,
  input: string,
): Promise<NotionPageResult> {
  const { pageId } = resolveCacheKey(input);
  const raw = await notionCliFetch(exec, input);
  const rawNormalized = resolveCacheKey(raw.id).pageId;
  if (rawNormalized !== pageId) {
    throw new Error(
      `Notion CLI returned wrong page (requested ${pageId}, got ${rawNormalized}) — refusing to cache`,
    );
  }
  const written = await cache.write(input, raw);
  return { ...written, fromCache: false };
}

/** 도구 핸들러: 캐시 우선. hit 이면 CLI 미호출. */
export async function handleNotionGet(
  cache: NotionCache,
  exec: NotionCliExecutor,
  input: string,
): Promise<NotionPageResult> {
  const cached = await cache.read(input);
  if (cached) {
    return { ...cached, fromCache: true };
  }
  return fetchAndCache(cache, exec, input);
}

/**
 * 도구 핸들러: 캐시를 무시하고 강제 재fetch. 기존 캐시가 있으면 heading-section 단위 diff 를
 * 함께 반환해 긴 기획서에서 바뀐 위치를 위에서부터 확인할 수 있게 한다.
 */
export async function handleNotionRefresh(
  cache: NotionCache,
  exec: NotionCliExecutor,
  input: string,
): Promise<NotionPageResult> {
  const previous = await cache.readAny(input);
  const refreshed = await fetchAndCache(cache, exec, input);
  if (!previous) {
    return refreshed;
  }
  return {
    ...refreshed,
    diff: diffMarkdownBySection(previous.markdown, refreshed.markdown),
  };
}

/** 도구 핸들러: 캐시 메타 + 만료 여부만. CLI 미호출. */
export async function handleNotionStatus(
  cache: NotionCache,
  input: string,
): Promise<NotionCacheStatus> {
  return cache.status(input);
}

export interface NotionExtractResult {
  entry: NotionPageResult['entry'];
  fromCache: boolean;
  chunkCount: number;
  chunks: NotionChunkSummary[];
  extracted: NotionActionExtraction;
}

/** 도구 핸들러: 캐시 우선으로 읽고 긴 문서용 청크 + 구현 액션 후보를 반환한다. */
export async function handleNotionExtract(
  cache: NotionCache,
  exec: NotionCliExecutor,
  input: string,
  options: { maxCharsPerChunk?: number } = {},
): Promise<NotionExtractResult> {
  const page = await handleNotionGet(cache, exec, input);
  const chunks = chunkNotionMarkdown(page.markdown, {
    maxCharsPerChunk: options.maxCharsPerChunk,
  });
  return {
    entry: page.entry,
    fromCache: page.fromCache,
    chunkCount: chunks.length,
    chunks: summarizeNotionChunks(chunks),
    extracted: extractActionItems(chunks),
  };
}
