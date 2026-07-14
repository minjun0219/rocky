/**
 * Claude Code 트랜스크립트(JSONL) 에서 "마지막 한 턴" 을 기계적으로 추출한다.
 * LLM 없이 동작 — Stop hook 이 워크로그 한 줄을 만들 재료(req/tools/did)만 뽑는다.
 */

export interface TurnParts {
  req: string;
  tools: string[];
  did: string;
}

interface RawBlock {
  type?: string;
  text?: string;
  name?: string;
}
interface RawMessage {
  role?: string;
  content?: string | RawBlock[];
}
interface RawEntry {
  message?: RawMessage;
}

function textOf(content: string | RawBlock[] | undefined): string {
  if (typeof content === 'string') {
    return content.trim();
  }
  if (!Array.isArray(content)) {
    return '';
  }
  return content
    .filter((b): b is RawBlock => !!b && b.type === 'text' && typeof b.text === 'string')
    .map((b) => b.text as string)
    .join('\n')
    .trim();
}

function isRealUserPrompt(msg: RawMessage): boolean {
  if (msg.role !== 'user') {
    return false;
  }
  if (typeof msg.content === 'string') {
    return msg.content.trim().length > 0;
  }
  if (!Array.isArray(msg.content)) {
    return false;
  }
  return msg.content.some(
    (b) => !!b && b.type === 'text' && typeof b.text === 'string' && b.text.trim().length > 0,
  );
}

export function extractTurn(transcriptText: string): TurnParts | null {
  const entries: RawEntry[] = [];
  for (const line of transcriptText.split('\n')) {
    const t = line.trim();
    if (!t) {
      continue;
    }
    try {
      const parsed = JSON.parse(t);
      if (parsed && typeof parsed === 'object') {
        entries.push(parsed as RawEntry);
      }
    } catch {
      // 손상/부분 라인 skip
    }
  }
  let startIdx = -1;
  for (let i = entries.length - 1; i >= 0; i--) {
    const msg = entries[i]?.message;
    if (msg && isRealUserPrompt(msg)) {
      startIdx = i;
      break;
    }
  }
  if (startIdx < 0) {
    return null;
  }
  const req = textOf(entries[startIdx]?.message?.content);
  const toolCounts = new Map<string, number>();
  let did = '';
  for (let i = startIdx + 1; i < entries.length; i++) {
    const msg = entries[i]?.message;
    if (msg?.role !== 'assistant') {
      continue;
    }
    if (Array.isArray(msg.content)) {
      for (const b of msg.content) {
        if (b && b.type === 'tool_use' && typeof b.name === 'string') {
          toolCounts.set(b.name, (toolCounts.get(b.name) ?? 0) + 1);
        }
      }
    }
    const txt = textOf(msg.content);
    if (txt) {
      did = txt;
    }
  }
  const tools = [...toolCounts.entries()].map(([name, n]) => (n > 1 ? `${name}(×${n})` : name));
  if (!req && !did && tools.length === 0) {
    return null;
  }
  return { req, tools, did };
}

export function buildTurnContent(parts: TurnParts, maxChars: number): string {
  const clip = (s: string): string => {
    const one = s.replace(/\s+/g, ' ').trim();
    return one.length > maxChars ? `${one.slice(0, maxChars)}…` : one;
  };
  const req = clip(parts.req) || '(none)';
  const tools = parts.tools.slice(0, 20).join(', ') || '(none)';
  const did = clip(parts.did) || '(none)';
  return `req: ${req} | tools: ${tools} | did: ${did}`;
}
