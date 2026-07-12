/**
 * 긴 Notion markdown 을 구조 단위로 분해하고, 구현 액션 후보를 추출한다.
 *
 * 목적:
 * - 문서를 통째로 LLM 컨텍스트에 넣지 않고 청크 단위로 근거를 유지
 * - 구현 가능한 TODO / API 의존성 / 확인 필요 사항을 재사용 가능한 JSON으로 고정
 */

import { isClosingFence, parseFenceMarker } from './notion-diff';

export interface NotionChunk {
  id: string;
  headingPath: string[];
  startLine: number;
  endLine: number;
  text: string;
  approxTokens: number;
}

export interface NotionChunkSummary {
  id: string;
  headingPath: string[];
  startLine: number;
  endLine: number;
  approxTokens: number;
  preview: string;
}

export interface ExtractedItem {
  text: string;
  chunkId: string;
}

export interface NotionActionExtraction {
  requirements: ExtractedItem[];
  screens: ExtractedItem[];
  apis: ExtractedItem[];
  todos: ExtractedItem[];
  questions: ExtractedItem[];
}

export interface ChunkOptions {
  maxCharsPerChunk?: number;
}

const DEFAULT_MAX_CHARS = 1400;

function resolveMaxChars(value: number | undefined): number {
  if (value === undefined) {
    return DEFAULT_MAX_CHARS;
  }
  if (!Number.isFinite(value) || !Number.isInteger(value) || value < 1) {
    return DEFAULT_MAX_CHARS;
  }
  return value;
}

function approxTokens(text: string): number {
  return Math.ceil(text.length / 4);
}

function normalizeLine(line: string): string {
  return line.replace(/\s+/g, ' ').trim();
}

function hardSliceLine(line: string, maxChars: number): string[] {
  if (!Number.isFinite(maxChars) || !Number.isInteger(maxChars) || maxChars < 1) {
    throw new Error('maxChars must be a positive integer');
  }
  if (line.length <= maxChars) {
    return [line];
  }
  const out: string[] = [];
  for (let i = 0; i < line.length; i += maxChars) {
    out.push(line.slice(i, i + maxChars));
  }
  return out;
}

function makeChunk(
  id: string,
  headingPath: string[],
  startLine: number,
  endLine: number,
  text: string,
): NotionChunk {
  return {
    id,
    headingPath,
    startLine,
    endLine,
    text: text.trim(),
    approxTokens: approxTokens(text),
  };
}

function summarizeChunk(chunk: NotionChunk): NotionChunkSummary {
  return {
    id: chunk.id,
    headingPath: chunk.headingPath,
    startLine: chunk.startLine,
    endLine: chunk.endLine,
    approxTokens: chunk.approxTokens,
    preview: normalizeLine(chunk.text).slice(0, 160),
  };
}

/**
 * markdown 을 heading 중심으로 1차 분할 후, 큰 블록은 문단/줄 단위 재분할한다.
 */
export function chunkNotionMarkdown(markdown: string, options: ChunkOptions = {}): NotionChunk[] {
  const maxChars = resolveMaxChars(options.maxCharsPerChunk);
  const lines = markdown.split(/\r?\n/);

  interface Block {
    headingPath: string[];
    startLine: number;
    endLine: number;
    lines: string[];
  }

  const blocks: Block[] = [];
  let currentStart = 1;
  let currentPath: string[] = [];
  let stack: Array<{ level: number; title: string }> = [];
  // fence 안에서는 heading 매칭을 끈다. 여는 marker(백틱/틸드 + 길이) 를 기억해 같은 종류 +
  // 같거나 더 긴 fence 에서만 닫아 `~~~` / ````` 같은 변형도 안정적으로 무시한다 (notion-diff 와 동일).
  let fenceMarker: string | null = null;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i] ?? '';
    if (fenceMarker) {
      if (isClosingFence(line, fenceMarker)) {
        fenceMarker = null;
      }
      continue;
    }
    const openingFence = parseFenceMarker(line);
    if (openingFence) {
      fenceMarker = openingFence;
      continue;
    }

    const m = line.match(/^(#{1,6})\s+(.*)$/);
    if (!m) {
      continue;
    }

    if (i + 1 > currentStart) {
      blocks.push({
        headingPath: [...currentPath],
        startLine: currentStart,
        endLine: i,
        lines: lines.slice(currentStart - 1, i),
      });
    }

    const level = m[1]?.length ?? 1;
    const title = normalizeLine(m[2] ?? '');
    stack = stack.filter((h) => h.level < level);
    stack.push({ level, title });
    currentPath = stack.map((h) => h.title);
    currentStart = i + 2;
  }

  if (currentStart <= lines.length) {
    blocks.push({
      headingPath: [...currentPath],
      startLine: currentStart,
      endLine: lines.length,
      lines: lines.slice(currentStart - 1),
    });
  }

  const chunks: NotionChunk[] = [];
  let index = 1;

  for (const block of blocks) {
    let acc: string[] = [];
    let accStart = block.startLine;

    const flush = (endLine: number) => {
      const text = acc.join('\n').trim();
      if (!text) {
        acc = [];
        accStart = endLine + 1;
        return;
      }
      chunks.push(
        makeChunk(
          `chunk-${String(index).padStart(3, '0')}`,
          block.headingPath,
          accStart,
          endLine,
          text,
        ),
      );
      index += 1;
      acc = [];
      accStart = endLine + 1;
    };

    for (let offset = 0; offset < block.lines.length; offset += 1) {
      const line = block.lines[offset] ?? '';
      const lineNumber = block.startLine + offset;

      if (line.length > maxChars) {
        flush(lineNumber - 1);
        for (const part of hardSliceLine(line, maxChars)) {
          chunks.push(
            makeChunk(
              `chunk-${String(index).padStart(3, '0')}`,
              block.headingPath,
              lineNumber,
              lineNumber,
              part,
            ),
          );
          index += 1;
        }
        accStart = lineNumber + 1;
        continue;
      }

      const candidate = acc.length > 0 ? `${acc.join('\n')}\n${line}` : line;
      if (candidate.length > maxChars) {
        flush(lineNumber - 1);
        accStart = lineNumber;
      }
      acc.push(line);
    }
    flush(block.endLine);
  }

  return chunks;
}

/** 원문 전체를 반환하지 않는 tool-response 용 청크 메타데이터. */
export function summarizeNotionChunks(chunks: NotionChunk[]): NotionChunkSummary[] {
  return chunks.map(summarizeChunk);
}

function dedupe(items: ExtractedItem[]): ExtractedItem[] {
  const seen = new Set<string>();
  const out: ExtractedItem[] = [];
  for (const item of items) {
    const key = item.text.toLowerCase();
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    out.push(item);
  }
  return out;
}

function extractBullets(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => /^(-|\*|\d+\.)\s+/.test(line))
    .map((line) => line.replace(/^(-|\*|\d+\.)\s+/, '').trim())
    .filter(Boolean);
}

function stripFencedCode(text: string): string {
  const out: string[] = [];
  // chunkNotionMarkdown 과 동일한 fence 추적 — `~~~` / 길이 변형 fence 안의 예시 코드(TODO / API
  // 경로 등) 가 실제 문서 내용처럼 추출되지 않도록 fence marker 를 기억해 확실히 제거한다.
  let fenceMarker: string | null = null;
  for (const line of text.split('\n')) {
    if (fenceMarker) {
      if (isClosingFence(line, fenceMarker)) {
        fenceMarker = null;
      }
      continue;
    }
    const openingFence = parseFenceMarker(line);
    if (openingFence) {
      fenceMarker = openingFence;
      continue;
    }
    out.push(line);
  }
  return out.join('\n');
}

function isLikelyActionLine(line: string): boolean {
  if (!line) {
    return false;
  }
  if (line.startsWith('|') || line.startsWith('>')) {
    return false;
  }
  if (/^```/.test(line)) {
    return false;
  }
  return /\bTODO\b|구현|추가|연동|분리|리팩터|작성|반영|수정|지원/i.test(line);
}

/**
 * 청크 텍스트의 규칙 기반 분류로 구현 액션 후보를 뽑는다.
 * LLM 없이도 반복 가능한 최소 추출 파이프라인을 제공한다.
 */
export function extractActionItems(chunks: NotionChunk[]): NotionActionExtraction {
  const requirements: ExtractedItem[] = [];
  const screens: ExtractedItem[] = [];
  const apis: ExtractedItem[] = [];
  const todos: ExtractedItem[] = [];
  const questions: ExtractedItem[] = [];

  const apiRe = /\b(GET|POST|PUT|PATCH|DELETE)\s+\/[\w\-./{}:]+\b/g;

  for (const chunk of chunks) {
    const heading = chunk.headingPath.join(' > ').toLowerCase();
    const extractableText = stripFencedCode(chunk.text);
    const lines = extractableText
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
    const bullets = extractBullets(extractableText);

    for (const match of extractableText.matchAll(apiRe)) {
      const text = normalizeLine(match[0] ?? '');
      if (text) {
        apis.push({ text, chunkId: chunk.id });
      }
    }

    for (const line of lines) {
      const normalized = normalizeLine(line);
      if (!normalized) {
        continue;
      }

      if (/\?$/.test(normalized) || /확인 필요|미정|논의 필요/i.test(normalized)) {
        questions.push({ text: normalized, chunkId: chunk.id });
      }

      const isCheckbox = /^-\s*\[\s*\]\s+/i.test(line);
      const isBullet = /^(-|\*|\d+\.)\s+/.test(line);
      if (isCheckbox || (isBullet && isLikelyActionLine(normalized))) {
        todos.push({
          text: normalized.replace(/^-\s*\[\s*\]\s+/i, '').replace(/^(-|\*|\d+\.)\s+/, ''),
          chunkId: chunk.id,
        });
      }

      if (/필수|반드시|지원해야|요구사항|제약/i.test(normalized) || heading.includes('요구사항')) {
        requirements.push({ text: normalized, chunkId: chunk.id });
      }

      if (/화면|페이지|모달|컴포넌트|폼|버튼/i.test(normalized) || heading.includes('화면')) {
        screens.push({ text: normalized, chunkId: chunk.id });
      }
    }

    if (heading.includes('todo') || heading.includes('합의 todo')) {
      for (const bullet of bullets) {
        const text = normalizeLine(bullet);
        if (isLikelyActionLine(text) || /\bapi\b/i.test(text)) {
          todos.push({ text, chunkId: chunk.id });
        }
      }
    }
  }

  return {
    requirements: dedupe(requirements),
    screens: dedupe(screens),
    apis: dedupe(apis),
    todos: dedupe(todos),
    questions: dedupe(questions),
  };
}
