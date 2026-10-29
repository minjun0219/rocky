// 돌아다니는 로키 — 창 윗면을 걷고, 발밑이 사라지면 떨어지고, 벽(화면 가장자리·창 옆면)을 기어오른다.
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

/// 기어오를 수 있는 세로줄 — 창의 옆면. `faces` 는 벽이 바라보는 쪽(-1 = 왼쪽 옆면, +1 = 오른쪽 옆면).
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
    case walk, stand, sit, fall, climb
}

/// 로키의 몸 — `step` 이 한 프레임씩 움직인다. 무엇을 할지 고르는 운은 `random`(0..<1)으로 받는다.
public struct Roamer: Equatable, Sendable {
    public static let walkSpeed: Double = 45
    public static let climbSpeed: Double = 35
    public static let gravity: Double = 1800
    /// 몸 반폭 — 벽에 붙거나 화면 끝에서 돌아설 때 쓴다.
    public static let halfWidth: Double = 28

    public var x: Double
    public var y: Double
    /// 바라보는 쪽(+1 오른쪽, -1 왼쪽).
    public var facing: Double = 1
    public var action: RoamAction = .fall
    public var vy: Double = 0
    /// 서 있거나 앉아 있을 남은 시간 — 돌아다닐 때만 쓴다.
    var pause: Double = 0
    /// 기어오르는 벽의 꼭대기와, 그게 화면 가장자리인지(그러면 꼭대기에서 뛰어내린다).
    var climbTop: Double = 0
    var climbWall: Double = 0
    var climbingScreen = false

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
    }

    public mutating func step(
        dt: Double, ledges: [Ledge], walls: [Wall], screen: Box, mood: RoamMood, random: () -> Double
    ) {
        switch action {
        case .fall:
            vy -= Self.gravity * dt
            let next = y + vy * dt
            // 지나쳐 내려간 발판 중 가장 높은 것에 내려앉는다.
            let landing = ledges
                .filter { $0.y <= y + 0.5 && $0.y >= next && x >= $0.minX && x <= $0.maxX }
                .max { $0.y < $1.y }
            if let landing {
                y = landing.y
                vy = 0
                settle(mood)
                pause = 0.5
            } else {
                y = max(next, screen.minY)
            }
        case .climb:
            guard mood == .roam else {
                action = .fall
                return
            }
            // 붙어 있던 창이 사라지거나 옮겨 가면 떨어진다.
            if !climbingScreen, !walls.contains(where: { $0.x == climbWall && $0.top == climbTop }) {
                action = .fall
                vy = 0
                return
            }
            y += Self.climbSpeed * dt
            if y >= climbTop {
                y = climbTop
                if climbingScreen {
                    // 화면 꼭대기 — 돌아서서 뛰어내린다.
                    facing = -facing
                    x += facing * Self.halfWidth
                    action = .fall
                } else {
                    // 창 윗면으로 올라선다.
                    x += facing * Self.halfWidth * 2
                    action = .walk
                }
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
            walk(dt: dt, on: ground, walls: walls, screen: screen, random: random)
        }
    }

    private mutating func settle(_ mood: RoamMood) {
        switch mood {
        case .roam: action = .stand
        case .stay: action = .stand
        case .sleep: action = .sit
        }
    }

    private mutating func walk(dt: Double, on ground: Ledge, walls: [Wall], screen: Box, random: () -> Double) {
        let next = x + facing * Self.walkSpeed * dt
        let front = x + facing * Self.halfWidth
        let nextFront = next + facing * Self.halfWidth
        // 앞을 막는 창 옆면 — 기어오르거나 돌아선다.
        if let wall = walls.first(where: {
            $0.faces == -facing && $0.bottom <= y + 2 && $0.top > y + 10
                && (front - $0.x) * (nextFront - $0.x) <= 0
        }) {
            if random() < 0.6 {
                x = wall.x - facing * Self.halfWidth
                climbTop = wall.top
                climbWall = wall.x
                climbingScreen = false
                action = .climb
            } else {
                facing = -facing
            }
            return
        }
        // 화면 가장자리 — 가끔 기어오르고, 아니면 돌아선다.
        if nextFront <= screen.minX || nextFront >= screen.maxX {
            if random() < 0.3 {
                climbTop = screen.maxY - 90
                climbingScreen = true
                action = .climb
            } else {
                facing = -facing
            }
            return
        }
        // 발판 끝 — 뛰어내리거나 돌아선다.
        if next < ground.minX || next > ground.maxX {
            if random() < 0.5 {
                x = next
                action = .fall
                vy = 0
            } else {
                facing = -facing
            }
            return
        }
        x = next
        // 가끔 멈춰 숨을 돌린다 — 초당 8% 쯤.
        if random() < dt * 0.08 {
            action = .stand
            pause = 1.5 + random() * 3
        }
    }
}
