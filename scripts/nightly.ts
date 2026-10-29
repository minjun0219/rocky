#!/usr/bin/env bun
/**
 * nightly 프리릴리스 — CI(`nightly.yml`)가 매일 밤 main 을 `v<version>-nightly.<YYYYMMDD>` 로 올린다.
 * 설계는 `docs/design/specs/2026-10-29-nightly-channel-design.md`.
 *
 * 순수 판정(`nightlyTag` · `planNightly` · `prunableTags` · `parseLsRemoteTags`)과 그 위의 얇은 CLI:
 *
 *   bun scripts/nightly.ts plan      빌드할지·태그·버전을 GITHUB_OUTPUT 에(build/tag/version/reason)
 *   bun scripts/nightly.ts stamp V   Cargo.toml·Cargo.lock 의 워크스페이스 버전을 V 로(커밋하지 않는다)
 *   bun scripts/nightly.ts prune     최근 KEEP 개만 남기고 오랜 nightly 프리릴리스·태그를 지운다
 *
 * 지난 nightly 는 `git ls-remote --tags` 로 찾는다(GitHub API 예산을 쓰지 않는다). CI 결과만 `gh api` 로 본다.
 */
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { syncCargoLock, syncCargoToml } from './sync-version';

/** 남길 nightly 개수(설계 4번). */
export const KEEP = 7;

const NIGHTLY = /^v(\d+\.\d+\.\d+)-nightly\.(\d{8})(?:\.(\d+))?$/;

export type NightlyTag = { tag: string; version: string; date: string; seq: number };

/** `v0.54.0-nightly.20261029` · `v0.54.0-nightly.20261029.2` 를 읽는다. 정식·`-next` 태그면 null. */
export function parseNightlyTag(tag: string): NightlyTag | null {
  const m = NIGHTLY.exec(tag.trim());
  const [, version, date, seq] = m ?? [];
  if (!version || !date) {
    return null;
  }
  return { tag: tag.trim(), version, date, seq: seq ? Number(seq) : 1 };
}

/** 오랜 것이 앞 — 날짜, 같은 날이면 순번. */
function compareNightly(a: NightlyTag, b: NightlyTag): number {
  return a.date === b.date ? a.seq - b.seq : a.date < b.date ? -1 : 1;
}

/** UTC 기준이 아니라 KST 날짜 — 01:30 KST(16:30 UTC 전날)에 도는 빌드가 그날 날짜를 달게. */
export function kstDate(now: Date): string {
  const kst = new Date(now.getTime() + 9 * 60 * 60 * 1000);
  return kst.toISOString().slice(0, 10).replaceAll('-', '');
}

/**
 * 오늘 붙일 태그. 같은 날 이미 있으면 `.2`, `.3` … (수동 실행으로 하루 두 번 낸 경우).
 * @throws 버전이 `x.y.z` 가 아니면 — 프리릴리스 버전(`-next.N`) 위에 nightly 꼬리를 또 달지 않는다
 */
export function nightlyTag(version: string, date: string, existing: readonly string[]): string {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`nightly 는 x.y.z 버전 위에만 단다: ${version}`);
  }
  const base = `v${version}-nightly.${date}`;
  const sameDay = existing
    .map(parseNightlyTag)
    .filter((t): t is NightlyTag => t !== null && t.version === version && t.date === date);
  if (sameDay.length === 0) {
    return base;
  }
  return `${base}.${Math.max(...sameDay.map((t) => t.seq)) + 1}`;
}

/** `git ls-remote --tags` 출력 → 태그 → 커밋. annotated 태그는 `^{}` 줄의 커밋을 쓴다. */
export function parseLsRemoteTags(out: string): Map<string, string> {
  const tags = new Map<string, string>();
  for (const line of out.split('\n')) {
    const [sha, ref] = line.trim().split(/\s+/);
    if (!sha || !ref?.startsWith('refs/tags/')) {
      continue;
    }
    const name = ref.slice('refs/tags/'.length);
    if (name.endsWith('^{}')) {
      tags.set(name.slice(0, -3), sha);
    } else if (!tags.has(name)) {
      tags.set(name, sha);
    }
  }
  return tags;
}

/** 가장 최근 nightly(태그와 커밋). 없으면 null. */
export function latestNightly(tags: Map<string, string>): { tag: string; sha: string } | null {
  const nightlies = [...tags.keys()]
    .map(parseNightlyTag)
    .filter((t): t is NightlyTag => t !== null)
    .sort(compareNightly);
  const last = nightlies.at(-1);
  return last ? { tag: last.tag, sha: tags.get(last.tag) ?? '' } : null;
}

/** `ci.yml` 의 그 커밋 push 실행 — 없으면 null. */
export type CiRun = { status: string; conclusion: string | null } | null;

export type NightlyPlan =
  | { build: true; tag: string; version: string; reason: string }
  | { build: false; reason: string };

/**
 * 오늘 밤 빌드할까(설계 2번) — main HEAD 가 지난 nightly 와 다르고 그 커밋의 CI 가 초록일 때만. 수동 실행(`force`)은
 * 커밋이 같아도 낸다(산출물을 다시 만들 때). CI 는 force 여도 본다 — 깨진 main 을 기기에 깔지 않는다.
 */
export function planNightly(input: {
  head: string;
  version: string;
  date: string;
  tags: Map<string, string>;
  ci: CiRun;
  force?: boolean;
}): NightlyPlan {
  if (!/^\d+\.\d+\.\d+$/.test(input.version)) {
    return { build: false, reason: `버전 ${input.version} 이 프리릴리스라 nightly 를 달지 않는다` };
  }
  const last = latestNightly(input.tags);
  if (!input.force && last && last.sha === input.head) {
    return { build: false, reason: `main ${input.head.slice(0, 7)} 은 이미 ${last.tag} 다` };
  }
  if (!input.ci) {
    return { build: false, reason: `main ${input.head.slice(0, 7)} 의 CI 실행을 찾지 못했다` };
  }
  if (input.ci.status !== 'completed') {
    return {
      build: false,
      reason: `main ${input.head.slice(0, 7)} 의 CI 가 아직 돈다(${input.ci.status})`,
    };
  }
  if (input.ci.conclusion !== 'success') {
    return {
      build: false,
      reason: `main ${input.head.slice(0, 7)} 의 CI 가 ${input.ci.conclusion} 이다`,
    };
  }
  const tag = nightlyTag(input.version, input.date, [...input.tags.keys()]);
  return {
    build: true,
    tag,
    version: tag.slice(1),
    reason: `main ${input.head.slice(0, 7)} → ${tag}`,
  };
}

/** 지울 nightly 태그 — 최근 `keep` 개를 뺀 나머지(오랜 것부터). 정식·`-next` 태그는 고르지 않는다. */
export function prunableTags(tags: readonly string[], keep = KEEP): string[] {
  const nightlies = tags
    .map(parseNightlyTag)
    .filter((t): t is NightlyTag => t !== null)
    .sort(compareNightly);
  return nightlies.slice(0, Math.max(0, nightlies.length - keep)).map((t) => t.tag);
}

function run(argv: string[]): string {
  const out = Bun.spawnSync(argv, { stderr: 'inherit' });
  if (!out.success) {
    throw new Error(`${argv.join(' ')} 실패(종료 코드 ${out.exitCode})`);
  }
  return out.stdout.toString();
}

function setOutput(name: string, value: string): void {
  const file = process.env.GITHUB_OUTPUT;
  if (file) {
    appendFileSync(file, `${name}=${value}\n`);
  }
  console.log(`${name}=${value}`);
}

function remoteTags(): Map<string, string> {
  return parseLsRemoteTags(run(['git', 'ls-remote', '--tags', 'origin', 'v*-nightly.*']));
}

function ciRun(sha: string): CiRun {
  const json = run([
    'gh',
    'api',
    `repos/{owner}/{repo}/actions/workflows/ci.yml/runs?head_sha=${sha}&event=push&per_page=1`,
  ]);
  const runs = (JSON.parse(json) as { workflow_runs?: CiRun[] }).workflow_runs ?? [];
  return runs[0] ?? null;
}

const repoRoot = join(import.meta.dir, '..');

if (import.meta.main) {
  const [cmd, arg] = process.argv.slice(2);
  if (cmd === 'plan') {
    const head = run(['git', 'rev-parse', 'HEAD']).trim();
    const version = (
      JSON.parse(readFileSync(join(repoRoot, 'package.json'), 'utf8')) as { version: string }
    ).version;
    const plan = planNightly({
      head,
      version,
      date: kstDate(new Date()),
      tags: remoteTags(),
      ci: ciRun(head),
      force: process.env.NIGHTLY_FORCE === 'true',
    });
    setOutput('build', String(plan.build));
    setOutput('reason', plan.reason);
    if (plan.build) {
      setOutput('tag', plan.tag);
      setOutput('version', plan.version);
    }
  } else if (cmd === 'stamp' && arg) {
    for (const [file, sync] of [
      ['Cargo.toml', syncCargoToml],
      ['Cargo.lock', syncCargoLock],
    ] as const) {
      const path = join(repoRoot, file);
      writeFileSync(path, sync(readFileSync(path, 'utf8'), arg));
      console.log(`${file} version → ${arg}`);
    }
  } else if (cmd === 'prune') {
    // 하나를 못 지워도(릴리스 없이 태그만 남음 등) 나머지는 지운다 — 다음 밤에 다시 본다.
    for (const tag of prunableTags([...remoteTags().keys()])) {
      const out = Bun.spawnSync(['gh', 'release', 'delete', tag, '--cleanup-tag', '--yes'], {
        stdout: 'inherit',
        stderr: 'inherit',
      });
      console.log(out.success ? `지움: ${tag}` : `못 지움: ${tag}(종료 코드 ${out.exitCode})`);
    }
  } else {
    console.error('쓰는 법: bun scripts/nightly.ts plan | stamp <version> | prune');
    process.exit(2);
  }
}
