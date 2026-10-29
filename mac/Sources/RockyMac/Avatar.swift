// 떠 있는 아바타 — 화면 구석의 작은 로키(프로젝트 헤일메리의 에리디언, 돌 골렘)가 에이전트 상태를 알린다.
// 내 차례인 세션이 있으면 껑충 뛰고 손을 흔들며 말풍선에 기다리는 것을 띄우고, 도는 세션이 있으면 팔을 들어 내리치고, 다 쉬면 주저앉아 잠든다.
// 창은 늘 위에 뜨는 패널이고 포커스를 가져가지 않는다. 끌어서 옮기면 자리를 기억한다.
import AppKit
import RockyKit
import SwiftUI

/// 아바타 패널을 띄우고 걷는다 — `AgentStore.showAvatar` 를 따른다.
@MainActor
final class AvatarController {
    private var panel: NSPanel?
    private let agents: AgentStore

    init(agents: AgentStore) {
        self.agents = agents
        // App.init 은 NSApplication 이 다 뜨기 전에 돈다 — 패널은 다음 바퀴에 만든다.
        Task { @MainActor in self.follow() }
    }

    /// 켜고 끄는 것을 따라간다 — Observation 은 한 번 알리므로 바뀔 때마다 다시 건다.
    private func follow() {
        withObservationTracking {
            _ = agents.showAvatar
        } onChange: { [weak self] in
            Task { @MainActor in self?.follow() }
        }
        agents.showAvatar ? show() : hide()
    }

    private func show() {
        if panel == nil {
            let panel = NSPanel(
                contentRect: NSRect(x: 0, y: 0, width: 240, height: 170),
                styleMask: [.borderless, .nonactivatingPanel],
                backing: .buffered,
                defer: false
            )
            panel.level = .floating
            panel.isFloatingPanel = true
            panel.hidesOnDeactivate = false
            panel.backgroundColor = .clear
            panel.isOpaque = false
            panel.hasShadow = false
            panel.isMovableByWindowBackground = true
            panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
            panel.contentView = NSHostingView(rootView: AvatarView(agents: agents))
            if !panel.setFrameUsingName("RockyAvatar"), let screen = NSScreen.main?.visibleFrame {
                // 처음엔 오른쪽 아래 — 독·메뉴 막대를 피해 보이는 영역 안에.
                panel.setFrameOrigin(NSPoint(x: screen.maxX - 260, y: screen.minY + 20))
            }
            panel.setFrameAutosaveName("RockyAvatar")
            self.panel = panel
        }
        panel?.orderFrontRegardless()
    }

    private func hide() {
        panel?.orderOut(nil)
    }
}

struct AvatarView: View {
    let agents: AgentStore
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            if let bubble = bubble(agents.mood) {
                Text(bubble)
                    .font(.caption)
                    .lineLimit(3)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
                    .frame(maxWidth: 220, alignment: .trailing)
                    .transition(.opacity)
            }
            Rocky(mood: agents.mood)
                .frame(width: 96, height: 88)
                .help(agents.mood.summary)
        }
        .frame(width: 240, height: 170, alignment: .bottomTrailing)
        .onTapGesture {
            if let url = agents.agentsURL { openURL(url) }
        }
        .contextMenu {
            Button("에이전트 탭 열기") {
                if let url = agents.agentsURL { openURL(url) }
            }
            Button("아바타 숨기기") { agents.showAvatar = false }
        }
        .animation(.easeInOut(duration: 0.3), value: agents.mood)
    }

    /// 말풍선 — 내 차례일 때만 늘 띄우고, 나머지는 조용히 둔다(마우스를 올리면 `help` 로 보인다).
    private func bubble(_ mood: AvatarMood) -> String? {
        guard case let .waiting(name, line, count) = mood else { return nil }
        let more = count > 1 ? " 외 \(count - 1)개" : ""
        return "\(name)\(more) — \(line ?? "답을 기다려요")"
    }
}

extension AvatarMood {
    /// 마우스를 올렸을 때의 한 줄.
    var summary: String {
        switch self {
        case .unknown: "데몬에 못 붙었거나 세션 목록을 못 얻었어요"
        case let .waiting(_, _, count): "내 차례인 세션 \(count)개"
        case let .working(count): "도는 세션 \(count)개"
        case .resting: "다 쉬고 있어요"
        }
    }
}

/// 로키 — 조각 그림(Gemini 가 그린 돌 골렘)을 관절로 이어 움직이는 컷아웃 리그.
/// 조각마다 자기 관절(pivot)과 부모 위의 붙는 자리(at)가 있고, 기분마다 관절 각도가 시간에 따라 바뀐다.
private struct Rocky: View {
    let mood: AvatarMood

    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 30, paused: !moving)) { context in
            let t = context.date.timeIntervalSinceReferenceDate
            Canvas { gc, size in
                draw(in: &gc, size: size, t: t)
            }
        }
    }

    /// 쉬는 중에도 숨은 쉰다 — 멈추는 건 연결이 없을 때뿐.
    private var moving: Bool {
        if case .unknown = mood { return false }
        return true
    }

    private func draw(in gc: inout GraphicsContext, size: CGSize, t: Double) {
        // 무대는 360×330 — 프레임에 맞춰 늘린다.
        gc.scaleBy(x: size.width / 360, y: size.height / 330)
        gc.fill(
            Path(ellipseIn: CGRect(x: 180 - 95, y: 312 - 9, width: 190, height: 18)),
            with: .color(.black.opacity(0.13))
        )
        guard let art = RockyArt.shared else {
            // 그림을 못 읽으면 돌 하나라도.
            gc.fill(Path(ellipseIn: CGRect(x: 110, y: 150, width: 140, height: 120)), with: .color(.gray))
            return
        }
        let q = RigPose(mood, t: t)
        if q.gray { gc.addFilter(.grayscale(1)) }

        // 몸통부터 — 나머지 조각은 부모의 행렬에서 이어진다.
        let root = CGAffineTransform(translationX: 180, y: 190 + q.y * 0.55)
            .scaledBy(x: RockyArt.stage, y: RockyArt.stage)
            .rotated(by: q.angle(.torso) * .pi / 180)
        var world: [RigPart: CGAffineTransform] = [:]
        world[.torso] = root
            .scaledBy(x: RigPart.torso.scale, y: RigPart.torso.scale)
            .translatedBy(x: -RigPart.torso.pivot.x, y: -RigPart.torso.pivot.y)
        for part in RigPart.drawOrder where part != .torso {
            guard let attach = part.attach, let base = world[attach.0] else { continue }
            world[part] = base
                .translatedBy(x: attach.1.x, y: attach.1.y)
                .rotated(by: q.angle(part) * .pi / 180)
                .scaledBy(x: part.scale, y: part.scale)
                .translatedBy(x: -part.pivot.x, y: -part.pivot.y)
        }
        for part in RigPart.drawOrder {
            guard let m = world[part], let image = art.images[part] else { continue }
            var layer = gc
            layer.concatenate(m)
            layer.draw(image.image, in: CGRect(origin: .zero, size: image.size))
        }
        if case .resting = mood {
            let rise = (t.truncatingRemainder(dividingBy: 3)) / 3
            gc.draw(
                Text("z").font(.system(size: 40, weight: .semibold)).foregroundColor(.secondary.opacity(1 - rise)),
                at: CGPoint(x: 270 + rise * 20, y: 150 - rise * 60)
            )
        }
    }
}

/// 리그의 조각 — 좌표는 리소스 PNG 의 픽셀(원본 그림을 절반으로 줄인 것).
private enum RigPart: String, CaseIterable {
    case torso, upperL, foreL, upperR, foreR, legL, legR, mid

    /// 뒤에서 앞으로 — 팔뚝은 몸통 앞에 온다.
    static let drawOrder: [RigPart] = [.mid, .upperL, .upperR, .legL, .legR, .torso, .foreL, .foreR]

    /// 조각 안의 관절 — 이 점을 축으로 돈다.
    var pivot: CGPoint {
        switch self {
        case .torso: CGPoint(x: 51, y: 46.5)
        case .upperL: CGPoint(x: 119, y: 25)
        case .foreL: CGPoint(x: 24, y: 111)
        case .upperR: CGPoint(x: 11, y: 28)
        case .foreR: CGPoint(x: 22, y: 128)
        case .legL: CGPoint(x: 36, y: 11)
        case .legR: CGPoint(x: 33, y: 11)
        case .mid: CGPoint(x: 22, y: 8)
        }
    }

    /// 부모와, 부모 조각 안에서 붙는 자리 — 팔뚝은 위팔의 팔꿈치에.
    var attach: (RigPart, CGPoint)? {
        switch self {
        case .torso: nil
        case .upperL: (.torso, CGPoint(x: 15, y: 26))
        case .foreL: (.upperL, CGPoint(x: 8, y: 26))
        case .upperR: (.torso, CGPoint(x: 88, y: 25))
        case .foreR: (.upperR, CGPoint(x: 151, y: 28))
        case .legL: (.torso, CGPoint(x: 24, y: 75))
        case .legR: (.torso, CGPoint(x: 80, y: 75))
        case .mid: (.torso, CGPoint(x: 52, y: 84))
        }
    }

    var scale: CGFloat {
        switch self {
        case .torso: 1
        case .upperL: 0.6
        case .foreL, .foreR: 1.05
        case .upperR: 0.48
        case .legL, .legR: 0.62
        case .mid: 0.55
        }
    }
}

/// 한 순간의 자세 — 몸의 높이와 관절 각도(도).
private struct RigPose {
    var y: Double = 0
    var gray = false
    private var angles: [RigPart: Double] = [:]

    func angle(_ part: RigPart) -> Double { angles[part] ?? 0 }

    /// 껑충 — h 가 0(땅)에서 1(가장 높이)으로 갈수록 몸이 뜨고 팔을 치켜들고 다리를 벌린다.
    private static func hop(_ h: Double) -> (Double, [RigPart: Double]) {
        (-60 * h, [
            .upperL: 20 + 40 * h, .foreL: 10, .upperR: -(20 + 40 * h), .foreR: -10,
            .legL: 6 + 18 * h, .legR: -6 - 18 * h,
        ])
    }

    private static func blend(_ a: [RigPart: Double], _ b: [RigPart: Double], _ w: Double) -> [RigPart: Double] {
        var out: [RigPart: Double] = [:]
        for part in RigPart.allCases {
            let from = a[part] ?? 0
            out[part] = from + ((b[part] ?? 0) - from) * w
        }
        return out
    }

    init(_ mood: AvatarMood, t: Double) {
        switch mood {
        case .waiting:
            // 내 차례 — 두 번 껑충 뛰고, 몸을 흔들며 오른손을 크게 흔든다(2.4초 한 바퀴).
            let phase = t.truncatingRemainder(dividingBy: 2.4)
            if phase < 1.2 {
                (y, angles) = Self.hop(sin(.pi * phase.truncatingRemainder(dividingBy: 0.6) / 0.6))
            } else {
                let q = phase - 1.2
                let sway = sin(2 * .pi * q / 1.2)
                let wave: [RigPart: Double] = [
                    .torso: 8 * sway, .upperL: -60, .foreL: 170, .upperR: -65, .foreR: -5 + 35 * sin(t * 14),
                    .legL: 8 + 6 * sway, .legR: -8 + 6 * sway,
                ]
                // 뛰기와 흔들기 사이는 0.15초 동안 섞는다 — 팔이 튀지 않게.
                let w = min(max(0, min(1, q / 0.15)), max(0, min(1, (1.2 - q) / 0.15)))
                let (y0, from) = Self.hop(0)
                y = y0 + (-8 * abs(sway) - y0) * w
                angles = Self.blend(from, wave, w)
            }
        case .working:
            // 도는 중 — 오른팔을 천천히 들어 망치처럼 내리치고, 그 반동에 몸이 들썩인다(0.8초 한 바퀴).
            let phase = t.truncatingRemainder(dividingBy: 0.8) / 0.8
            var lift = 0.0
            var hit = 0.0
            if phase < 0.6 {
                let u = phase / 0.6
                lift = u * u * (3 - 2 * u)
            } else if phase < 0.7 {
                let u = (phase - 0.6) / 0.1
                lift = 1 - u * u
            } else {
                hit = exp(-(phase - 0.7) * 18)
            }
            y = -4 * lift + 5 * hit
            angles = [
                .torso: -4 * lift + 5 * hit, .upperL: -40, .foreL: 80, .upperR: 40 - 100 * lift, .foreR: -100 + 90 * lift,
                .legL: 10 + 4 * hit, .legR: -10 - 4 * hit,
            ]
        case .resting:
            // 다 쉼 — 주저앉아 팔로 몸을 감싸고 숨만 쉰다.
            y = 50 + sin(t * 1.6) * 2
            angles = [.torso: 4, .upperL: -70, .foreL: 120, .upperR: 70, .foreR: -120, .legL: 10, .legR: -10]
        case .unknown:
            // 연결 없음 — 회색으로 고개를 갸웃.
            gray = true
            angles = [.torso: -6, .upperL: -40, .foreL: -150, .upperR: -40, .foreR: -75, .legL: 12, .legR: -12]
        }
    }
}

/// 조각 그림 — .app 이면 `Contents/Resources/rocky/`, `swift run` 이면 SwiftPM 리소스 번들에서 읽는다.
/// `Bundle.module` 은 .app 안에서 번들을 못 찾으면 멈추므로 쓰지 않는다.
private final class RockyArt {
    struct Piece {
        let image: Image
        let size: CGSize
    }

    /// 리그 좌표를 무대 좌표로 — 원본(절반으로 줄이기 전)의 0.55 배.
    static let stage: CGFloat = 1.1

    static let shared: RockyArt? = RockyArt()

    let images: [RigPart: Piece]

    private init?() {
        let swiftpm = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("RockyMac_RockyMac.bundle")
        let bundles = [Bundle.main] + [swiftpm.flatMap { Bundle(url: $0) }].compactMap { $0 }
        var images: [RigPart: Piece] = [:]
        for part in RigPart.allCases {
            let url = bundles.lazy
                .compactMap { $0.url(forResource: part.rawValue, withExtension: "png", subdirectory: "rocky") }
                .first
            guard let url, let cg = NSImage(contentsOf: url)?.cgImage(forProposedRect: nil, context: nil, hints: nil)
            else { return nil }
            images[part] = Piece(
                image: Image(decorative: cg, scale: 1),
                size: CGSize(width: cg.width, height: cg.height)
            )
        }
        self.images = images
    }
}
