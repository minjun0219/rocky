// 아바타의 혼잣말 — 세션 목록이 바뀔 때(일을 시작했다 · 마쳤다)와 일하는 동안 가끔 말풍선으로 상태를 알린다.
// 내 차례(background `blocked`)는 여기서 말하지 않는다 — 그 말풍선은 `AvatarMood.waiting` 이 늘 띄운다.
// 묶음 규칙은 `agentPhase`(웹 `web/agents.ts` 의 사본)를 그대로 쓴다.
import Foundation

public struct Chatter: Equatable, Sendable {
    /// 일하는 동안 이만큼마다 한 번 지금 하는 일을 말한다.
    public static let nudge: Double = 240

    /// 지난번에 본 세션별 묶음 — 처음 한 번은 보기만 하고 말하지 않는다(앱을 켤 때 "다 시작했어요"가 쏟아지지 않게).
    var known: [String: AgentPhase]?
    var lastSaid: Double = 0

    public init() {}

    /// 새 세션 목록을 보고 할 말 — 없으면 nil. `sessions` 가 nil 이면(못 읽음) 아무것도 바꾸지 않는다.
    public mutating func update(_ sessions: [Session]?, now: Double) -> String? {
        guard let sessions else { return nil }
        let phases = Dictionary(sessions.map { ($0.sessionId, agentPhase($0)) }, uniquingKeysWith: { a, _ in a })
        defer { known = phases }
        guard let known else {
            lastSaid = now
            return nil
        }
        let byName = sessions.sorted { $0.name < $1.name }
        // 돌던 세션이 쉬거나 끝났다(목록에서 사라진 것은 세지 않는다 — 이름을 모른다).
        let finished = byName.filter {
            known[$0.sessionId] == .run && (phases[$0.sessionId] == .idle || phases[$0.sessionId] == .done)
        }
        // 새로 돌기 시작했다 — 처음 보는 세션이거나 쉬던 세션.
        let started = byName.filter { phases[$0.sessionId] == .run && known[$0.sessionId] != .run }
        if let first = finished.first {
            lastSaid = now
            return "\(first.name)\(Self.more(finished.count)) 일을 마쳤어요"
        }
        if let first = started.first {
            lastSaid = now
            return "\(first.name)\(Self.more(started.count)) 일을 시작했어요"
        }
        let running = agentRows(sessions).filter { agentPhase($0) == .run }
        guard let first = running.first, now - lastSaid >= Self.nudge else { return nil }
        lastSaid = now
        let line = agentLine(first).map { " — \($0)" } ?? " 일하는 중이에요"
        return "\(first.name)\(Self.more(running.count))\(line)"
    }

    private static func more(_ count: Int) -> String {
        count > 1 ? " 외 \(count - 1)개" : ""
    }
}
