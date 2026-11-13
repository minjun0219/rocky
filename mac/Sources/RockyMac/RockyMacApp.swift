// rocky 맥 앱 — 메뉴 막대에 에이전트 세션과 머지 후보 PR 을 늘 띄우고, 떠 있는 아바타가 세션 상태를 보인다.
// 데몬의 클라이언트일 뿐 상주 서버가 아니다. 규칙은 docs/features/mac.md.
import AppKit
import RockyKit
import SwiftUI

@main
struct RockyMacApp: App {
    @State private var store: DaemonStore
    @State private var agents: AgentStore
    @State private var avatar: AvatarController
    @State private var loginItem: LoginItem

    init() {
        let store = DaemonStore(
            settings: DaemonSettings.resolve(
                env: ProcessInfo.processInfo.environment,
                defaults: { UserDefaults.standard.string(forKey: $0) }
            )
        )
        store.start()
        let agents = AgentStore(client: store.client)
        agents.start()
        _store = State(initialValue: store)
        _agents = State(initialValue: agents)
        _avatar = State(initialValue: AvatarController(agents: agents))
        _loginItem = State(initialValue: LoginItem())
    }

    var body: some Scene {
        MenuBarExtra {
            MenuContent(store: store, agents: agents, loginItem: loginItem)
        } label: {
            MenuLabel(store: store, agents: agents)
        }
        .menuBarExtraStyle(.window)
    }
}

/// 메뉴 막대의 글자 — 끊겼으면 빈 원, 내 차례인 세션이 있으면 말풍선과 그 수, 머지 후보가 있으면 PR 표시와 그 수, 아니면 체크 표시.
struct MenuLabel: View {
    let store: DaemonStore
    let agents: AgentStore

    var body: some View {
        if !store.connected {
            Image(systemName: "circle.dashed")
        } else if case let .waiting(_, _, count) = agents.mood {
            Label("\(count)", systemImage: "exclamationmark.bubble.fill")
                .labelStyle(.titleAndIcon)
        } else if store.readyPullRequests.isEmpty {
            Image(systemName: "checkmark.circle")
        } else {
            Label("\(store.readyPullRequests.count)", systemImage: "arrow.triangle.pull")
                .labelStyle(.titleAndIcon)
        }
    }
}
