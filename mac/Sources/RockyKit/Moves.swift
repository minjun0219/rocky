// 서 있는 로키의 몸짓 고르기 — 같은 동작만 되풀이하지 않게 세 층으로 쌓는다.
// 바탕(기분마다 도는 루프) 위에 가끔 하는 작은 동작을 가중 랜덤으로 얹고(직전 것은 빼고), 길이와 박자는 범위에서 뽑는다.
// 서 있는 자리(바닥 · 창 윗면 · 창 끝)마다 후보가 다르다. 드문 사건(튜브)은 `Roamer` 몫이다.
import Foundation

/// 한 토막의 몸짓.
public enum Move: String, CaseIterable, Sendable {
    /// 기분이 정하는 바탕 루프 그대로(다 쉼 · 연결 없음).
    case mood
    /// 도는 중 — 망치질, 그리고 망치질 사이에 쉬는 동작들.
    case hammer, look, wipe, shoulder, stomp, sit, peek
    /// 내 차례 — 처음엔 크게 뛰며 손을 흔들고, 그 뒤엔 마우스 쪽을 보고 서 있다가 가끔 손만 흔든다.
    case cheer, watch, wave
}

/// 서 있는 자리 — 창 끝이면 내려다보기가 후보에 든다(`toward` 는 끝이 있는 쪽, +1 오른쪽).
public enum Spot: Equatable, Sendable {
    case floor
    case window
    case edge(toward: Double)
}

/// 발판 위의 자리 — 첫 발판(화면 바닥)이면 바닥, 끝에서 `margin` 안이면 창 끝. 발판이 없으면 바닥으로 본다.
public func spot(x: Double, y: Double, ledges: [Ledge], margin: Double = 40) -> Spot {
    guard let ground = ledges.first(where: { abs($0.y - y) <= 2 && x >= $0.minX && x <= $0.maxX }),
          ground != ledges.first
    else { return .floor }
    if x - ground.minX < margin { return .edge(toward: -1) }
    if ground.maxX - x < margin { return .edge(toward: 1) }
    return .window
}

/// 지금 하는 몸짓 — `start` 는 시작 시각(초), `length` 는 길이, `tempo` 는 한 박자(망치질 한 번)의 길이.
public struct Clip: Equatable, Sendable {
    public var move: Move
    public var start: Double
    public var length: Double
    public var tempo: Double

    public init(move: Move, start: Double, length: Double, tempo: Double) {
        self.move = move
        self.start = start
        self.length = length
        self.tempo = tempo
    }

    public var end: Double { start + length }

    /// 토막 안에서 흐른 비율(0...1).
    public func progress(at now: Double) -> Double {
        length > 0 ? min(max((now - start) / length, 0), 1) : 1
    }
}

/// 몸짓을 고르는 쪽이 보는 기분 — 앱의 `AvatarMood` 를 줄인 것.
public enum MoveMood: Equatable, Sendable {
    case working
    /// 내 차례 — 기다리는 세션이 바뀌면(`who`) 다시 크게 알린다.
    case waiting(who: String)
    case other
}

/// 쉬는 동작 후보 하나 — 가중치와 길이 범위(초).
public struct Break: Equatable, Sendable {
    public var move: Move
    public var weight: Double
    public var length: ClosedRange<Double>
}

public struct Choreographer: Equatable, Sendable {
    /// 쉬는 동작 후보 — 창 끝이면 내려다보기가 들고, 바닥이면 앉기가 더 잦다.
    public static func breaks(at spot: Spot) -> [Break] {
        var out = [
            Break(move: .look, weight: 4, length: 1.6 ... 2.6),
            Break(move: .wipe, weight: 3, length: 1.4 ... 2.0),
            Break(move: .shoulder, weight: 2, length: 1.2 ... 1.8),
            Break(move: .stomp, weight: 1.5, length: 0.8 ... 1.2),
            Break(move: .sit, weight: spot == .floor ? 2 : 1, length: 2.5 ... 4),
        ]
        if case .edge = spot { out.append(Break(move: .peek, weight: 3, length: 1.5 ... 2.5)) }
        return out
    }

    /// 망치질 한 번의 박자와, 쉬기 전에 내리치는 횟수.
    public static let hammerTempo: ClosedRange<Double> = 0.7 ... 0.95
    public static let strikes: ClosedRange<Int> = 3 ... 7
    /// 내 차례 — 처음 크게 알리는 시간(뛰기·흔들기 2.4초 두 바퀴), 그 뒤 손 흔드는 간격과 길이.
    public static let cheerLength: Double = 4.8
    public static let waveEvery: ClosedRange<Double> = 10 ... 20
    public static let waveLength: Double = 1.6

    public private(set) var clip = Clip(move: .mood, start: 0, length: 0, tempo: 1)
    var mood: MoveMood?
    /// 방금 한 쉬는 동작 — 다음 쉬는 동작에서 뺀다.
    var lastBreak: Move?

    public init() {}

    /// `now` 의 몸짓 — 기분이 바뀌었거나 지금 토막이 끝났으면 다음 것을 고른다.
    @discardableResult
    public mutating func update(now: Double, mood: MoveMood, spot: Spot, random: () -> Double) -> Clip {
        if mood != self.mood {
            self.mood = mood
            lastBreak = nil
            begin(first(for: mood), now: now, spot: spot, random: random)
        } else if now >= clip.end, clip.move != .mood {
            begin(next(after: clip.move, spot: spot, random: random), now: now, spot: spot, random: random)
        }
        return clip
    }

    private func first(for mood: MoveMood) -> Move {
        switch mood {
        case .working: .hammer
        case .waiting: .cheer
        case .other: .mood
        }
    }

    private mutating func next(after move: Move, spot: Spot, random: () -> Double) -> Move {
        switch move {
        case .hammer:
            let options = Self.breaks(at: spot).filter { $0.move != lastBreak }
            let pick = Self.weighted(options.map { $0.weight }, random())
            lastBreak = options[pick].move
            return lastBreak!
        case .look, .wipe, .shoulder, .stomp, .sit, .peek: return .hammer
        case .cheer, .wave: return .watch
        case .watch: return .wave
        case .mood: return .mood
        }
    }

    private mutating func begin(_ move: Move, now: Double, spot: Spot, random: () -> Double) {
        switch move {
        case .hammer:
            let tempo = Self.pick(Self.hammerTempo, random())
            let count = Self.strikes.lowerBound
                + min(Int(random() * Double(Self.strikes.count)), Self.strikes.count - 1)
            clip = Clip(move: .hammer, start: now, length: tempo * Double(count), tempo: tempo)
        case .cheer:
            clip = Clip(move: .cheer, start: now, length: Self.cheerLength, tempo: 2.4)
        case .watch:
            clip = Clip(move: .watch, start: now, length: Self.pick(Self.waveEvery, random()), tempo: 1)
        case .wave:
            clip = Clip(move: .wave, start: now, length: Self.waveLength, tempo: 1)
        case .mood:
            clip = Clip(move: .mood, start: now, length: 0, tempo: 1)
        case .look, .wipe, .shoulder, .stomp, .sit, .peek:
            let range = Self.breaks(at: spot).first { $0.move == move }?.length ?? 1 ... 2
            clip = Clip(move: move, start: now, length: Self.pick(range, random()), tempo: 1)
        }
    }

    static func pick(_ range: ClosedRange<Double>, _ r: Double) -> Double {
        range.lowerBound + (range.upperBound - range.lowerBound) * r
    }

    /// 가중치 목록에서 하나 — `r`(0..<1)이 누적 가중치의 어느 칸에 떨어지는지.
    static func weighted(_ weights: [Double], _ r: Double) -> Int {
        let total = weights.reduce(0, +)
        var acc = 0.0
        for (i, w) in weights.enumerated() {
            acc += w
            if r * total < acc { return i }
        }
        return weights.count - 1
    }
}
