// 떠 있는 아바타 — 화면 구석의 작은 로키(프로젝트 헤일메리의 에리디언, 돌 골렘)가 에이전트 상태를 알린다.
// 내 차례인 세션이 있으면 껑충 뛰고 손을 흔들며 말풍선에 기다리는 것을 띄우고, 도는 세션이 있으면 팔을 들어 내리치고, 다 쉬면 주저앉아 잠든다.
// 창은 늘 위에 뜨는 패널이고 포커스를 가져가지 않는다. 돌아다니기를 켜면 창 윗면을 걷고, 끌어다 놓으면 떨어지고,
// 가끔 튜브를 놓아 다른 창으로 건너간다. 두 번 누르면 웹 에이전트 탭이 열리고, 말풍선은 누르면 닫힌다.
import AppKit
import RockyKit
import SwiftUI

/// 아바타 패널을 띄우고 걷는다 — `AgentStore.showAvatar` 를 따른다. `roam` 이 켜져 있으면 초당 30번 로키를 한 걸음씩
/// 움직여(`RockyKit.Roamer`) 패널을 옮긴다. 창 목록은 0.5초마다 다시 읽는다 — 위치만 읽으니 화면 기록 권한은 필요 없다.
@MainActor
final class AvatarController {
    private var panel: NSPanel?
    /// 튜브를 그리는 패널 — 로키 패널 바로 뒤에 뜨고 클릭은 늘 흘려보낸다.
    private var tubePanel: NSPanel?
    private let tubeArt = TubeArt()
    private var shownTube: Tube?
    private let agents: AgentStore
    private let pose = RoamPose()
    private var timer: Timer?
    private var roamer: Roamer?
    private var windows: [Box] = []
    private var scanned: TimeInterval = 0
    private var last: TimeInterval = 0
    /// 마지막으로 옮겨 놓은 자리 — 이것과 다르면 사람이 끌어다 놓은 것이다.
    private var placed: NSPoint?

    /// 패널 안 로키 발의 자리 — 로키 칸(96×88)은 패널 오른쪽 아래에 있고, 발은 무대 330 중 312 높이다.
    private static let size = NSSize(width: 240, height: 170)
    private static let foot = NSPoint(x: 240 - 48, y: 88 * (330 - 312) / 330)

    init(agents: AgentStore) {
        self.agents = agents
        // App.init 은 NSApplication 이 다 뜨기 전에 돈다 — 패널은 다음 바퀴에 만든다.
        Task { @MainActor in self.follow() }
    }

    /// 켜고 끄는 것을 따라간다 — Observation 은 한 번 알리므로 바뀔 때마다 다시 건다.
    private func follow() {
        withObservationTracking {
            _ = agents.showAvatar
            _ = agents.roam
        } onChange: { [weak self] in
            Task { @MainActor in self?.follow() }
        }
        agents.showAvatar ? show() : hide()
        if agents.showAvatar, agents.roam { startRoaming() } else { stopRoaming() }
    }

    private func show() {
        if panel == nil {
            let panel = Self.floatingPanel(size: Self.size)
            panel.isMovableByWindowBackground = true
            panel.contentView = NSHostingView(rootView: AvatarView(agents: agents, pose: pose))
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
        hideTube()
    }

    private static func floatingPanel(size: NSSize) -> NSPanel {
        let panel = NSPanel(
            contentRect: NSRect(origin: .zero, size: size),
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
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        return panel
    }

    /// 튜브 — 처음 보일 때 곡선을 한 번 펼쳐 패널 크기를 정하고, 그다음엔 자란 정도와 옅어짐만 바꾼다.
    private func showTube(_ roamer: Roamer) {
        guard let tube = roamer.tube else { return hideTube() }
        if tube != shownTube {
            shownTube = tube
            // 관은 발판 위에 얹히게 중심선을 띄우고, 두 끝은 발판 표면까지 내린다.
            var points = [(tube.startX, tube.startY)]
            points += (0 ... 48).map { i in
                let p = tube.point(at: Double(i) / 48)
                return (p.x, p.y + TubeSize.lift)
            }
            points.append((tube.endX, tube.endY))
            let pad = TubeSize.width
            let minX = points.map { $0.0 }.min()! - pad, maxX = points.map { $0.0 }.max()! + pad
            let minY = points.map { $0.1 }.min()! - pad, maxY = points.map { $0.1 }.max()! + pad
            let frame = NSRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
            tubeArt.points = points.map { CGPoint(x: $0.0 - minX, y: maxY - $0.1) }
            if tubePanel == nil {
                let panel = Self.floatingPanel(size: frame.size)
                panel.ignoresMouseEvents = true
                panel.contentView = NSHostingView(rootView: TubeView(art: tubeArt))
                tubePanel = panel
            }
            tubePanel?.setFrame(frame, display: true)
            tubePanel?.orderFrontRegardless()
            // 로키는 관 안에 든 것처럼 보이게 늘 관 앞에.
            panel?.orderFrontRegardless()
        }
        tubeArt.reveal = roamer.tubeReveal
        tubeArt.opacity = roamer.tubeOpacity
    }

    private func hideTube() {
        guard shownTube != nil else { return }
        shownTube = nil
        tubePanel?.orderOut(nil)
    }

    private func startRoaming() {
        guard timer == nil else { return }
        // 초당 30번 옮기는 동안은 자리를 기억하지 않는다 — 매번 설정 파일에 쓰게 된다.
        panel?.setFrameAutosaveName("")
        last = ProcessInfo.processInfo.systemUptime
        let timer = Timer(timeInterval: 1 / 30, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    private func stopRoaming() {
        timer?.invalidate()
        timer = nil
        roamer = nil
        placed = nil
        pose.action = .stand
        pose.shrunk = false
        hideTube()
        panel?.ignoresMouseEvents = false
        panel?.setFrameAutosaveName("RockyAvatar")
    }

    private func tick() {
        guard let panel else { return }
        let now = ProcessInfo.processInfo.systemUptime
        let dt = min(now - last, 0.1)
        last = now
        let origin = panel.frame.origin
        let foot = NSPoint(x: origin.x + Self.foot.x, y: origin.y + Self.foot.y)
        if roamer == nil { roamer = Roamer(x: foot.x, y: foot.y) }
        // 사람이 끌고 있으면 따라가기만 하고, 놓으면 그 자리에서 떨어진다.
        if let placed, placed != origin {
            roamer?.drop(atX: foot.x, y: foot.y)
            pose.action = .fall
            pose.shrunk = false
            hideTube()
            if NSEvent.pressedMouseButtons & 1 != 0 {
                self.placed = origin
                return
            }
        }
        guard var roamer else { return }
        let screen = (NSScreen.screens.first { $0.frame.contains(foot) } ?? NSScreen.main)?.visibleFrame ?? .zero
        let area = Box(x: screen.minX, y: screen.minY, width: screen.width, height: screen.height)
        if now - scanned > 0.5 {
            windows = Self.scanWindows()
            scanned = now
        }
        roamer.step(
            dt: dt, ledges: ledges(windows: windows, screen: area), walls: walls(windows: windows, screen: area),
            screen: area, mood: agents.mood.roamMood, random: { Double.random(in: 0 ..< 1) }
        )
        roamer.x = min(max(roamer.x, area.minX + Roamer.halfWidth), area.maxX - Roamer.halfWidth)
        self.roamer = roamer
        let next = NSPoint(x: roamer.x - Self.foot.x, y: roamer.y - Self.foot.y)
        if next != origin { panel.setFrameOrigin(next) }
        placed = next
        // 패널은 240×170 이지만 로키는 오른쪽 아래 96×88 뿐 — 그 밖의 클릭은 아래 창으로 흘려보낸다(말풍선이 뜬 동안은 패널 전체).
        let mouse = NSEvent.mouseLocation
        let figure = NSRect(x: next.x + Self.size.width - 96, y: next.y, width: 96, height: 88)
        var hot = figure.contains(mouse)
        if let bubble = agents.mood.bubble, bubble != pose.dismissed {
            hot = NSRect(origin: next, size: Self.size).contains(mouse)
        }
        if panel.ignoresMouseEvents == hot { panel.ignoresMouseEvents = !hot }
        if pose.action != roamer.action { pose.action = roamer.action }
        if pose.facing != roamer.facing { pose.facing = roamer.facing }
        if pose.shrunk != roamer.insideTube { pose.shrunk = roamer.insideTube }
        showTube(roamer)
    }

    /// 화면에 떠 있는 보통 창(레이어 0)들 — 앞에서 뒤 순서, AppKit 좌표로. 자기 창과 아주 작은 창은 뺀다.
    private static func scanWindows() -> [Box] {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        guard let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else { return [] }
        let top = NSScreen.screens.first?.frame.maxY ?? 0
        let me = ProcessInfo.processInfo.processIdentifier
        return list.compactMap { info in
            guard (info[kCGWindowLayer as String] as? Int) == 0,
                  (info[kCGWindowOwnerPID as String] as? Int32) != me,
                  (info[kCGWindowAlpha as String] as? Double ?? 1) > 0.1,
                  let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                  let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary),
                  rect.width >= 80, rect.height >= 40
            else { return nil }
            // CG 창 좌표는 주 화면 왼쪽 위가 원점이고 y 가 아래로 자란다.
            return Box(x: rect.minX, y: top - rect.maxY, width: rect.width, height: rect.height)
        }
    }
}

/// 돌아다니는 동안의 몸짓 — 컨트롤러가 쓰고 로키 그림이 읽는다.
@MainActor
@Observable
final class RoamPose {
    var action: RoamAction = .stand
    var facing: Double = 1
    /// 튜브 안에서는 몸을 줄인다.
    var shrunk = false
    /// 누른 말풍선 — 같은 글이면 다시 띄우지 않고, 글이 바뀌면(다른 세션이 기다리면) 다시 뜬다.
    var dismissed: String?
}

/// 관의 굵기와, 발 자리에서 관 중심선까지의 높이 — 줄어든 로키(약 0.45배)가 관 한가운데 오게.
private enum TubeSize {
    static let width: Double = 54
    static let lift: Double = 22
}

/// 튜브 그림의 상태 — 컨트롤러가 쓰고 `TubeView` 가 읽는다. 점은 튜브 패널 안의 SwiftUI 좌표(y 가 아래로)다.
@MainActor
@Observable
final class TubeArt {
    var points: [CGPoint] = []
    var reveal: Double = 0
    var opacity: Double = 1
}

/// 헤일메리의 터널 같은 반투명 관 — 호박색 테두리에 옅은 속, 위쪽에 유리 같은 빛 한 줄.
private struct TubeView: View {
    let art: TubeArt

    var body: some View {
        let points = art.points
        let reveal = art.reveal
        return Canvas { gc, _ in
            var path = Path()
            path.addLines(points)
            let shown = path.trimmedPath(from: 0, to: reveal)
            let style = StrokeStyle(lineWidth: TubeSize.width, lineCap: .butt, lineJoin: .round)
            gc.stroke(shown, with: .color(Color(red: 0.55, green: 0.42, blue: 0.25).opacity(0.75)), style: style)
            var inner = gc
            inner.blendMode = .copy
            inner.stroke(
                shown, with: .color(Color(red: 0.95, green: 0.86, blue: 0.66).opacity(0.28)),
                style: StrokeStyle(lineWidth: TubeSize.width - 8, lineCap: .butt, lineJoin: .round)
            )
            gc.stroke(
                shown.offsetBy(dx: 0, dy: -TubeSize.width / 4), with: .color(.white.opacity(0.35)),
                style: StrokeStyle(lineWidth: 3, lineCap: .round, lineJoin: .round)
            )
        }
        .opacity(art.opacity)
    }
}

extension AvatarMood {
    /// 일하는 중이면 돌아다니고, 내 차례·연결 없음이면 그 자리에 서 있고, 다 쉬면 앉아 존다.
    var roamMood: RoamMood {
        switch self {
        case .working: .roam
        case .waiting, .unknown: .stay
        case .resting: .sleep
        }
    }
}

struct AvatarView: View {
    let agents: AgentStore
    let pose: RoamPose
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            if let bubble = agents.mood.bubble, bubble != pose.dismissed {
                Text(bubble)
                    .font(.caption)
                    .lineLimit(3)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
                    .frame(maxWidth: 220, alignment: .trailing)
                    .onTapGesture { pose.dismissed = bubble }
                    .transition(.opacity)
            }
            Rocky(mood: agents.mood, action: pose.action, facing: pose.facing)
                .frame(width: 96, height: 88)
                .scaleEffect(pose.shrunk ? 0.45 : 1, anchor: UnitPoint(x: 0.5, y: 312 / 330))
                .animation(.easeInOut(duration: 0.25), value: pose.shrunk)
                .help(agents.mood.summary)
                // 한 번 누르는 건 끌어 옮기다 놓는 것과 헷갈린다 — 웹 에이전트 탭은 두 번 눌러야 연다.
                .onTapGesture(count: 2) {
                    if let url = agents.agentsURL { openURL(url) }
                }
        }
        .frame(width: 240, height: 170, alignment: .bottomTrailing)
        .contextMenu {
            Button("에이전트 탭 열기") {
                if let url = agents.agentsURL { openURL(url) }
            }
            Button(agents.roam ? "돌아다니기 멈추기" : "돌아다니기") { agents.roam.toggle() }
            Button("아바타 숨기기") { agents.showAvatar = false }
        }
        .animation(.easeInOut(duration: 0.3), value: agents.mood)
        .animation(.easeInOut(duration: 0.2), value: pose.dismissed)
    }
}

extension AvatarMood {
    /// 말풍선 — 내 차례일 때만 띄우고, 나머지는 조용히 둔다(마우스를 올리면 `help` 로 보인다).
    var bubble: String? {
        guard case let .waiting(name, line, count) = self else { return nil }
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
    var action: RoamAction = .stand
    var facing: Double = 1

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
        if case .unknown = mood, action == .stand { return false }
        return true
    }

    private func draw(in gc: inout GraphicsContext, size: CGSize, t: Double) {
        // 무대는 360×330 — 프레임에 맞춰 늘린다.
        gc.scaleBy(x: size.width / 360, y: size.height / 330)
        // 그림자는 발이 무언가를 딛고 있을 때만.
        if action != .fall, action != .climb, action != .tube {
            gc.fill(
                Path(ellipseIn: CGRect(x: 180 - 95, y: 312 - 9, width: 190, height: 18)),
                with: .color(.black.opacity(0.13))
            )
        }
        guard let art = RockyArt.shared else {
            // 그림을 못 읽으면 돌 하나라도.
            gc.fill(Path(ellipseIn: CGRect(x: 110, y: 150, width: 140, height: 120)), with: .color(.gray))
            return
        }
        let q = RigPose(mood, action: action, t: t)
        if q.gray { gc.addFilter(.grayscale(1)) }
        // 왼쪽을 보면 무대를 좌우로 뒤집는다 — 자세는 늘 오른쪽을 보는 것으로 짠다.
        var figure = gc
        if facing < 0 {
            figure.translateBy(x: 360, y: 0)
            figure.scaleBy(x: -1, y: 1)
        }

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
            var layer = figure
            layer.concatenate(m)
            layer.draw(image.image, in: CGRect(origin: .zero, size: image.size))
        }
        if case .resting = mood, action == .sit || action == .stand {
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

    init(_ mood: AvatarMood, action: RoamAction, t: Double) {
        if case .unknown = mood { gray = true }
        switch action {
        case .walk:
            // 걷기 — 한 발씩 들며 몸을 앞으로 숙이고 팔을 흔든다(0.6초 한 걸음 쌍).
            let p = sin(2 * .pi * t / 0.6)
            y = -5 * abs(p)
            angles = [
                .torso: 6 + 3 * p, .upperL: -35 + 20 * p, .foreL: 70, .upperR: 35 + 20 * p, .foreR: -70,
                .legL: -4 + 22 * max(0, p), .legR: 4 - 22 * max(0, -p),
            ]
            return
        case .climb, .tube:
            // 기어오르기(튜브 안에서 기어가기도) — 두 팔을 번갈아 뻗고 다리로 민다.
            let p = sin(2 * .pi * t / 0.8)
            angles = [
                .torso: -4, .upperL: 60 + 20 * p, .foreL: -30, .upperR: -60 + 20 * p, .foreR: 30,
                .legL: 10 + 20 * p, .legR: -10 + 20 * p,
            ]
            return
        case .fall:
            // 떨어지기 — 팔을 허우적거리고 다리를 벌린다.
            angles = [
                .upperL: 70 + 12 * sin(t * 22), .foreL: -20, .upperR: -70 + 12 * sin(t * 22 + 1), .foreR: 20,
                .legL: 28, .legR: -28,
            ]
            return
        case .stand, .sit:
            break
        }
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
