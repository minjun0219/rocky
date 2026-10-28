// 어디에 붙고 누구로 쓰나 — 앱 설정(`defaults write dev.minjun.rocky.mac <키> <값>`) > 환경 변수 > 기본값.
// 메뉴 막대 앱은 셸 환경을 받지 않으므로(Finder·로그인 항목으로 뜬다) 설정이 먼저다. 환경 변수는 터미널에서 띄울 때용.
import Foundation

public struct DaemonSettings: Equatable, Sendable {
    public var base: String
    public var actor: String

    /// 웹 UI 와 TUI 의 기본 actor.
    public static let defaultActor = "logan"
    public static let defaultPort = 8636

    /// `defaults` 는 `UserDefaults.standard.string(forKey:)` — 테스트는 사전으로 바꾼다.
    public static func resolve(env: [String: String], defaults: (String) -> String?) -> DaemonSettings {
        func pick(_ key: String) -> String? {
            defaults(key).flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 }
        }
        let port = env["ROCKY_TODO_PORT"].flatMap(Int.init) ?? defaultPort
        let base = pick("daemonURL") ?? "http://127.0.0.1:\(port)"
        let actor = pick("actor") ?? env["ROCKY_TODO_ACTOR"].flatMap { $0.isEmpty ? nil : $0 } ?? defaultActor
        return DaemonSettings(base: base, actor: actor)
    }
}
