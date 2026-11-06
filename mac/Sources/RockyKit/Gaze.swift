// 사람의 눈길 — 로키가 사람이 보는 곳(포인터 · 앞 창 · 최근 입력) 가까이로 따라가게 하는 순수 판정.
// 신호를 읽는 건 앱 몫이고(포인터 자리, 마지막 입력 뒤 흐른 초, 앞 앱의 맨 앞 창), 여기서는 어디를 볼지와 어디에 설지만 정한다.
// 좌표는 `Roamer` 와 같은 AppKit 화면 좌표(원점 왼쪽 아래, y 는 위로)다.
import Foundation

/// 사람이 보고 있는 화면의 한 점.
public struct GazePoint: Equatable, Sendable {
    public var x: Double
    public var y: Double

    public init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }

    func distance(to x: Double, _ y: Double) -> Double { hypot(self.x - x, self.y - y) }
}

/// 한 번 읽은 신호.
public struct GazeInput: Equatable, Sendable {
    public var pointerX: Double
    public var pointerY: Double
    /// 포인터가 마지막으로 움직인 뒤 흐른 초.
    public var sincePointer: Double
    /// 마지막 키 입력 뒤 흐른 초 — 못 읽으면 아주 큰 값.
    public var sinceKey: Double
    /// 앞에 있는 앱의 맨 앞 창 — 없거나 로키 자신의 창이면 nil.
    public var focus: Box?
    /// 로키의 발 자리 — 로키를 향해 오는 포인터는 눈길로 치지 않는다.
    public var rockyX: Double
    public var rockyY: Double

    public init(
        pointerX: Double, pointerY: Double, sincePointer: Double, sinceKey: Double, focus: Box?,
        rockyX: Double, rockyY: Double
    ) {
        self.pointerX = pointerX
        self.pointerY = pointerY
        self.sincePointer = sincePointer
        self.sinceKey = sinceKey
        self.focus = focus
        self.rockyX = rockyX
        self.rockyY = rockyY
    }
}

/// 눈길을 좇는다 — 거슬리지 않게 느긋하게. 포인터가 움직이는 동안은 따라가지 않고 멈춘 뒤에야 보고, 조금 옮긴 것은 무시하며,
/// 로키 쪽으로 다가온 포인터(로키를 누르려는 손)는 보지 않는다. 한동안 아무 입력이 없으면 눈길을 놓는다(사람이 자리를 비웠다).
public struct GazeTracker: Equatable, Sendable {
    /// 포인터가 이만큼 멈춰 있어야 그 자리를 본다.
    public static let settle: Double = 0.6
    /// 키 입력이 이만큼 안이고 포인터보다 나중이면 앞 창을 본다.
    public static let typing: Double = 3
    /// 아무 입력이 이만큼 없으면 눈길을 놓는다.
    public static let away: Double = 120
    /// 눈길이 이만큼 넘게 옮겨야 따라간다.
    public static let minShift: Double = 180
    /// 로키 발에서 이만큼 안의 포인터는 로키를 보는 것으로 친다.
    public static let nearRocky: Double = 160

    public private(set) var target: GazePoint?

    public init() {}

    @discardableResult
    public mutating func update(_ input: GazeInput) -> GazePoint? {
        if min(input.sincePointer, input.sinceKey) > Self.away {
            target = nil
            return nil
        }
        var spot: GazePoint?
        if input.sinceKey < Self.typing, input.sinceKey < input.sincePointer, let focus = input.focus {
            // 입력 중 — 앞 창의 윗변. 포인터가 그 창 위에 있으면 포인터 쪽, 아니면 창 가운데.
            let inside = input.pointerX >= focus.minX && input.pointerX <= focus.maxX
            spot = GazePoint(x: inside ? input.pointerX : (focus.minX + focus.maxX) / 2, y: focus.maxY)
        } else if input.sincePointer >= Self.settle {
            let pointer = GazePoint(x: input.pointerX, y: input.pointerY)
            if pointer.distance(to: input.rockyX, input.rockyY) > Self.nearRocky { spot = pointer }
        }
        guard let spot else { return target }
        if let target, spot.distance(to: target.x, target.y) < Self.minShift { return target }
        target = spot
        return target
    }
}

/// 따라가서 설 자리 — 발판 위의 한 점.
public struct Goal: Equatable, Sendable {
    public var x: Double
    public var y: Double

    public init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }
}

/// 눈길 옆에 설 자리 — 눈길 바로 아래가 아니라 옆으로 `keepAway` 비켜서(포인터와 보던 글을 가리지 않게), 눈길보다 낮은 발판 중
/// 가장 높은 것. 양쪽을 다 보고 더 높은(눈길에 가까운) 쪽을 고르며, 높이가 같으면 로키가 있는 쪽. 둘 다 없으면 nil.
public func followGoal(gaze: GazePoint, ledges: [Ledge], fromX x: Double, keepAway: Double = 110) -> Goal? {
    let first: Double = x >= gaze.x ? 1 : -1
    let picks = [first, -first].compactMap { side -> Goal? in
        let gx = gaze.x + side * keepAway
        return ledges
            .filter {
                $0.y <= gaze.y + 1 && gx >= $0.minX + Roamer.halfWidth && gx <= $0.maxX - Roamer.halfWidth
            }
            .max { $0.y < $1.y }
            .map { Goal(x: gx, y: $0.y) }
    }
    // `max` 는 같은 값이면 뒤의 것을 고르므로, 로키 쪽(앞)을 남기려고 직접 비교한다.
    return picks.reduce(Goal?.none) { best, next in
        guard let best else { return next }
        return next.y > best.y ? next : best
    }
}
