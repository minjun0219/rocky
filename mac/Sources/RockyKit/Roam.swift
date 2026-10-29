// 돌아다니는 로키 — 창 윗면을 걷고, 발밑이 사라지면 떨어지고, 낮은 창 옆면은 뛰어오르고 높은 창은 튜브로 넘고,
// 창 사이 틈은 건너뛴다. 바닥에 오래 머물지 않게 헤일메리의 터널처럼 튜브를 놓아 창 윗면으로 올라가고,
// 올라갈 창이 없으면 화면을 크게 돌아 바닥의 먼 곳으로 건너간다.
// 좌표는 AppKit 화면 좌표(원점 왼쪽 아래, y 는 위로)이고 (x, y) 는 로키의 발 가운데다. 창 목록을 읽고 패널을 옮기는 건 앱 몫이다.
import Foundation

/// 화면 위의 사각형 — 창 하나 또는 화면의 보이는 영역.
public struct Box: Equatable, Sendable {
    public var minX: Double
    public var minY: Double
    public var maxX: Double
    public var maxY: Double

    public init(x: Double, y: Double, width: Double, height: Double) {
        minX = x
        minY = y
        maxX = x + width
        maxY = y + height
    }
}

/// 밟을 수 있는 가로줄 — 창 윗면에서 앞 창에 가리지 않은 구간, 또는 화면 바닥.
public struct Ledge: Equatable, Sendable {
    public var y: Double
    public var minX: Double
    public var maxX: Double
}

/// 앞을 막는 세로줄 — 창의 옆면. `faces` 는 벽이 바라보는 쪽(-1 = 왼쪽 옆면, +1 = 오른쪽 옆면).
public struct Wall: Equatable, Sendable {
    public var x: Double
    public var bottom: Double
    public var top: Double
    public var faces: Double
}

/// 앞에서 뒤 순서의 창들로 발판을 만든다. 윗면 바로 위를 앞 창이 덮는 구간은 빼고, 화면 밖이나
/// 로키가 설 틈(`headroom`)이 없는 윗면은 버린다. 화면 바닥은 늘 첫 발판이다.
public func ledges(windows: [Box], screen: Box, headroom: Double = 90, minWidth: Double = 40) -> [Ledge] {
    var out = [Ledge(y: screen.minY, minX: screen.minX, maxX: screen.maxX)]
    for (i, window) in windows.enumerated() {
        let top = window.maxY
        guard top > screen.minY, top + headroom <= screen.maxY else { continue }
        var pieces = [(max(window.minX, screen.minX), min(window.maxX, screen.maxX))]
        for front in windows[..<i] where front.minY <= top && front.maxY > top {
            pieces = pieces.flatMap { piece -> [(Double, Double)] in
                guard front.maxX > piece.0, front.minX < piece.1 else { return [piece] }
                return [(piece.0, front.minX), (front.maxX, piece.1)].filter { $0.1 > $0.0 }
            }
        }
        for piece in pieces where piece.1 - piece.0 >= minWidth {
            out.append(Ledge(y: top, minX: piece.0, maxX: piece.1))
        }
    }
    return out
}

/// 창 옆면 — 화면 안에 있는 부분만. 가려졌는지는 보지 않는다(벽은 윗면보다 관대해도 어색하지 않다).
public func walls(windows: [Box], screen: Box, headroom: Double = 90) -> [Wall] {
    windows.flatMap { window -> [Wall] in
        let bottom = max(window.minY, screen.minY)
        guard window.maxY + headroom <= screen.maxY, window.maxY > bottom else { return [] }
        return [
            Wall(x: window.minX, bottom: bottom, top: window.maxY, faces: -1),
            Wall(x: window.maxX, bottom: bottom, top: window.maxY, faces: 1),
        ].filter { $0.x > screen.minX && $0.x < screen.maxX }
    }
}

/// 기분이 정하는 움직임 — 일하는 중이면 돌아다니고, 내 차례면 그 자리에 서서 알리고, 다 쉬면 앉아 존다.
public enum RoamMood: Equatable, Sendable {
    case roam, stay, sleep
}

public enum RoamAction: Equatable, Sendable {
    case walk, stand, sit, fall, jump, tube
}

/// 두 발판을 잇는 튜브 — 출발점에서 위로 솟았다가 도착점으로 내려오는 3차 베지어. 꼭짓점 높이는 `peak`.
public struct Tube: Equatable, Sendable {
    public var startX: Double
    public var startY: Double
    public var endX: Double
    public var endY: Double
    public var peak: Double

    /// 화면 꼭대기를 넘지 않게 두 끝보다 `rise` 만큼 위로 솟는다.
    public init(startX: Double, startY: Double, endX: Double, endY: Double, screen: Box, rise: Double = 140) {
        self.startX = startX
        self.startY = startY
        self.endX = endX
        self.endY = endY
        peak = min(max(startY, endY) + rise, screen.maxY - 30)
    }

    /// t(0...1) 자리의 점.
    public func point(at t: Double) -> (x: Double, y: Double) {
        let u = 1 - t
        let a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t
        return (a * startX + b * startX + c * endX + d * endX, a * startY + (b + c) * peak + d * endY)
    }

    /// 꺾은선으로 잰 길이.
    public var length: Double {
        var total = 0.0
        var prev = point(at: 0)
        for i in 1 ... 32 {
            let next = point(at: Double(i) / 32)
            total += hypot(next.x - prev.x, next.y - prev.y)
            prev = next
        }
        return total
    }
}

/// 로키의 몸 — `step` 이 한 프레임씩 움직인다. 무엇을 할지 고르는 운은 `random`(0..<1)으로 받는다.
public struct Roamer: Equatable, Sendable {
    public static let walkSpeed: Double = 45
    public static let gravity: Double = 1800
    /// 몸 반폭 — 벽에 붙거나 화면 끝에서 돌아설 때 쓴다.
    public static let halfWidth: Double = 28
    /// 튜브 — 걷는 동안 초당 이만큼의 확률로 놓는다. 바닥에서 올라갈 창 윗면이 있으면 `upRate`, 그 밖에는 `tubeRate`.
    /// 놓는 데 `tubeBuild` 초, 건너는 속도, 다 건너고 남아 있다 사라지는 시간.
    public static let tubeRate: Double = 0.04
    public static let upRate: Double = 0.15
    public static let tubeBuild: Double = 0.6
    public static let tubeSpeed: Double = 320
    public static let tubeLinger: Double = 0.8
    public static let tubeFade: Double = 0.4
    /// 올라갈 창이 없는 바닥에서 튜브로 건널 때 — 적어도 이만큼 떨어진 곳으로, 화면 높이의 이 비율만큼 솟는다.
    public static let crossing: Double = 300
    public static let crossingRise: ClosedRange<Double> = 0.3 ... 0.6
    /// 착지하면 이만큼 쭈그렸다 편다.
    public static let landTime: Double = 0.2
    /// 점프 — 쪼그리는 시간, 떠 있는 시간, 닿을 수 있는 높이(이보다 높은 창 옆면은 튜브로 넘는다), 내려설 곳보다 더 솟는 높이.
    public static let jumpCrouch: Double = 0.3
    public static let jumpAir: Double = 0.6
    public static let jumpReach: Double = 260
    public static let jumpLift: Double = 40
    /// 발판 끝 너머 이 틈 안에 다른 발판이 있으면 건너뛴다. 건너뛸 데가 없으면 `dropChance` 로 뛰어내리고 나머지는 돌아선다.
    public static let hopGap: Double = 160
    public static let dropChance: Double = 0.3

    public var x: Double
    public var y: Double
    /// 바라보는 쪽(+1 오른쪽, -1 왼쪽).
    public var facing: Double = 1
    public var action: RoamAction = .fall
    public var vy: Double = 0
    /// 서 있거나 앉아 있을 남은 시간 — 돌아다닐 때만 쓴다.
    var pause: Double = 0
    /// 뛰는 출발점과 내려설 자리, 쪼그리기 시작한 뒤 흐른 시간.
    var jumpFrom = Point(x: 0, y: 0)
    var jumpTo = Point(x: 0, y: 0)
    var jumpAge: Double = 0
    /// 놓인 튜브와 놓은 뒤 흐른 시간 — 다 건넌 뒤에도 잠깐 남았다가 사라진다.
    public private(set) var tube: Tube?
    public private(set) var tubeAge: Double = 0
    var landing: Double = 0

    struct Point: Equatable, Sendable {
        var x: Double
        var y: Double
    }

    public init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }

    /// 끌어다 놓았을 때 — 그 자리에서 떨어지기 시작한다.
    public mutating func drop(atX x: Double, y: Double) {
        self.x = x
        self.y = y
        action = .fall
        vy = 0
        tube = nil
    }

    /// 튜브를 건너는 데 걸리는 시간.
    var tubeTravel: Double { (tube?.length ?? 0) / Self.tubeSpeed }

    /// 튜브가 그려진 정도(0...1) — 놓는 동안 출발점에서부터 자란다.
    public var tubeReveal: Double { min(tubeAge / Self.tubeBuild, 1) }

    /// 튜브의 불투명도 — 다 건너고 잠깐 뒤에 옅어진다.
    public var tubeOpacity: Double {
        let gone = tubeAge - Self.tubeBuild - tubeTravel - Self.tubeLinger
        return gone > 0 ? max(0, 1 - gone / Self.tubeFade) : 1
    }

    /// 착지 반동(1 → 0) — 내려앉은 직후 쭈그렸다 편다.
    public var squash: Double { landing / Self.landTime }

    /// 점프 전 쪼그림(0 → 1) — 뛰기 직전에 가장 깊다. 떠 있는 동안과 점프가 아닐 때는 0.
    public var crouch: Double {
        action == .jump && jumpAge < Self.jumpCrouch ? jumpAge / Self.jumpCrouch : 0
    }

    /// 튜브 안에 들어가 있는지 — 그동안 몸을 줄인다.
    public var insideTube: Bool { action == .tube && tubeAge >= Self.tubeBuild }

    public mutating func step(
        dt: Double, ledges: [Ledge], walls: [Wall], screen: Box, mood: RoamMood, random: () -> Double
    ) {
        landing = max(0, landing - dt)
        if tube != nil {
            tubeAge += dt
            if action != .tube, tubeOpacity <= 0 { tube = nil }
        }
        switch action {
        case .tube:
            // 튜브를 다 놓으면 그 길을 따라 건너간다 — 기분이 바뀌어도 건너던 건 마저 건넌다.
            guard let tube else {
                action = .fall
                return
            }
            let t = min(max((tubeAge - Self.tubeBuild) / max(tubeTravel, 0.01), 0), 1)
            let at = tube.point(at: t)
            x = at.x
            y = at.y
            if t >= 1 {
                vy = 0
                action = .stand
                pause = 1
            }
        case .fall:
            vy -= Self.gravity * dt
            let next = y + vy * dt
            // 지나쳐 내려간 발판 중 가장 높은 것에 내려앉는다.
            let below = ledges
                .filter { $0.y <= y + 0.5 && $0.y >= next && x >= $0.minX && x <= $0.maxX }
                .max { $0.y < $1.y }
            if let below {
                y = below.y
                vy = 0
                settle(mood)
                pause = 0.5
                landing = Self.landTime
            } else {
                y = max(next, screen.minY)
            }
        case .jump:
            guard mood == .roam else {
                action = .fall
                return
            }
            // 내려설 발판이 사라지거나 옮겨 가면 그 자리에서 떨어진다.
            if Self.ledge(at: jumpTo, in: ledges) == nil {
                action = .fall
                vy = 0
                return
            }
            jumpAge += dt
            let u = (jumpAge - Self.jumpCrouch) / Self.jumpAir
            guard u > 0 else { return }
            if u >= 1 {
                (x, y) = (jumpTo.x, jumpTo.y)
                vy = 0
                action = .stand
                pause = 0.4
                landing = Self.landTime
                return
            }
            if jumpTo.y > jumpFrom.y + 10 {
                // 위로 — 앞쪽 60% 는 거의 곧게 솟아 윗면보다 `jumpLift` 만큼 높이 오르고, 나머지 동안 앞으로 넘어가며 내려앉는다.
                let peak = jumpTo.y + Self.jumpLift
                if u < 0.6 {
                    let r = u / 0.6
                    y = jumpFrom.y + (peak - jumpFrom.y) * (1 - (1 - r) * (1 - r))
                    x = jumpFrom.x
                } else {
                    let r = (u - 0.6) / 0.4
                    y = peak - Self.jumpLift * r * r
                    x = jumpFrom.x + (jumpTo.x - jumpFrom.x) * r * r * (3 - 2 * r)
                }
            } else {
                // 옆이나 아래로 — 높은 쪽 끝보다 `jumpLift` 만큼 솟는 포물선으로 건너간다(y = y0 + a·u² + b·u, 꼭짓점이 그 높이).
                let d = jumpTo.y - jumpFrom.y, k = max(d, 0) + Self.jumpLift
                let b = 2 * k + 2 * (k * (k - d)).squareRoot(), a = d - b
                y = jumpFrom.y + a * u * u + b * u
                x = jumpFrom.x + (jumpTo.x - jumpFrom.x) * u
            }
        case .walk, .stand, .sit:
            guard let ground = ledges.first(where: { abs($0.y - y) <= 2 && x >= $0.minX && x <= $0.maxX }) else {
                action = .fall
                vy = 0
                return
            }
            y = ground.y
            guard mood == .roam else {
                settle(mood)
                return
            }
            if action != .walk {
                pause -= dt
                if pause <= 0 {
                    action = .walk
                    facing = random() < 0.5 ? -1 : 1
                }
                return
            }
            walk(dt: dt, on: ground, ledges: ledges, walls: walls, screen: screen, random: random)
        }
    }

    private mutating func settle(_ mood: RoamMood) {
        switch mood {
        case .roam: action = .stand
        case .stay: action = .stand
        case .sleep: action = .sit
        }
    }

    private mutating func walk(
        dt: Double, on ground: Ledge, ledges: [Ledge], walls: [Wall], screen: Box, random: () -> Double
    ) {
        let next = x + facing * Self.walkSpeed * dt
        let front = x + facing * Self.halfWidth
        let nextFront = next + facing * Self.halfWidth
        // 앞을 막는 창 옆면 — 윗면에 설 자리가 있으면 닿을 만한 높이는 뛰어오르고 더 높으면 튜브를 놓아 넘어간다. 아니면 돌아선다.
        if let wall = walls.first(where: {
            $0.faces == -facing && $0.bottom <= y + 2 && $0.top > y + 10
                && (front - $0.x) * (nextFront - $0.x) <= 0
        }) {
            let landing = Point(x: wall.x + facing * Self.halfWidth * 2, y: wall.top)
            if let top = Self.ledge(at: landing, in: ledges) {
                if wall.top - y <= Self.jumpReach, random() < 0.7 {
                    x = wall.x - facing * Self.halfWidth
                    jump(to: landing)
                    return
                }
                if tube == nil, random() < 0.5 {
                    lay(toX: Self.anywhere(on: top, random), y: top.y, screen: screen)
                    return
                }
            }
            facing = -facing
            return
        }
        // 화면 가장자리 — 돌아선다.
        if nextFront <= screen.minX || nextFront >= screen.maxX {
            facing = -facing
            return
        }
        // 발판 끝 — 건너편 발판이 가까우면 건너뛰고, 아니면 가끔 뛰어내리고 대개는 돌아선다.
        if next < ground.minX || next > ground.maxX {
            if let landing = hop(from: ground, ledges: ledges), random() < 0.7 {
                jump(to: landing)
            } else if random() < Self.dropChance {
                x = next
                action = .fall
                vy = 0
            } else {
                facing = -facing
            }
            return
        }
        x = next
        // 가끔 튜브를 놓아 다른 곳으로 건너간다 — 한 번에 하나만. 바닥에서는 위쪽 창 윗면으로 자주 올라가고,
        // 올라갈 창이 없으면 화면을 크게 돌아 바닥의 먼 곳으로, 창 위에서는 멀리 있는 다른 창 윗면으로 건너간다.
        if tube == nil {
            let onFloor = ground.y <= screen.minY
            let tops = ledges.filter {
                $0 != ground && $0.y > screen.minY && $0.maxX - $0.minX >= Self.halfWidth * 2 + 20
            }
            if random() < dt * (onFloor && !tops.isEmpty ? Self.upRate : Self.tubeRate) {
                let choices = onFloor ? tops : tops.filter { abs(($0.minX + $0.maxX) / 2 - x) >= 150 }
                if !choices.isEmpty {
                    let target = choices[min(Int(random() * Double(choices.count)), choices.count - 1)]
                    lay(toX: Self.anywhere(on: target, random), y: target.y, screen: screen)
                    return
                }
                if onFloor {
                    let lo = ground.minX + Self.halfWidth, hi = ground.maxX - Self.halfWidth
                    let left = max(0, x - Self.crossing - lo), right = max(0, hi - x - Self.crossing)
                    if left + right > 0 {
                        let r = random() * (left + right)
                        let endX = r < left ? lo + r : x + Self.crossing + (r - left)
                        let share = Self.crossingRise.lowerBound
                            + (Self.crossingRise.upperBound - Self.crossingRise.lowerBound) * random()
                        lay(toX: endX, y: ground.y, screen: screen, rise: (screen.maxY - screen.minY) * share)
                        return
                    }
                }
            }
        }
        // 가끔 멈춰 숨을 돌린다 — 초당 8% 쯤.
        if random() < dt * 0.08 {
            action = .stand
            pause = 1.5 + random() * 3
        }
    }

    /// 뛰기 시작한다 — 쪼그렸다가(`jumpCrouch`) 떠올라 `landing` 에 내려앉는다.
    private mutating func jump(to landing: Point) {
        jumpFrom = Point(x: x, y: y)
        jumpTo = landing
        jumpAge = 0
        action = .jump
    }

    /// 튜브를 놓기 시작한다 — 놓는 동안(`tubeBuild`) 그 자리에 서 있다가 관을 따라 건너간다.
    private mutating func lay(toX endX: Double, y endY: Double, screen: Box, rise: Double = 140) {
        tube = Tube(startX: x, startY: y, endX: endX, endY: endY, screen: screen, rise: rise)
        tubeAge = 0
        if endX != x { facing = endX > x ? 1 : -1 }
        action = .tube
    }

    /// 발판 끝 너머 `hopGap` 안에 있는 가장 가까운 발판의 내려설 자리 — 위아래로 `jumpReach` 안인 것만.
    /// 밑에 겹쳐 있는 발판은 떨어지면 닿고, 위에 겹쳐 있는 발판은 그 창 옆면이 앞을 막으니 여기서는 보지 않는다.
    private func hop(from ground: Ledge, ledges: [Ledge]) -> Point? {
        let edge = facing > 0 ? ground.maxX : ground.minX
        return ledges.compactMap { ledge -> (gap: Double, landing: Point)? in
            guard ledge != ground, ledge.maxX - ledge.minX >= Self.halfWidth * 2 + 20,
                  abs(ledge.y - y) <= Self.jumpReach
            else { return nil }
            let near = facing > 0 ? ledge.minX : ledge.maxX
            let gap = (near - edge) * facing
            guard gap >= 0, gap <= Self.hopGap else { return nil }
            return (gap, Point(x: near + facing * Self.halfWidth * 2, y: ledge.y))
        }.min { $0.gap < $1.gap }?.landing
    }

    /// `point` 를 딛고 설 수 있는 발판.
    private static func ledge(at point: Point, in ledges: [Ledge]) -> Ledge? {
        ledges.first { abs($0.y - point.y) <= 0.5 && point.x >= $0.minX && point.x <= $0.maxX }
    }

    /// 발판 위 아무 자리 — 몸이 걸치지 않게 양 끝에서 반폭만큼 안쪽.
    private static func anywhere(on ledge: Ledge, _ random: () -> Double) -> Double {
        ledge.minX + halfWidth + random() * max(0, ledge.maxX - ledge.minX - halfWidth * 2)
    }
}
