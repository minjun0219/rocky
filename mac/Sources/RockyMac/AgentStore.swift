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
    /// 아바타의 기분 — 세션 목록을 받을 때마다 정한다. 다 쉬어도 방금까지 일했으면 5분은 깨어 있다(`Drowsiness`).
    private(set) var mood: AvatarMood = .unknown
    private var drowsiness = Drowsiness()
    /// 떠 있는 아바타를 보이나 — 다음 실행에도 이어진다. 기본은 켜짐.
    var showAvatar: Bool {
        didSet {
            UserDefaults.standard.set(showAvatar, forKey: "avatar")
            // 켜면 바로 새로 — 60초 주기 사이에 켠 아바타가 낡은 기분으로 뜨지 않게.
            if showAvatar { refresh() }
        }
    }

    /// 아바타가 창 위를 돌아다니나 — 끄면 그 자리에 머문다. 기본은 켜짐.
    var roam: Bool {
        didSet { UserDefaults.standard.set(roam, forKey: "roam") }
    }

    /// 돌아다니는 동안 사람의 눈길(포인터 · 앞 창 · 입력)을 따라가나 — 끄면 화면을 제멋대로 돈다. 기본은 켜짐.
    var follow: Bool {
        didSet { UserDefaults.standard.set(follow, forKey: "follow") }
    }

    /// 아바타 모양 — 원작 · 미니멀 벡터 · 돌멩이 · 모두 나란히(비교용). 기본은 원작.
    var avatarStyle: AvatarStyle {
        didSet { UserDefaults.standard.set(avatarStyle.rawValue, forKey: AvatarStyle.key) }
    }

    /// 아바타가 할 혼잣말(`Chatter`) — 세션 목록을 새로 받을 때 정하고, 같은 글이 또 나와도 알아보게 번호를 붙인다.
    private(set) var chat: AvatarChat?
    private var chatter = Chatter()

    private let client: DaemonClient
    private var poll: Task<Void, Never>?
    private var wake: Task<Void, Never>?

    init(client: DaemonClient) {
        self.client = client
        showAvatar = UserDefaults.standard.object(forKey: "avatar") as? Bool ?? true
        roam = UserDefaults.standard.object(forKey: "roam") as? Bool ?? true
        follow = UserDefaults.standard.object(forKey: "follow") as? Bool ?? true
        avatarStyle = AvatarStyle.resolve(UserDefaults.standard.string(forKey: AvatarStyle.key))
    }

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
            let result = try await client.sessions()
            self.result = result
            error = nil
            let now = Date().timeIntervalSinceReferenceDate
            let mood = drowsiness.mood(avatarMood(result), now: now)
            if self.mood != mood { self.mood = mood }
            if let text = chatter.update(result.available ? result.sessions : nil, now: now) {
                chat = AvatarChat(id: (chat?.id ?? 0) + 1, text: text)
            }
        } catch {
            self.error = error.localizedDescription
            if mood != .unknown { mood = .unknown }
        }
    }
}

/// 아바타의 혼잣말 한 마디 — 같은 글이 다시 나와도 `id` 로 새 말인 줄 안다.
struct AvatarChat: Equatable {
    var id: Int
    var text: String
}

extension AvatarStyle {
    /// 메뉴에 보이는 이름.
    var title: String {
        switch self {
        case .concept: "원작"
        case .minimal: "미니멀"
        case .pebble: "돌멩이"
        case .all: "모두"
        }
    }
}
