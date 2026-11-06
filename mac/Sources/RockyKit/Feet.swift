// 두 발 — 걸을 때 한 발씩 들어 앞으로 옮겨 디딘다. 디딘 발은 화면에 붙어 있어 몸만 그 위를 지나간다.
// 예전엔 두 다리를 같은 쪽으로 흔들기만 하고 발이 몸과 같이 초당 45pt 로 미끄러져 빙판을 미끄러지는 듯했다(2026-11-06 캡처 실측).
// 그림(RockyMac)은 `Stance` 를 받아 다리 조각을 그 자리로 옮기고 기울인다.
import Foundation

/// 몸에서 본 발 하나 — 엉덩이 아래 제자리에서 화면 오른쪽(+)으로 나간 거리와 든 높이(pt).
public struct Footing: Equatable, Sendable {
    public var offset: Double
    public var lift: Double

    public init(offset: Double, lift: Double) {
        self.offset = offset
        self.lift = lift
    }

    public static let rest = Footing(offset: 0, lift: 0)
}

/// 두 발의 자리 — 화면에서 본 왼발과 오른발.
public struct Stance: Equatable, Sendable {
    public var left: Footing
    public var right: Footing

    public init(left: Footing, right: Footing) {
        self.left = left
        self.right = right
    }

    public static let rest = Stance(left: .rest, right: .rest)

    /// 오른쪽을 보고 짠 그림의 왼다리 · 오른다리 — 왼쪽을 보면 그림을 좌우로 뒤집으므로 화면의 오른발이 그림의 왼다리이고 앞뒤도 바뀐다.
    public func drawn(facing: Double) -> (left: Footing, right: Footing) {
        guard facing < 0 else { return (left, right) }
        return (Footing(offset: -right.offset, lift: right.lift), Footing(offset: -left.offset, lift: left.lift))
    }
}

/// 걸음 — 몸(로키 발 자리의 화면 x)이 움직인 거리로 박자를 세어 반 바퀴마다 한 발씩 든다. 든 발은 `swing` 초 동안 옮겨
/// 몸이 그때 있을 자리 앞쪽에 디딘다. 거리로 세므로 빨라지든 줄든 디딘 발이 미끄러지지 않는다.
/// 멈춰 서면 엉덩이 아래에서 벗어난 발을 한 발씩 모은다.
public struct Feet: Equatable, Sendable {
    /// 엉덩이 반 간격(pt) — 그림의 두 엉덩이(조각 칸 164 · 352)를 패널 크기로.
    public static let hip: Double = 15
    /// 한 바퀴(두 걸음)의 거리 — 천천히 걸으면 짧게, 다 빨라지면(`Roamer.walkSpeed`) 길게. 다 빨라지면 0.23초에 한 걸음.
    public static let cycleSlow: Double = 12
    public static let cycleFast: Double = 21
    /// 발을 들어 옮기는 시간과 드는 높이(pt).
    public static let swing: Double = 0.18
    public static let height: Double = 2.5
    /// 멈춘 뒤 이만큼 넘게 벗어난 발은 모은다. 걷기 시작해 이만큼 가면 첫 발을 든다.
    public static let settle: Double = 1.5
    public static let lead: Double = 0.5

    struct Foot: Equatable, Sendable {
        var x: Double
        var lift: Double = 0
        var from: Double = 0
        /// 든 뒤 흐른 시간 — 디디고 있으면 nil.
        var age: Double?
    }

    /// 화면의 왼발 · 오른발.
    private(set) var feet: [Foot]
    private var last: Double
    private var phase: Double = 0
    /// 걷는 쪽(+1 오른쪽) — 서 있으면 nil. 쪽이 바뀌면 박자를 처음부터 센다.
    private var heading: Double?

    public init(x: Double) {
        feet = [Foot(x: x - Self.hip), Foot(x: x + Self.hip)]
        last = x
    }

    private static func home(_ i: Int, _ x: Double) -> Double {
        x + (i == 0 ? -hip : hip)
    }

    /// 발판 위의 한 프레임 — `x` 는 이번 프레임의 몸 자리.
    public mutating func step(x: Double, dt: Double) {
        guard dt > 0 else { return }
        let dx = x - last
        last = x
        let v = dx / dt
        let moving = abs(v) > 0.5
        let toward: Double = v > 0 ? 1 : -1
        let cycle = Self.cycleSlow + (Self.cycleFast - Self.cycleSlow) * min(abs(v) / Roamer.walkSpeed, 1)
        // 디딜 자리 — 디딘 동안 몸이 지나가는 길의 가운데에 엉덩이가 오게(앞뒤로 같은 만큼).
        let reach = max(0, (cycle - abs(v) * Self.swing) / 2)
        if moving {
            if heading != toward {
                heading = toward
                phase = 1 - Self.lead / cycle
            }
            let before = phase
            phase += abs(dx) / cycle
            // 가는 쪽 발이 먼저(바퀴가 넘어갈 때), 다른 발은 반 바퀴 뒤에.
            let front = toward > 0 ? 1 : 0
            if before.rounded(.down) != phase.rounded(.down) { raise(front) }
            if (before - 0.5).rounded(.down) != (phase - 0.5).rounded(.down) { raise(1 - front) }
        } else {
            heading = nil
        }
        for i in feet.indices {
            guard let age = feet[i].age else { continue }
            let now = age + dt
            let u = min(now / Self.swing, 1)
            // 내릴 자리는 몸이 내릴 때쯤 있을 자리 기준 — 빨라지거나 줄면 따라 고친다.
            let to = Self.home(i, x) + (moving ? toward * reach + v * max(Self.swing - now, 0) : 0)
            feet[i].x = feet[i].from + (to - feet[i].from) * u * u * (3 - 2 * u)
            feet[i].lift = u < 1 ? Self.height * sin(.pi * u) : 0
            feet[i].age = u < 1 ? now : nil
        }
        // 멈춰 서면 — 엉덩이 아래에서 벗어난 발을 한 발씩 모은다(먼 발부터).
        if !moving, feet.allSatisfy({ $0.age == nil }) {
            let off = feet.indices.map { abs(feet[$0].x - Self.home($0, x)) }
            let far = off[0] >= off[1] ? 0 : 1
            if off[far] > Self.settle { raise(far) }
        }
    }

    /// 발판을 떠나 있는 동안(뛰어오름 · 떨어지기 · 튜브) — 발을 몸과 같이 옮기며 0.08초쯤에 엉덩이 아래로 끌어온다.
    /// 한 프레임에 붙이면 걷던 다리가 툭 튄다.
    public mutating func airborne(x: Double, dt: Double) {
        let keep = max(0, 1 - dt / 0.08)
        for i in feet.indices {
            feet[i].x = Self.home(i, x) + (feet[i].x - Self.home(i, last)) * keep
            feet[i].lift *= keep
            feet[i].age = nil
        }
        last = x
        heading = nil
    }

    private mutating func raise(_ i: Int) {
        guard feet[i].age == nil else { return }
        feet[i].age = 0
        feet[i].from = feet[i].x
    }

    /// 몸이 `x` 에 있을 때 두 발의 자리.
    public func stance(at x: Double) -> Stance {
        Stance(
            left: Footing(offset: feet[0].x - Self.home(0, x), lift: feet[0].lift),
            right: Footing(offset: feet[1].x - Self.home(1, x), lift: feet[1].lift)
        )
    }
}
