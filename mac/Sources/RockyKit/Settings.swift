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

/// 아바타 모양(`defaults` 의 `avatarStyle`) — 원작 시안을 잘라 낸 조각(`concept`), 도형 열몇 개로 그린 미니멀 벡터(`minimal`),
/// 둘을 나란히 띄워 비교하기(`both`). 모르는 값이나 빈 값은 원작이다.
public enum AvatarStyle: String, CaseIterable, Sendable {
    case concept, minimal, both

    public static let key = "avatarStyle"

    public static func resolve(_ raw: String?) -> AvatarStyle {
        raw.flatMap { AvatarStyle(rawValue: $0.trimmingCharacters(in: .whitespaces)) } ?? .concept
    }

    /// 그릴 로키들, 왼쪽에서 오른쪽 순서 — 원작은 늘 오른쪽 끝(패널이 발 자리로 삼는 칸)에 선다.
    public var figures: [AvatarLook] {
        switch self {
        case .concept: [.concept]
        case .minimal: [.minimal]
        case .both: [.minimal, .concept]
        }
    }
}

/// 로키 하나의 그림체.
public enum AvatarLook: Sendable {
    case concept, minimal
}
