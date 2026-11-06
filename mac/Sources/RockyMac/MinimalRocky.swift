// 미니멀 벡터 로키 — 시안을 자른 조각(`RockyArt`) 대신 도형 열몇 개(돔 몸통 · 캡슐 팔다리 · 금 두 줄 · 청록 점 셋 · 점 눈 · 선 입)로 그린다.
// 오너가 원작 질감 대신 단순한 그림체를 비교해 보려고 고른 것이다(2026-11-06, 시안은 프로젝트 파일 mac-avatar/simple-style/minimal-vector.svg).
// 자세(`RockyPose`)는 원작과 같은 것을 받고, 칸 좌표도 원작 조각과 같다 — 몸통 가운데 (258, 160), 발바닥 y 298.75,
// 엉덩이 (164, 236) · (352, 236), 어깨 (100, 176) · (415.5, 176). 그래서 걷기 · 발 디딤 · 기울기가 그대로 맞는다.
import SwiftUI

enum MinimalRocky {
    private static let skin = Color(red: 0xd9 / 255, green: 0xa8 / 255, blue: 0x6c / 255)
    private static let ink = Color(red: 0x4a / 255, green: 0x2a / 255, blue: 0x1c / 255)
    private static let band = Color(red: 0xc0 / 255, green: 0x8a / 255, blue: 0x52 / 255)
    private static let teal = Color(red: 0x2f / 255, green: 0x9a / 255, blue: 0x8a / 255)
    private static let tongue = Color(red: 0xe0 / 255, green: 0x70 / 255, blue: 0x7a / 255)

    private static let hips = (left: CGPoint(x: 164, y: 236), right: CGPoint(x: 352, y: 236))
    private static let shoulders = (left: CGPoint(x: 100, y: 176), right: CGPoint(x: 415.5, y: 176))
    /// 원작 조각의 내린 팔 각도 — `raise` 0 이 이만큼 몸 밖으로 벌어진 팔이다.
    private static let rest = 37.33
    private static let line = 8.0
    private static let limbLine = 7.0

    /// 통째 자세(잠 · 갸웃 · 뛰는 중)는 그릴 조각이 없으니 관절 자세로 바꾼다 — 원작 시안의 그 자세를 흉내 낸다.
    static func joints(_ q: RockyPose) -> RockyPose {
        guard let whole = q.whole else { return q }
        var m = q
        m.whole = nil
        switch whole {
        case .jump:
            // 두 손을 펴 치켜들고 다리를 벌린다.
            m.face = .joy
            m.armL = RockyPose.Arm(raise: 115, hand: .open)
            m.armR = RockyPose.Arm(raise: 100, hand: .open)
            m.legL = 25
            m.legR = -25
        case .puzzled:
            // 오른손 주먹을 머리에 대고 눈썹을 모은 채 갸웃.
            m.face = .focus
            m.look = -0.4
            m.armL = RockyPose.Arm(raise: -5)
            m.armR = RockyPose.Arm(raise: 165, hand: .curl)
            m.tilt -= 5
        case .sleep:
            // 눈을 감고 납작 엎드려 팔다리를 옆으로 뻗는다.
            m.blink = true
            m.face = .smile
            m.squash *= 0.8
            m.armL = RockyPose.Arm(raise: 55)
            m.armR = RockyPose.Arm(raise: 55)
            m.legL = 72
            m.legR = -72
        }
        return m
    }

    /// 뒤에서 앞으로 — 다리, 팔, 몸통, 무늬, 얼굴. `gc` 는 원작 조각과 같은 칸 좌표로 맞춰 둔 것.
    static func draw(_ q: RockyPose, in gc: GraphicsContext) {
        leg(gc, hip: hips.left, angle: q.legL, step: q.stepL)
        leg(gc, hip: hips.right, angle: q.legR, step: q.stepR)
        var upper = gc
        upper.translateBy(x: 258, y: 236 + q.rise)
        upper.rotate(by: .degrees(q.lean))
        upper.translateBy(x: -258, y: -236)
        arm(upper, left: true, q.armL)
        arm(upper, left: false, q.armR)
        body(upper)
        face(upper, q)
    }

    private static func outlined(_ gc: GraphicsContext, _ path: Path, fill: Color, width: Double) {
        gc.fill(path, with: .color(fill))
        gc.stroke(path, with: .color(ink), style: StrokeStyle(lineWidth: width, lineJoin: .round))
    }

    private static func capsule(_ rect: CGRect) -> Path {
        Path(roundedRect: rect, cornerRadius: min(rect.width, rect.height) / 2)
    }

    private static func leg(_ gc: GraphicsContext, hip: CGPoint, angle: Double, step: CGSize) {
        var g = gc
        g.translateBy(x: hip.x + step.width, y: hip.y + step.height)
        g.rotate(by: .degrees(angle))
        outlined(g, capsule(CGRect(x: -26, y: 8, width: 52, height: 55)), fill: skin, width: limbLine)
    }

    /// 팔 — 어깨에서 똑바로 내린 캡슐을 몸 밖으로 `rest + raise` 도 돌린다. 왼팔은 시계 방향(+), 오른팔은 반대로 돌려야 들린다.
    /// 손 모양이 바뀌는 동안은 원작처럼 옛 손과 새 손을 겹쳐 바꾼다.
    private static func arm(_ gc: GraphicsContext, left: Bool, _ arm: RockyPose.Arm) {
        guard let from = arm.from, from != arm.hand, arm.fade < 1 else { return paintArm(gc, left: left, arm.hand, arm.raise) }
        var old = gc, new = gc
        old.opacity = min(1, 2 * (1 - arm.fade))
        new.opacity = min(1, 2 * arm.fade)
        paintArm(old, left: left, from, arm.raise)
        paintArm(new, left: left, arm.hand, arm.raise)
    }

    private static func paintArm(_ gc: GraphicsContext, left: Bool, _ hand: RockyPose.Hand, _ raise: Double) {
        var g = gc
        let shoulder = left ? shoulders.left : shoulders.right
        let turn = rest + raise
        g.translateBy(x: shoulder.x, y: shoulder.y)
        g.rotate(by: .degrees(left ? turn : -turn))
        switch hand {
        case .rest:
            outlined(g, capsule(CGRect(x: -25, y: -20, width: 50, height: 120)), fill: skin, width: limbLine)
        case .open:
            // 편 손 — 끝에 손가락 셋(엄지는 없다).
            for x in [-17.0, 0, 17] {
                let y = x == 0 ? 112.0 : 104.0
                outlined(g, Path(ellipseIn: CGRect(x: x - 12, y: y - 12, width: 24, height: 24)), fill: skin, width: limbLine)
            }
            outlined(g, capsule(CGRect(x: -25, y: -20, width: 50, height: 124)), fill: skin, width: limbLine)
        case .curl:
            // 주먹 — 짧은 팔 끝에 둥근 주먹.
            outlined(g, capsule(CGRect(x: -25, y: -20, width: 50, height: 100)), fill: skin, width: limbLine)
            outlined(g, Path(ellipseIn: CGRect(x: -30, y: 62, width: 60, height: 56)), fill: skin, width: limbLine)
        }
    }

    /// 낮은 돔 몸통과 무늬 — 아랫배 띠, 금 두 줄, 청록 점 셋.
    private static func body(_ gc: GraphicsContext) {
        var dome = Path()
        dome.move(to: CGPoint(x: 96, y: 226))
        dome.addQuadCurve(to: CGPoint(x: 258, y: 40), control: CGPoint(x: 96, y: 40))
        dome.addQuadCurve(to: CGPoint(x: 420, y: 226), control: CGPoint(x: 420, y: 40))
        dome.addQuadCurve(to: CGPoint(x: 258, y: 254), control: CGPoint(x: 420, y: 254))
        dome.addQuadCurve(to: CGPoint(x: 96, y: 226), control: CGPoint(x: 96, y: 254))
        dome.closeSubpath()
        outlined(gc, dome, fill: skin, width: line)

        var belly = Path()
        belly.move(to: CGPoint(x: 124, y: 226))
        belly.addQuadCurve(to: CGPoint(x: 394, y: 226), control: CGPoint(x: 258, y: 246))
        gc.stroke(belly, with: .color(band.opacity(0.6)), style: StrokeStyle(lineWidth: 11, lineCap: .round))

        let crack = StrokeStyle(lineWidth: 5.5, lineCap: .round, lineJoin: .round)
        gc.stroke(zigzag(from: CGPoint(x: 188, y: 75), [(20, 25), (-11, 20), (25, 11)]), with: .color(ink), style: crack)
        gc.stroke(zigzag(from: CGPoint(x: 300, y: 72), [(-14, 31), (17, 8), (-20, 25)]), with: .color(ink), style: crack)

        for (x, y, r): (Double, Double, Double) in [(339, 100, 14), (370, 125, 8.5), (149, 139, 10)] {
            outlined(gc, Path(ellipseIn: CGRect(x: x - r, y: y - r, width: 2 * r, height: 2 * r)), fill: teal, width: r > 9 ? 5.5 : 4.2)
        }
    }

    private static func zigzag(from start: CGPoint, _ steps: [(Double, Double)]) -> Path {
        var path = Path()
        var at = start
        path.move(to: at)
        for (dx, dy) in steps {
            at = CGPoint(x: at.x + dx, y: at.y + dy)
            path.addLine(to: at)
        }
        return path
    }

    /// 얼굴 — 점 눈(하이라이트 하나)과 선 입. 눈은 보는 쪽(`look`)으로 5 옮긴다.
    private static func face(_ gc: GraphicsContext, _ q: RockyPose) {
        let shift = 5 * q.look
        let eyes = [CGPoint(x: 216 + shift, y: 160), CGPoint(x: 300 + shift, y: 160)]
        let pen = StrokeStyle(lineWidth: 6, lineCap: .round, lineJoin: .round)
        for (i, eye) in eyes.enumerated() {
            if q.blink {
                // 감은 눈 — 아래로 둥근 선.
                var lid = Path()
                lid.move(to: CGPoint(x: eye.x - 16, y: eye.y))
                lid.addQuadCurve(to: CGPoint(x: eye.x + 16, y: eye.y), control: CGPoint(x: eye.x, y: eye.y + 13))
                gc.stroke(lid, with: .color(ink), style: pen)
            } else if q.face == .happy || q.face == .joy {
                // 웃는 눈 — 위로 둥근 선.
                var arc = Path()
                arc.move(to: CGPoint(x: eye.x - 15, y: eye.y + 5))
                arc.addQuadCurve(to: CGPoint(x: eye.x + 15, y: eye.y + 5), control: CGPoint(x: eye.x, y: eye.y - 14))
                gc.stroke(arc, with: .color(ink), style: pen)
            } else {
                gc.fill(Path(ellipseIn: CGRect(x: eye.x - 18, y: eye.y - 18, width: 36, height: 36)), with: .color(ink))
                gc.fill(Path(ellipseIn: CGRect(x: eye.x + 0.5, y: eye.y - 12.5, width: 11, height: 11)), with: .color(.white))
            }
            if q.face == .focus {
                // 모은 눈썹 — 안쪽 끝이 내려간다.
                let out = i == 0 ? -1.0 : 1.0
                var brow = Path()
                brow.move(to: CGPoint(x: eye.x + 20 * out, y: eye.y - 34))
                brow.addLine(to: CGPoint(x: eye.x - 10 * out, y: eye.y - 27))
                gc.stroke(brow, with: .color(ink), style: pen)
            }
        }
        let mouth = CGPoint(x: 258 + shift, y: 188)
        var path = Path()
        switch q.face {
        case .joy:
            path.move(to: CGPoint(x: mouth.x - 20, y: mouth.y - 4))
            path.addQuadCurve(to: CGPoint(x: mouth.x + 20, y: mouth.y - 4), control: CGPoint(x: mouth.x, y: mouth.y + 28))
            path.closeSubpath()
            outlined(gc, path, fill: tongue, width: 5.5)
        case .surprise:
            gc.fill(Path(ellipseIn: CGRect(x: mouth.x - 9, y: mouth.y - 9, width: 18, height: 22)), with: .color(ink))
        case .focus:
            path.move(to: CGPoint(x: mouth.x - 11, y: mouth.y + 2))
            path.addLine(to: CGPoint(x: mouth.x + 11, y: mouth.y + 2))
            gc.stroke(path, with: .color(ink), style: pen)
        case .smile, .happy:
            path.move(to: CGPoint(x: mouth.x - 14, y: mouth.y))
            path.addQuadCurve(to: CGPoint(x: mouth.x + 14, y: mouth.y), control: CGPoint(x: mouth.x, y: mouth.y + 14))
            gc.stroke(path, with: .color(ink), style: pen)
        }
    }
}
