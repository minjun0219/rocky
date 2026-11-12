// 로그인할 때 앱을 띄울지 — 메뉴의 "로그인할 때 열기"가 macOS 로그인 항목(`SMAppService.mainApp`)에 앱을 올리고 내린다
// (2026-11-06 첫 실행에 저절로 올렸다 → 2026-11-12 사람이 켤 때만). 데몬은 앱을 띄우지 않는다 — 앱은 데몬의 클라이언트일 뿐이고,
// 앱이 먼저 뜨면 데몬이 올라올 때까지 SSE 를 다시 붙는다. 규칙은 docs/features/mac.md.
import Foundation
import Observation
import ServiceManagement

@MainActor
@Observable
final class LoginItem {
    /// 지금 로그인 항목에 올라 있나 — 시스템 설정에서 사람이 끈 것도 따라간다(메뉴를 열 때 다시 읽는다).
    private(set) var enabled = false
    /// 마지막으로 켜고 끄다 실패한 까닭 — 다음 시도가 지운다.
    private(set) var error: String?

    /// .app 으로 떴을 때만 — `swift run` 의 실행 파일은 로그인 항목에 올릴 수 없다.
    let available = Bundle.main.bundleURL.pathExtension == "app"

    init() {
        // 저절로 올리지 않는다 — 앱은 `rocky app` 으로 필요할 때 띄우는 선택 부품이다(2026-11-12). 켜는 것은 메뉴의
        // "로그인할 때 열기"로 사람이 한다. 예전에 올라간 것은 그대로 둔다(사람이 켠 것과 구별하지 못한다).
        reload()
    }

    func reload() {
        guard available else { return }
        enabled = SMAppService.mainApp.status == .enabled
    }

    func set(_ on: Bool) {
        guard available else { return }
        do {
            if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            error = nil
        } catch {
            self.error = "로그인 항목을 \(on ? "켜지" : "끄지") 못했어요: \(error.localizedDescription)"
        }
        reload()
    }
}
