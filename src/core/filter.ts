import type { HttpMethod, IndexedEndpoint } from "./indexer";

/**
 * `openapi_search` / `list_endpoints` 에서 쓰는 필터 + 점수화 검색.
 *
 * keyword 점수: operationId(4) > path(3) > summary(2) > description(1).
 * keyword 가 없으면 모든 endpoint 가 점수 0 으로 통과 — 입력 순서를 유지한다.
 */

export interface EndpointFilter {
  spec?: string;
  tag?: string;
  method?: HttpMethod;
  keyword?: string;
}

interface ScoredEndpoint {
  endpoint: IndexedEndpoint;
  score: number;
}

const SCORE_OPERATION_ID = 4;
const SCORE_PATH = 3;
const SCORE_SUMMARY = 2;
const SCORE_DESCRIPTION = 1;

export function filterEndpoints(
  endpoints: IndexedEndpoint[],
  filter: EndpointFilter,
): IndexedEndpoint[] {
  const keyword = filter.keyword?.trim().toLowerCase();
  const scored: ScoredEndpoint[] = [];

  for (const ep of endpoints) {
    if (filter.spec && ep.specName !== filter.spec) continue;
    if (filter.tag && !ep.tags.includes(filter.tag)) continue;
    if (filter.method && ep.method !== filter.method) continue;

    if (!keyword) {
      scored.push({ endpoint: ep, score: 0 });
      continue;
    }
    const score = scoreKeyword(ep, keyword);
    if (score > 0) scored.push({ endpoint: ep, score });
  }

  if (keyword) {
    scored.sort((a, b) => b.score - a.score);
  }
  return scored.map((s) => s.endpoint);
}

function scoreKeyword(ep: IndexedEndpoint, keyword: string): number {
  let score = 0;
  if (ep.operationId?.toLowerCase().includes(keyword))
    score += SCORE_OPERATION_ID;
  if (ep.path.toLowerCase().includes(keyword)) score += SCORE_PATH;
  if (ep.summary?.toLowerCase().includes(keyword)) score += SCORE_SUMMARY;
  if (ep.description?.toLowerCase().includes(keyword))
    score += SCORE_DESCRIPTION;
  return score;
}
