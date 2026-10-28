// 에이전트 세션 상태 — 데몬 `GET /api/sessions` 를 주기적으로 묻는다. 세션 목록은 SSE 로 오지 않고, 데몬이 몇 초 캐시한
// `claude agents --json` 이라 웹 에이전트 탭과 같은 주기로만 묻는다: 아바타가 보이면 15초, 아니면 60초.
import Foundation
import Observation
import RockyKit

@MainActor
@Observable
final class AgentStore {
    /// 아바타가 보이는 동안의 주기 — 웹 에이전트 탭을 보고 있을 때(`POLL_MS`)와 같다.
    static let watchingInterval: Double = 15
    /// 아바타를 끈 동안의 주기 — 메뉴 칸만 채운다(웹 `IDLE_POLL_MS`).
    static let idleInterval: Double = 60

    private(set) var result: SessionsResult?
    private(set) var error: String?
    /// 떠 있는 아바타를 보이나 — 다음 실행에도 이어진다. 기본은 켜짐.
    var showAvatar: Bool {
        didSet {
            UserDefaults.standard.set(showAvatar, forKey: "avatar")
            // 켜면 바로 새로 — 60초 주기 사이에 켠 아바타가 낡은 기분으로 뜨지 않게.
            if showAvatar { refresh() }
        }
    }

    private let client: DaemonClient
    private var poll: Task<Void, Never>?
    private var wake: Task<Void, Never>?

    init(client: DaemonClient) {
        self.client = client
        showAvatar = UserDefaults.standard.object(forKey: "avatar") as? Bool ?? true
    }

    var mood: AvatarMood { avatarMood(error == nil ? result : nil) }
    var rows: [Session] { agentRows(result?.sessions ?? []) }
    var agentsURL: URL? { URL(string: client.base + "/?view=agents") }

    func start() {
        guard poll == nil else { return }
        poll = Task {
            while !Task.isCancelled {
                await load()
                let interval = showAvatar ? Self.watchingInterval : Self.idleInterval
                try? await Task.sleep(for: .seconds(interval))
            }
        }
    }

    /// 주기를 기다리지 않고 한 번 묻는다 — 메뉴를 열 때, 아바타를 켤 때.
    func refresh() {
        wake?.cancel()
        wake = Task { await load() }
    }

    private func load() async {
        do {
            result = try await client.sessions()
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}
