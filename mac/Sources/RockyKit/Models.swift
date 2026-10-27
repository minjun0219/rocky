// 데몬 REST 응답 — Rust `rocky_core::types`·`prwatch` 의 JSON 모양(camelCase) 중 맥 앱이 쓰는 필드만.
// 정본은 Rust 쪽이고(`docs/rewrite/contract.md`), 웹의 `web/types.ts` 와 같은 사본이다. 모르는 필드는 버린다.
import Foundation

public struct Board: Codable, Identifiable, Hashable, Sendable {
    public var id: String
    public var key: String
    public var title: String
    public var description: String?
    /// `owner/name` — PR 감시 대상.
    public var repo: String?
    public var archivedAt: String?

    public init(id: String, key: String, title: String, repo: String? = nil) {
        self.id = id
        self.key = key
        self.title = title
        self.repo = repo
    }
}

public struct Section: Codable, Identifiable, Hashable, Sendable {
    public var id: String
    public var boardId: String
    public var title: String
    public var position: Int

    public init(id: String, boardId: String, title: String, position: Int = 0) {
        self.id = id
        self.boardId = boardId
        self.title = title
        self.position = position
    }
}

public enum TodoStatus: String, Codable, Sendable {
    case todo, doing, done
}

/// `POST /api/todos/:ref/status` 의 `{ action }` — 맥 앱이 보내는 것만.
public enum StatusAction: String, Sendable {
    case start, stop, done, reopen

    /// 상태 줄에 띄울 말 — TUI 의 `ACTION_LABEL` 과 같다.
    public var label: String {
        switch self {
        case .start: "착수"
        case .stop: "멈춤"
        case .done: "완료"
        case .reopen: "다시 열기"
        }
    }

    /// 그 상태에서 누를 수 있는 동작 — 웹·TUI 의 버튼과 같은 짝.
    public static func available(for status: TodoStatus) -> [StatusAction] {
        switch status {
        case .todo: [.start, .done]
        case .doing: [.stop, .done]
        case .done: [.reopen]
        }
    }
}

/// 응답 전용 todo(`TodoView`) — 저장 모델에 참조(`rocky-12`)와 댓글 수를 얹은 것.
public struct Todo: Codable, Identifiable, Hashable, Sendable {
    public var id: String
    public var number: Int
    public var boardId: String
    public var sectionId: String?
    public var parentId: String?
    public var title: String
    public var status: TodoStatus
    public var priority: String
    public var doingBy: String?
    public var doingSince: String?
    /// `live` · `idle` · `gone` · `unknown` — doing 이고 서버가 세션을 조회했을 때만 온다. 없으면 판정하지 않은 것.
    public var doingState: String?
    public var position: Int
    public var ref: String
    public var commentCount: Int

    public init(
        id: String,
        number: Int,
        boardId: String,
        title: String,
        status: TodoStatus = .todo,
        sectionId: String? = nil,
        parentId: String? = nil,
        doingBy: String? = nil,
        ref: String? = nil
    ) {
        self.id = id
        self.number = number
        self.boardId = boardId
        self.sectionId = sectionId
        self.parentId = parentId
        self.title = title
        self.status = status
        self.priority = "p4"
        self.doingBy = doingBy
        self.position = 0
        self.ref = ref ?? "board-\(number)"
        self.commentCount = 0
    }
}

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
