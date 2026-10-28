// 에이전트 세션 — 데몬 `GET /api/sessions`(출처는 `claude agents --json`)의 사본과 아바타·메뉴가 쓰는 순수 판정.
// 상태 묶음은 웹 `web/agents.ts` 의 `agentPhase` 와 같은 규칙이다 — 한쪽을 바꾸면 같이 고친다.
import Foundation

/// background 세션의 작업 요약 — Claude 가 남긴 한 줄(`rocky_core::sessions::JobSummary`). 못 읽으면 통째로 없다.
public struct JobSummary: Codable, Hashable, Sendable {
    /// 지금 하는 일 또는 멈춘 자리 한 줄.
    public var detail: String?
    /// 사람에게 필요한 것 — `blocked` 일 때 무엇을 기다리는지.
    public var needs: String?

    public init(detail: String? = nil, needs: String? = nil) {
        self.detail = detail
        self.needs = needs
    }
}

public struct Session: Codable, Identifiable, Hashable, Sendable {
    public var sessionId: String
    public var cwd: String
    /// `interactive` · `background` — CLI 값 그대로.
    public var kind: String
    /// 사람이 읽는 이름(`eelpout-a3`).
    public var name: String
    /// `idle` · `busy` — pid 없이 잠든 background 행에는 없다.
    public var status: String?
    /// background 의 수명 상태 — `working` · `blocked`(사람 답을 기다림) · `done`. interactive 에는 없다.
    public var state: String?
    public var job: JobSummary?

    public var id: String { sessionId }

    public init(
        sessionId: String,
        name: String,
        cwd: String = "/tmp/repo",
        kind: String = "interactive",
        status: String? = "idle",
        state: String? = nil,
        job: JobSummary? = nil
    ) {
        self.sessionId = sessionId
        self.cwd = cwd
        self.kind = kind
        self.name = name
        self.status = status
        self.state = state
        self.job = job
    }
}

public struct SessionsResult: Codable, Sendable {
    /// 세션 목록을 얻을 수 있었나 — false 면 `reason` 을 그대로 보인다.
    public var available: Bool
    public var reason: String?
    public var sessions: [Session]

    public init(available: Bool, reason: String? = nil, sessions: [Session]) {
        self.available = available
        self.reason = reason
        self.sessions = sessions
    }
}

/// 세션의 묶음 — 웹 에이전트 탭과 같다. background `blocked` 는 내 차례, `busy`·background `working` 은 실행 중,
/// 나머지는 쉬는 중. interactive 의 `idle` 은 사람이 칠 차례지만 터미널(cmux)이 이미 알리므로 내 차례로 치지 않는다.
public enum AgentPhase: Int, Comparable, Sendable {
    case blocked, run, idle, done

    public static func < (a: AgentPhase, b: AgentPhase) -> Bool { a.rawValue < b.rawValue }

    public var label: String {
        switch self {
        case .blocked: "내 차례"
        case .run: "실행 중"
        case .idle: "쉬는 중"
        case .done: "끝남"
        }
    }
}

public func agentPhase(_ session: Session) -> AgentPhase {
    if session.state == "blocked" { return .blocked }
    if session.state == "done" { return .done }
    return session.status == "busy" || session.state == "working" ? .run : .idle
}

/// 어디서 도나 — 레포 폴더 이름(워크트리는 레포로 접는다). 웹 `repoLabel` 과 같다.
public func repoLabel(_ cwd: String) -> String {
    let root = cwd.components(separatedBy: "/.claude/worktrees/").first ?? cwd
    return root.split(separator: "/").last.map(String.init) ?? cwd
}

/// 세션 줄의 둘째 줄 — 내 차례면 기다리는 것(`needs`, 없으면 `detail`), 아니면 하는 일(`detail`).
public func agentLine(_ session: Session) -> String? {
    let line = agentPhase(session) == .blocked ? session.job?.needs ?? session.job?.detail : session.job?.detail
    return line.flatMap { $0.isEmpty ? nil : $0 }
}

/// 메뉴에 띄울 세션 — 끝난 것을 빼고 내 차례 → 실행 중 → 쉬는 중, 같은 묶음은 이름 순.
public func agentRows(_ sessions: [Session]) -> [Session] {
    sessions
        .filter { agentPhase($0) != .done }
        .sorted { (agentPhase($0), $0.name) < (agentPhase($1), $1.name) }
}

/// 아바타의 기분 — 가장 급한 것 하나로 정한다.
public enum AvatarMood: Equatable, Sendable {
    /// 데몬에 못 붙었거나 세션 목록을 못 얻었다.
    case unknown
    /// 사람 답을 기다리는 세션이 있다 — 첫 세션과 그 한 줄, 그리고 모두 몇 개인지.
    case waiting(name: String, line: String?, count: Int)
    /// 도는 세션이 있다.
    case working(count: Int)
    /// 다 쉬고 있다(세션이 없을 때도).
    case resting
}

public func avatarMood(_ result: SessionsResult?) -> AvatarMood {
    guard let result, result.available else { return .unknown }
    let rows = agentRows(result.sessions)
    let waiting = rows.filter { agentPhase($0) == .blocked }
    if let first = waiting.first {
        return .waiting(name: first.name, line: agentLine(first), count: waiting.count)
    }
    let running = rows.filter { agentPhase($0) == .run }.count
    return running > 0 ? .working(count: running) : .resting
}
