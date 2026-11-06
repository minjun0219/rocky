// 눈 가리기 — 비밀번호를 칠 때(시스템의 보안 입력이 켜졌을 때) 로키가 두 손으로 눈을 가린다(오너 요청 2026-11-06).
// 보안 입력이 켜졌는지는 앱이 읽고(`IsSecureEventInputEnabled`), 여기서는 가릴지와 손을 올린 정도만 정한다.
import Foundation

public struct EyeCover: Equatable, Sendable {
    /// 보안 입력이 이만큼 넘게 켜져 있으면 비밀번호 칸이 아니라 늘 켜 둔 것으로 본다 —
    /// 터미널의 "보안 키보드 입력"처럼 켜 두는 앱이 있어, 그대로 따르면 로키가 내내 눈을 가린다.
    public static let giveUp: Double = 60
    /// 손을 올리고 내리는 데 걸리는 시간.
    public static let ease: Double = 0.3

    /// 보안 입력이 켜진 시각 — 꺼지면 nil.
    var since: Double?
    /// 손을 올린 정도(0 … 1).
    public private(set) var amount: Double = 0

    public init() {}

    /// 가려야 하나 — 켜진 지 `giveUp` 초 안일 때만.
    public var covering: Bool { amount > 0 }

    @discardableResult
    public mutating func update(secure: Bool, now: Double, dt: Double) -> Double {
        since = secure ? since ?? now : nil
        let target: Double = since.map { now - $0 < Self.giveUp } == true ? 1 : 0
        let step = dt / Self.ease
        amount = target > amount ? min(target, amount + step) : max(target, amount - step)
        return amount
    }
}
