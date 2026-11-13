// 데몬 REST 응답 — Rust `rocky_core::types`·`prwatch` 의 JSON 모양(camelCase) 중 맥 앱이 쓰는 필드만.
// 정본은 Rust 쪽이고(`docs/rewrite/contract.md`), 웹의 `web/types.ts` 와 같은 사본이다. 모르는 필드는 버린다.
import Foundation

/// 데몬 PR 감시의 스냅숏 — `rocky_core::prwatch::PrSnapshot` 중 메뉴에 쓰는 것.
public struct PrSnapshot: Codable, Identifiable, Hashable, Sendable {
    public var repo: String
    public var number: Int
    public var title: String
    public var url: String
    /// `OPEN` · `MERGED` · `CLOSED`.
    public var state: String
    public var isDraft: Bool
    /// `pass` · `fail` · `pending`.
    public var ci: String
    /// 데몬이 판정한 "머지 후보" — CI 통과·충돌 없음·처리 안 된 스레드 없음 등. 판정은 데몬 몫이다.
    public var ready: Bool

    public var id: String { "\(repo)#\(number)" }

    public init(repo: String, number: Int, title: String, url: String, state: String = "OPEN", ci: String = "pass", ready: Bool) {
        self.repo = repo
        self.number = number
        self.title = title
        self.url = url
        self.state = state
        self.isDraft = false
        self.ci = ci
        self.ready = ready
    }
}

/// SSE `/api/events` 의 한 프레임 — `rocky_core::types::ChangeEvent`.
public struct ChangeEvent: Decodable, Sendable {
    public var entity: String
    public var action: String
}
