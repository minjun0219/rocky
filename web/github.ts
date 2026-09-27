/** GitHub 링크 판별 — 이슈 생성 버튼의 중복 가드와 표시가 같은 판별을 쓴다. 정본은 Rust `rocky_core::github`. */

/**
 * links 중 GitHub **이슈** URL 을 찾는다. PR URL(`/pull/<n>`)은 이슈가 아니다.
 */
export function findIssueLink(links: readonly { url: string }[]): string | undefined {
  return links.find((link) =>
    /^https:\/\/github\.com\/[^/]+\/[^/]+\/issues\/\d+(?:[/?#]|$)/.test(link.url),
  )?.url;
}

/** 이슈 URL 끝의 번호. 링크 제목(`#12`)을 만드는 데 쓴다. */
export function issueNumberFrom(url: string): number | undefined {
  const match = /\/issues\/(\d+)/.exec(url.trim());
  return match?.[1] ? Number(match[1]) : undefined;
}
