// 떠 있는 아바타 — 화면 구석의 작은 로키(프로젝트 헤일메리의 에리디언: 다섯 다리, 돌 같은 몸)가 에이전트 상태를 알린다.
// 내 차례인 세션이 있으면 깡충거리며 말풍선에 기다리는 것을 띄우고, 도는 세션이 있으면 다리를 두드리고, 다 쉬면 잠든다.
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
                contentRect: NSRect(x: 0, y: 0, width: 240, height: 150),
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
                .frame(width: 72, height: 60)
                .help(agents.mood.summary)
        }
        .frame(width: 240, height: 150, alignment: .bottomTrailing)
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

/// 로키 — 다섯 다리와 둥근 돌 몸. 기분마다 색과 움직임이 다르다.
private struct Rocky: View {
    let mood: AvatarMood

    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 20, paused: !moving)) { context in
            let t = context.date.timeIntervalSinceReferenceDate
            Canvas { gc, size in
                draw(in: &gc, size: size, t: t)
            }
        }
    }

    private var moving: Bool {
        switch mood {
        case .waiting, .working: true
        case .unknown, .resting: false
        }
    }

    private var shell: Color {
        switch mood {
        case .waiting: .orange
        case .working: Color(red: 0.55, green: 0.47, blue: 0.40)
        case .resting: Color(red: 0.45, green: 0.42, blue: 0.40)
        case .unknown: .gray
        }
    }

    private func draw(in gc: inout GraphicsContext, size: CGSize, t: Double) {
        let w = size.width
        let h = size.height
        // 내 차례면 깡충, 도는 중이면 살짝 들썩.
        let hop: Double = switch mood {
        case .waiting: abs(sin(t * 6)) * 6
        case .working: sin(t * 3) * 1.5
        default: 0
        }
        let center = CGPoint(x: w / 2, y: h * 0.45 - hop)
        let body = CGRect(x: center.x - w * 0.3, y: center.y - h * 0.28, width: w * 0.6, height: h * 0.5)

        // 다리 다섯 — 도는 중이면 하나씩 두드린다.
        for i in 0 ..< 5 {
            let angle = Double.pi * (0.15 + 0.175 * Double(i))
            var tap: Double = 0
            if case .working = mood, Int(t * 6) % 5 == i {
                tap = -4
            }
            let start = CGPoint(x: center.x + cos(angle) * w * 0.22, y: center.y + h * 0.12)
            let knee = CGPoint(x: center.x + cos(angle) * w * 0.42, y: center.y + h * 0.05 + tap)
            let foot = CGPoint(x: center.x + cos(angle) * w * 0.48, y: h * 0.95 - hop * 0.2 + tap)
            var leg = Path()
            leg.move(to: start)
            leg.addQuadCurve(to: foot, control: knee)
            gc.stroke(leg, with: .color(shell.opacity(0.9)), style: StrokeStyle(lineWidth: 3, lineCap: .round))
        }

        // 몸 — 돌 같은 둥근 덩어리와 윗면의 무늬.
        gc.fill(Path(ellipseIn: body), with: .color(shell))
        var ridge = Path()
        ridge.move(to: CGPoint(x: body.minX + body.width * 0.25, y: body.minY + body.height * 0.35))
        ridge.addQuadCurve(
            to: CGPoint(x: body.maxX - body.width * 0.25, y: body.minY + body.height * 0.35),
            control: CGPoint(x: center.x, y: body.minY + body.height * 0.1)
        )
        gc.stroke(ridge, with: .color(.black.opacity(0.25)), lineWidth: 1.5)

        // 로키에게는 눈이 없다 — 대신 기분을 몸 위의 기호로.
        let mark: String = switch mood {
        case .waiting: "!"
        case .working: "♪"
        case .resting: "z"
        case .unknown: "?"
        }
        gc.draw(
            Text(mark).font(.system(size: 14, weight: .bold)).foregroundColor(.white),
            at: CGPoint(x: center.x, y: center.y + 2)
        )
        if case .resting = mood {
            gc.draw(
                Text("z").font(.system(size: 10, weight: .semibold)).foregroundColor(.secondary),
                at: CGPoint(x: body.maxX + 2, y: body.minY - 2)
            )
        }
    }
}
