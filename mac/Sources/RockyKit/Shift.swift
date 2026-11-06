// 자세 바꾸기 — 기분 · 몸짓 · 움직임이 바뀌면 그림이 앞 자세에서 새 자세로 섞어 넘어가게 장면을 층으로 쌓는다(섞는 셈은 RockyMac 의 `Rocky`).
// 바뀔 때마다 새 층을 얹고, 층은 바뀐 갈래에 맞는 시간 동안 아래 층들 위로 섞여 들어온다. 다 들어온 층 아래는 버린다.
// 한 프레임에 자세를 갈아 끼우면 표정 · 팔이 툭 튀고, 시안 그림을 통째로 그리는 자세(엎드려 자기 · 갸웃)는 서 있다가 한 프레임 만에
// 엎드렸다(2026-11-06 맥 캡처 실측, 오너 지적 "자세 바꿀 때도 자연스럽게").
import Foundation

/// 자세를 고르는 기분 — `AvatarMood` 에서 세션 이름 · 개수를 뺀 갈래. 이름이나 개수만 바뀌면 자세는 그대로다.
public enum PoseMood: Equatable, Sendable {
    case working, waiting, awake, resting, unknown
}

extension AvatarMood {
    public var poseMood: PoseMood {
        switch self {
        case .working: .working
        case .waiting: .waiting
        case .awake: .awake
        case .resting: .resting
        case .unknown: .unknown
        }
    }
}

/// 한 순간 로키가 하는 것 — 그림이 자세를 짜는 재료.
public struct Moment: Equatable, Sendable {
    public var mood: PoseMood
    public var action: RoamAction
    public var clip: Clip
    /// 점프 전 쪼그림과 뜬 정도(0 → 1, `Roamer.crouch` · `flight`) — 흐르는 값이라 바뀌어도 새 층을 얹지 않는다.
    public var crouch: Double
    public var flight: Double
    /// 지금 기분이 시작된 시각 — 잠들기 · 갸웃의 앞 동작을 여기서 잰다. `Shifts` 가 적고, 처음 장면은 오래전으로 친다.
    public internal(set) var since = -Double.infinity

    public init(mood: PoseMood, action: RoamAction, clip: Clip, crouch: Double = 0, flight: Double = 0) {
        self.mood = mood
        self.action = action
        self.clip = clip
        self.crouch = crouch
        self.flight = flight
    }

    /// 시안 그림을 통째로 그리는 장면 — 다 쉬거나(엎드려 자기) 연결이 없을 때(갸웃) 서 있거나 앉아 있으면.
    public var whole: Bool { (mood == .resting || mood == .unknown) && (action == .stand || action == .sit) }

    /// 같은 장면인지 — 기분 · 움직임 · 몸짓이 같으면. 흐르는 값(쪼그림 · 뜬 정도)은 보지 않는다.
    func same(as other: Moment) -> Bool {
        mood == other.mood && action == other.action && clip == other.clip
    }
}

/// 그림이 섞는 장면의 층 — 맨 뒤가 지금 장면이고, 그 앞은 섞여 사라지는 중인 앞 장면들(바뀔 때의 흐르는 값 그대로)이다.
public struct Shifts: Equatable, Sendable {
    public struct Layer: Equatable, Sendable {
        public var moment: Moment
        /// 섞여 들어오기 시작한 시각과 다 들어오는 데 걸리는 시간.
        public var start: Double
        public var length: Double

        /// 섞여 들어온 정도(0 … 1, 직선) — 그림이 처음과 끝이 느린 곡선으로 바꿔 쓴다.
        public func weight(at t: Double) -> Double {
            length > 0 ? min(max((t - start) / length, 0), 1) : 1
        }
    }

    /// 섞는 시간 — 움직임만 바뀌면(걷다 서기 · 떨어지다 내려앉기) 0.2초, 기분이나 몸짓이 바뀌면 0.4초.
    /// 통째 그림으로 들 때는 0.5초(관절이 그 그림의 쌍둥이 자세로 간 뒤 겹쳐 바꾼다), 날 때는 0.9초(그림이 옅어진 뒤 일어난다).
    public static let actionShift = 0.2
    public static let moveShift = 0.4
    public static let intoWhole = 0.5
    public static let outOfWhole = 0.9
    /// 한 번에 섞는 층 수의 상한 — 넘치면 오래된 것부터 버린다(그 한 프레임만 조금 튄다).
    public static let most = 4

    public private(set) var layers: [Layer]

    /// 처음 장면 — 기분은 오래전에 시작된 것으로 친다(앱을 켜자마자 잠들기 동작을 하지 않게).
    public init(_ moment: Moment) {
        var first = moment
        first.since = -.infinity
        layers = [Layer(moment: first, start: -.infinity, length: 0)]
    }

    /// 지금 장면.
    public var current: Moment { layers[layers.count - 1].moment }

    /// 이번 프레임의 장면 — 같은 장면이면 흐르는 값만 고치고, 바뀌었으면 새 층을 얹는다. 다 섞여 들어온 층 아래는 버린다.
    public mutating func update(_ moment: Moment, now: Double) {
        var next = moment
        let top = current
        next.since = next.mood == top.mood ? top.since : now
        if next.same(as: top) {
            layers[layers.count - 1].moment = next
        } else {
            layers.append(Layer(moment: next, start: now, length: Self.length(from: top, to: next)))
        }
        if let covered = layers.indices.dropFirst().last(where: { layers[$0].weight(at: now) >= 1 }) {
            layers.removeFirst(covered)
        }
        if layers.count > Self.most { layers.removeFirst(layers.count - Self.most) }
    }

    /// 앞 장면에서 다음 장면으로 섞는 시간.
    static func length(from a: Moment, to b: Moment) -> Double {
        if a.whole, !b.whole || a.mood != b.mood { return outOfWhole }
        if b.whole, !a.whole { return intoWhole }
        if a.mood != b.mood || a.clip != b.clip { return moveShift }
        return actionShift
    }
}
