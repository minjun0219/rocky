import type {
  AgentJournal,
  JournalAppendInput,
  JournalEntry,
  JournalReadOptions,
  JournalSearchOptions,
  JournalStatus,
} from './journal';

/**
 * journal_* 도구 핸들러 — 진입점(`src/index.ts`)은 등록만, 실제 동작은 여기에 위임한다
 * (notion / openapi 와 동일 패턴). 모두 로컬 파일시스템만 만지고 remote 호출은 없다.
 */

/** 도구 핸들러: 저널에 한 줄 append. */
export function handleJournalAppend(
  journal: AgentJournal,
  input: JournalAppendInput,
): Promise<JournalEntry> {
  return journal.append(input);
}

/** 도구 핸들러: 가장 최근 항목부터 필터 / limit 적용해 반환. */
export function handleJournalRead(
  journal: AgentJournal,
  options: JournalReadOptions = {},
): Promise<JournalEntry[]> {
  return journal.read(options);
}

/** 도구 핸들러: substring (case-insensitive) 검색. */
export function handleJournalSearch(
  journal: AgentJournal,
  query: string,
  options: JournalSearchOptions = {},
): Promise<JournalEntry[]> {
  return journal.search(query, options);
}

/** 도구 핸들러: 저널 메타 + wikiDir + 마지막 curate watermark. */
export function handleJournalStatus(journal: AgentJournal): Promise<JournalStatus> {
  return journal.status();
}
