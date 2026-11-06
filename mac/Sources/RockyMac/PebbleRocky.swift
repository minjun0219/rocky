// 돌멩이 로키 — 원작 느낌(사암색 돔 · 금 · 청록 점)을 다 뺀 각진 회색 바윗돌 캐릭터. 오너가 미니멀 벡터 다음으로 하나 더 보자고 했다
// (2026-11-06 "원작 느낌 없이 돌맹이 캐릭터로", 자세 시트와 SVG 사본은 프로젝트 파일 mac-avatar/simple-style/pebble-*).
// 몸통 · 팔다리 · 얼굴만 다르고 자세(`RockyPose`)와 칸 좌표는 원작 조각 · `MinimalRocky` 와 같다 — 통째 자세도 `MinimalRocky.joints` 로 바꿔 받는다.
import SwiftUI

enum PebbleRocky {
    private static let stone = Color(red: 0xae / 255, green: 0xb5 / 255, blue: 0xbd / 255)
    private static let ink = Color(red: 0x38 / 255, green: 0x40 / 255, blue: 0x4a / 255)
    private static let cheek = Color(red: 0xf2 / 255, green: 0xa0 / 255, blue: 0xa6 / 255)
    private static let tongue = Color(red: 0xe0 / 255, green: 0x70 / 255, blue: 0x7a / 255)

    /// 엉덩이와 어깨 — 몸통이 둥글어 원작보다 조금 안쪽 · 아래에 붙는다. 발바닥(y 299)은 원작과 같다.
    private static let hips = (left: CGPoint(x: 170, y: 236), right: CGPoint(x: 346, y: 236))
    private static let shoulders = (left: CGPoint(x: 112, y: 190), right: CGPoint(x: 404, y: 190))
    private static let rest = 37.33

    /// 자는 자세 — 원작처럼 엎드려 팔을 뻗지 않고 팔다리를 다 넣은 채 바닥에 내려앉아 눈을 감는다(2026-11-06 오너가 원래 잠 자세가 이상하다고 해서 그린 후보 셋 중 하나,
    /// 후보는 프로젝트 파일 mac-avatar/simple-style/rock-sleep-options.png). 숨에 맞춘 눌림(`squash`)은 그대로 받는다.
    static func asleep(_ q: RockyPose) -> RockyPose {
        var m = q
        m.whole = nil
        m.blink = true
        m.face = .smile
        // 바위 아래 끝(칸 y 252)이 발바닥(298.75)에 닿게 — 무대에는 `RockyArt.scale`(0.6)배로 올라간다.
        m.y += (298.75 - 252) * 0.6
        return m
    }

    /// `tucked` 면 팔다리를 그리지 않는다(잘 때).
    static func draw(_ q: RockyPose, tucked: Bool = false, in gc: GraphicsContext) {
        if !tucked {
            leg(gc, hip: hips.left, angle: q.legL, step: q.stepL)
            leg(gc, hip: hips.right, angle: q.legR, step: q.stepR)
        }
        var upper = gc
        upper.translateBy(x: 258, y: 236 + q.rise)
        upper.rotate(by: .degrees(q.lean))
        upper.translateBy(x: -258, y: -236)
        if !tucked {
            arm(upper, left: true, q.armL)
            arm(upper, left: false, q.armR)
        }
        body(upper)
        face(upper, q)
    }

    private static func outlined(_ gc: GraphicsContext, _ path: Path, width: Double = 7) {
        gc.fill(path, with: .color(stone))
        gc.stroke(path, with: .color(ink), style: StrokeStyle(lineWidth: width, lineJoin: .round))
    }

    private static func capsule(_ rect: CGRect) -> Path {
        Path(roundedRect: rect, cornerRadius: min(rect.width, rect.height) / 2)
    }

    private static func dot(_ x: Double, _ y: Double, _ r: Double) -> Path {
        Path(ellipseIn: CGRect(x: x - r, y: y - r, width: 2 * r, height: 2 * r))
    }

    private static func leg(_ gc: GraphicsContext, hip: CGPoint, angle: Double, step: CGSize) {
        var g = gc
        g.translateBy(x: hip.x + step.width, y: hip.y + step.height)
        g.rotate(by: .degrees(angle))
        outlined(g, capsule(CGRect(x: -24, y: 6, width: 48, height: 57)))
    }

    private static func arm(_ gc: GraphicsContext, left: Bool, _ arm: RockyPose.Arm) {
        guard let from = arm.from, from != arm.hand, arm.fade < 1 else { return paintArm(gc, left: left, arm.hand, arm.raise) }
        var old = gc, new = gc
        old.opacity = min(1, 2 * (1 - arm.fade))
        new.opacity = min(1, 2 * arm.fade)
        paintArm(old, left: left, from, arm.raise)
        paintArm(new, left: left, arm.hand, arm.raise)
    }

    /// 짧은 팔 — 편 손은 끝에 손가락 셋, 주먹은 끝에 동그란 주먹.
    private static func paintArm(_ gc: GraphicsContext, left: Bool, _ hand: RockyPose.Hand, _ raise: Double) {
        var g = gc
        let shoulder = left ? shoulders.left : shoulders.right
        let turn = rest + raise
        g.translateBy(x: shoulder.x, y: shoulder.y)
        g.rotate(by: .degrees(left ? turn : -turn))
        if hand == .open {
            for x in [-14.0, 0, 14] { outlined(g, dot(x, x == 0 ? 94 : 87, 11)) }
        }
        outlined(g, capsule(CGRect(x: -22, y: -14, width: 44, height: 104)))
        if hand == .curl { outlined(g, dot(0, 86, 26)) }
    }

    /// 모서리가 각진 바윗돌 — 아래 그늘 한 줄과 왼쪽 위 반짝임 둘.
    /// 처음엔 위가 볼록한 돔이었는데 오너가 "응가같은 모양"이라 해서 후보 넷(납작 조약돌 · 각진 바위 · 공 · 모찌) 중 각진 바위를 골랐다
    /// (2026-11-06, 후보는 프로젝트 파일 mac-avatar/simple-style/pebble-shapes.png).
    private static func body(_ gc: GraphicsContext) {
        let corners: [CGPoint] = [
            CGPoint(x: 122, y: 252), CGPoint(x: 96, y: 184), CGPoint(x: 136, y: 104), CGPoint(x: 226, y: 70),
            CGPoint(x: 326, y: 78), CGPoint(x: 398, y: 118), CGPoint(x: 424, y: 196), CGPoint(x: 398, y: 252),
        ]
        var rock = Path()
        rock.addLines(corners)
        rock.closeSubpath()
        outlined(gc, rock, width: 8)

        var shine = gc
        shine.translateBy(x: 196, y: 104)
        shine.rotate(by: .degrees(-20))
        shine.fill(Path(ellipseIn: CGRect(x: -34, y: -15, width: 68, height: 30)), with: .color(.white.opacity(0.55)))
        gc.fill(dot(242, 92, 5.5), with: .color(.white.opacity(0.55)))
    }

    /// 얼굴 — 세로로 긴 점 눈, 볼터치, 고양이 입(ω). 눈은 보는 쪽(`look`)으로 5 옮긴다.
    private static func face(_ gc: GraphicsContext, _ q: RockyPose) {
        let shift = 5 * q.look
        let pen = StrokeStyle(lineWidth: 6, lineCap: .round, lineJoin: .round)
        for (i, x) in [214.0, 302].enumerated() {
            let eye = CGPoint(x: x + shift, y: 166)
            let out = i == 0 ? -1.0 : 1.0
            gc.fill(
                Path(ellipseIn: CGRect(x: eye.x + 4 * out - 17, y: eye.y + 18, width: 34, height: 20)),
                with: .color(cheek.opacity(0.8))
            )
            if q.blink {
                var lid = Path()
                lid.move(to: CGPoint(x: eye.x - 15, y: eye.y))
                lid.addQuadCurve(to: CGPoint(x: eye.x + 15, y: eye.y), control: CGPoint(x: eye.x, y: eye.y + 12))
                gc.stroke(lid, with: .color(ink), style: pen)
            } else if q.face == .happy || q.face == .joy {
                var arc = Path()
                arc.move(to: CGPoint(x: eye.x - 14, y: eye.y + 5))
                arc.addQuadCurve(to: CGPoint(x: eye.x + 14, y: eye.y + 5), control: CGPoint(x: eye.x, y: eye.y - 13))
                gc.stroke(arc, with: .color(ink), style: pen)
            } else {
                gc.fill(Path(ellipseIn: CGRect(x: eye.x - 11, y: eye.y - 15, width: 22, height: 30)), with: .color(ink))
                gc.fill(dot(eye.x + 3, eye.y - 6, 4), with: .color(.white))
            }
            if q.face == .focus {
                var brow = Path()
                brow.move(to: CGPoint(x: eye.x + 16 * out, y: eye.y - 30))
                brow.addLine(to: CGPoint(x: eye.x - 8 * out, y: eye.y - 24))
                gc.stroke(brow, with: .color(ink), style: pen)
            }
        }
        let mouth = CGPoint(x: 258 + shift, y: 194)
        var path = Path()
        switch q.face {
        case .joy:
            path.move(to: CGPoint(x: mouth.x - 16, y: mouth.y - 4))
            path.addQuadCurve(to: CGPoint(x: mouth.x + 16, y: mouth.y - 4), control: CGPoint(x: mouth.x, y: mouth.y + 22))
            path.closeSubpath()
            gc.fill(path, with: .color(tongue))
            gc.stroke(path, with: .color(ink), style: StrokeStyle(lineWidth: 5.5, lineJoin: .round))
        case .surprise:
            gc.fill(Path(ellipseIn: CGRect(x: mouth.x - 8, y: mouth.y - 8, width: 16, height: 20)), with: .color(ink))
        case .focus:
            path.move(to: CGPoint(x: mouth.x - 9, y: mouth.y + 2))
            path.addLine(to: CGPoint(x: mouth.x + 9, y: mouth.y + 2))
            gc.stroke(path, with: .color(ink), style: pen)
        case .smile, .happy:
            path.move(to: CGPoint(x: mouth.x - 11, y: mouth.y))
            path.addQuadCurve(to: CGPoint(x: mouth.x, y: mouth.y), control: CGPoint(x: mouth.x - 5.5, y: mouth.y + 8))
            path.addQuadCurve(to: CGPoint(x: mouth.x + 11, y: mouth.y), control: CGPoint(x: mouth.x + 5.5, y: mouth.y + 8))
            gc.stroke(path, with: .color(ink), style: pen)
        }
    }
}
