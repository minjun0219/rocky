#!/usr/bin/env bun
/**
 * changesets Version PR 이 병합돼 main 의 package.json 버전이 오르면, 그 버전으로
 * `v<version>` git 태그 + GitHub Release 를 생성한다 (릴리스 노트는 CHANGELOG 해당 섹션).
 * npm publish 는 하지 않는다 — 태그 + GitHub Release 만.
 *
 * release.yml 의 스텝에서 매 main push 마다 실행되므로 멱등이어야 한다:
 * 이미 `v<version>` 태그가 있으면 아무 것도 하지 않는다.
 *
 * 전제: GitHub Actions 러너 (git push 자격증명 persist + gh CLI + GH_TOKEN/GITHUB_TOKEN).
 */
import { $ } from 'bun';
import { readFileSync } from 'node:fs';
import { extractChangelogSection } from './changelog';

const version = (JSON.parse(readFileSync('package.json', 'utf8')) as { version?: string }).version;
if (!version) {
  throw new Error('package.json 에 version 이 없다');
}
const tag = `v${version}`;

const existing = (await $`git tag -l ${tag}`.text()).trim();
if (existing === tag) {
  console.log(`${tag} 이미 존재 — release skip (멱등)`);
  process.exit(0);
}

let changelog = '';
try {
  changelog = readFileSync('CHANGELOG.md', 'utf8');
} catch {
  // CHANGELOG 가 없으면 노트는 태그명으로 대체
}
const notes = extractChangelogSection(changelog, version) || tag;

await $`git tag -a ${tag} -m ${`rocky ${tag}`}`;
await $`git push origin ${tag}`;
await $`gh release create ${tag} --title ${tag} --notes ${notes}`;
console.log(`released ${tag}`);
