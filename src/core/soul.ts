import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

/**
 * 소울(페르소나) 코어 로직 — 순수/DI. IO 엔트리(`src/hooks/inject-soul.ts`)와
 * 슬래시 커맨드가 이 모듈을 소비한다.
 *
 * 소울 = markdown 파일. frontmatter(`name` / `description`) + 본문(페르소나 지침).
 * 프리셋은 번들 `souls/`, 커스텀은 `~/.config/rocky/souls/`. 같은 이름이면 커스텀이 이긴다.
 *
 * 소울은 AGENTS.md 의 게이트/안전 규칙 위에 얹히는 "플레이버 + 작업 스타일 레이어" 다.
 * `buildSoulContext` 가 그 우선순위 preamble 을 앞에 붙인다.
 */

/** 소울 디렉터리 쌍. 테스트/커맨드가 임의 경로를 주입할 수 있게 파라미터화. */
export interface SoulDirs {
  /** 번들 프리셋 디렉터리 (`<pluginRoot>/souls`). */
  presetDir: string;
  /** 커스텀 소울 디렉터리 (`~/.config/rocky/souls`). */
  customDir: string;
}

/** 목록 항목 — 본문 없이 메타만. */
export interface SoulSummary {
  name: string;
  description: string;
  source: 'preset' | 'custom';
  path: string;
}

/** 본문 포함 소울. */
export interface Soul extends SoulSummary {
  body: string;
}

/**
 * 최소 frontmatter 파서 (의존성 0). 문서가 `---\n...\n---\n` 로 시작하면 그 안에서
 * `name:` / `description:` 라인만 읽고, 나머지를 body 로 돌려준다. frontmatter 가 없으면
 * 입력 전체가 body.
 */
export function parseFrontmatter(raw: string): {
  name?: string;
  description?: string;
  body: string;
} {
  const normalized = raw.replace(/^﻿/, '');
  if (!normalized.startsWith('---')) {
    return { body: normalized };
  }
  // 첫 줄(---) 이후의 닫는 --- 를 찾는다.
  const lines = normalized.split('\n');
  let end = -1;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i]!.trim() === '---') {
      end = i;
      break;
    }
  }
  if (end === -1) {
    return { body: normalized };
  }
  const meta: { name?: string; description?: string } = {};
  for (let i = 1; i < end; i++) {
    const line = lines[i]!;
    const m = line.match(/^\s*(name|description)\s*:\s*(.*)$/);
    if (m) {
      const key = m[1] as 'name' | 'description';
      // 양끝 따옴표 제거 (선택).
      meta[key] = m[2]!.trim().replace(/^["']|["']$/g, '');
    }
  }
  const body = lines.slice(end + 1).join('\n');
  return { ...meta, body };
}

/**
 * 기본 소울 디렉터리. 프리셋은 이 파일 기준 `<pluginRoot>/souls` (src/core → ../../souls),
 * 커스텀은 `~/.config/rocky/souls`. `import.meta.dir` 사용 — `__dirname` 금지.
 */
export function resolveDefaultSoulDirs(): SoulDirs {
  return {
    presetDir: join(import.meta.dir, '..', '..', 'souls'),
    customDir: join(homedir(), '.config', 'rocky', 'souls'),
  };
}

/** config 의 활성 소울 이름. 미설정이면 undefined. */
export function resolveSoulName(config: { soul?: string }): string | undefined {
  const name = config.soul?.trim();
  return name && name.length > 0 ? name : undefined;
}

/** 한 디렉터리의 `*.md` 를 요약으로 읽는다. 없거나 못 읽으면 빈 배열. */
function listDir(dir: string, source: 'preset' | 'custom'): SoulSummary[] {
  if (!existsSync(dir)) {
    return [];
  }
  let names: string[];
  try {
    names = readdirSync(dir).filter((f) => f.endsWith('.md'));
  } catch {
    return [];
  }
  const out: SoulSummary[] = [];
  for (const file of names) {
    const path = join(dir, file);
    // 정체성 키는 항상 파일명 stem — frontmatter 의 name: 은 identity 로 쓰지 않는다
    // (readSoul 도 파일명으로 찾으므로, 여기서 어긋나면 listSouls 결과를 readSoul 로 못 찾는다).
    const stem = file.replace(/\.md$/, '');
    try {
      const parsed = parseFrontmatter(readFileSync(path, 'utf8'));
      out.push({
        name: stem,
        description: parsed.description?.trim() ?? '',
        source,
        path,
      });
    } catch {
      // 못 읽는 파일은 조용히 건너뛴다.
    }
  }
  return out;
}

/**
 * 프리셋 + 커스텀 소울 머지. 이름이 겹치면 커스텀이 프리셋을 덮어쓴다.
 * name 오름차순 정렬.
 */
export function listSouls(dirs: SoulDirs = resolveDefaultSoulDirs()): SoulSummary[] {
  const byName = new Map<string, SoulSummary>();
  for (const s of listDir(dirs.presetDir, 'preset')) {
    byName.set(s.name, s);
  }
  for (const s of listDir(dirs.customDir, 'custom')) {
    byName.set(s.name, s); // 커스텀이 프리셋을 덮어씀
  }
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/** 이름으로 한 소울을 읽는다 (커스텀 우선). 없으면 null. */
export function readSoul(name: string, dirs: SoulDirs = resolveDefaultSoulDirs()): Soul | null {
  for (const [dir, source] of [
    [dirs.customDir, 'custom'],
    [dirs.presetDir, 'preset'],
  ] as const) {
    const path = join(dir, `${name}.md`);
    if (!existsSync(path)) {
      continue;
    }
    try {
      const parsed = parseFrontmatter(readFileSync(path, 'utf8'));
      return {
        // 정체성 키는 파일명 인자 그 자체 — listDir 과 동일한 규칙으로 맞춘다.
        name,
        description: parsed.description?.trim() ?? '',
        body: parsed.body,
        source,
        path,
      };
    } catch {
      // 이 후보(예: 커스텀)를 못 읽으면 다음 후보(예: 프리셋)로 폴백한다 (루프 계속) —
      // listDir 의 "못 읽는 파일은 건너뛴다" 와 동일한 fail-soft 정책.
    }
  }
  return null;
}

/** `buildSoulContext` 옵션. */
export interface SoulContextOptions {
  /** 사용자를 부르는 호칭 (`rocky.json` 의 `callsign`). trim 후 비어있으면 무시. */
  callsign?: string;
}

/**
 * 소울 본문을 세션 주입용 컨텍스트로 감싼다. 앞에 우선순위 preamble 을 붙여, 페르소나가
 * AGENTS.md 의 게이트/안전 규칙과 충돌하면 항상 후자가 이기도록 명시한다.
 * `opts.callsign` 이 있으면 본문 뒤에 호칭 지시 한 줄을 덧붙인다 — 소울 본문의 기본
 * 호칭 규칙보다 우선한다.
 */
export function buildSoulContext(soul: Soul, opts: SoulContextOptions = {}): string {
  const lines = [
    `# rocky soul: ${soul.name}`,
    '',
    '아래는 이 세션에 선택된 rocky 소울(페르소나)이다. 말투/성격 + 작업 방식의 레이어일 뿐,',
    'AGENTS.md / CLAUDE.md 의 게이트·검증·안전 규칙을 절대 덮어쓰지 않는다 — 충돌 시 그 규칙이 이긴다.',
    '',
    soul.body.trim(),
  ];
  const callsign = opts.callsign?.trim();
  if (callsign) {
    // JSON.stringify 로 감싸 따옴표 등 특수문자가 있어도 주입 라인이 깨지지 않게 한다.
    lines.push(
      '',
      `사용자 호칭: 이 세션의 사용자를 ${JSON.stringify(callsign)}(이)라고 부른다 — 소울 본문의 기본 호칭 규칙보다 우선한다.`,
    );
  }
  return lines.join('\n');
}
