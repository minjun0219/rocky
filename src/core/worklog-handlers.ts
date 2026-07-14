import type {
  Worklog,
  WorklogAppendInput,
  WorklogEntry,
  WorklogReadOptions,
  WorklogSearchOptions,
  WorklogStatus,
} from './worklog';

/**
 * worklog_* 도구 핸들러 — 진입점(`src/index.ts`)은 등록만, 실제 동작은 여기에 위임한다
 * (notion / openapi 와 동일 패턴). 모두 로컬 파일시스템만 만지고 remote 호출은 없다.
 */

/** 도구 핸들러: 저널에 한 줄 append. */
export function handleWorklogAppend(
  worklog: Worklog,
  input: WorklogAppendInput,
): Promise<WorklogEntry> {
  return worklog.append(input);
}

/** 도구 핸들러: 가장 최근 항목부터 필터 / limit 적용해 반환. */
export function handleWorklogRead(
  worklog: Worklog,
  options: WorklogReadOptions = {},
): Promise<WorklogEntry[]> {
  return worklog.read(options);
}

/** 도구 핸들러: substring (case-insensitive) 검색. */
export function handleWorklogSearch(
  worklog: Worklog,
  query: string,
  options: WorklogSearchOptions = {},
): Promise<WorklogEntry[]> {
  return worklog.search(query, options);
}

/** 도구 핸들러: 저널 메타 + wikiDir + 마지막 curate watermark. */
export function handleWorklogStatus(worklog: Worklog): Promise<WorklogStatus> {
  return worklog.status();
}
