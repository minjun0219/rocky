// rocky 맥 앱 — 메뉴 막대에 돌고 있는 일과 머지 후보 PR 을 늘 띄우고, 할 일 창에서 보드를 보고 착수·멈춤·완료·추가한다.
// 데몬의 클라이언트일 뿐 상주 서버가 아니다. 규칙은 docs/features/mac.md.
import AppKit
import RockyKit
import SwiftUI

@main
struct RockyMacApp: App {
    @State private var store: BoardStore

    init() {
        let store = BoardStore(
            settings: DaemonSettings.resolve(
                env: ProcessInfo.processInfo.environment,
                defaults: { UserDefaults.standard.string(forKey: $0) }
            )
        )
        store.start()
        _store = State(initialValue: store)
    }

    var body: some Scene {
        MenuBarExtra {
            MenuContent(store: store)
        } label: {
            MenuLabel(store: store)
        }
        .menuBarExtraStyle(.window)

        Window("rocky 보드", id: BoardWindow.id) {
            BoardWindow(store: store)
        }
        .defaultSize(width: 720, height: 560)
    }
}

/// 메뉴 막대의 글자 — 끊겼으면 빈 원, 아니면 체크리스트와 돌고 있는 일의 수.
struct MenuLabel: View {
    let store: BoardStore

    var body: some View {
        if !store.connected {
            Image(systemName: "circle.dashed")
        } else if store.doing.isEmpty {
            Image(systemName: "checklist")
        } else {
            Label("\(store.doing.count)", systemImage: "checklist")
                .labelStyle(.titleAndIcon)
        }
    }
}
