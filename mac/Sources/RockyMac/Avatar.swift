// 떠 있는 아바타 — 화면 구석의 작은 로키(프로젝트 헤일메리의 에리디언을 귀여운 돌 친구로 그린 것)가 에이전트 상태를 알린다.
// 내 차례인 세션이 있으면 껑충 뛰고 주먹 인사를 하며 말풍선에 기다리는 것을 띄우고, 도는 세션이 있으면 주먹으로 두드리고, 다 쉬면 엎드려 잠든다.
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
    private var roaming = false
    private var moves = Choreographer()
    /// 마지막 걸음의 발판 — 서 있는 자리(바닥 · 창 윗면 · 창 끝)를 가린다.
    private var ledgesHere: [Ledge] = []
    private var windows: [Box] = []
    private var scanned: TimeInterval = 0
    private var last: TimeInterval = 0
    /// 마지막으로 옮겨 놓은 자리 — 이것과 다르면 사람이 끌어다 놓은 것이다.
    private var placed: NSPoint?
    /// 왼쪽 버튼이 로키 위에서 눌린 채인지 — 지난 바퀴에 눌려 있었는지로 "막 눌렀다"를 가린다.
    private var dragging = false
    private var wasDown = false

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
        if agents.showAvatar, agents.roam { startRoaming() } else if roaming { stopRoaming() }
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
        startTimer()
    }

    private func hide() {
        panel?.orderOut(nil)
        hideTube()
        timer?.invalidate()
        timer = nil
    }

    /// 보이는 동안 초당 30번 — 몸짓은 늘 고르고, 돌아다니기가 켜져 있으면 걸음도 잰다.
    private func startTimer() {
        guard timer == nil else { return }
        last = ProcessInfo.processInfo.systemUptime
        let timer = Timer(timeInterval: 1 / 30, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
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
        guard !roaming else { return }
        roaming = true
        // 초당 30번 옮기는 동안은 자리를 기억하지 않는다 — 매번 설정 파일에 쓰게 된다.
        panel?.setFrameAutosaveName("")
    }

    private func stopRoaming() {
        roaming = false
        roamer = nil
        ledgesHere = []
        placed = nil
        dragging = false
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
        if roaming { roam(panel, now: now, dt: dt) }
        choreograph(panel)
    }

    /// 서 있을 때의 몸짓 — `RockyKit.Choreographer` 가 고르고, 내 차례에 마우스 쪽 보기·창 끝에서 내려다보기만 방향을 바꾼다.
    private func choreograph(_ panel: NSPanel) {
        let here = roamer.map { spot(x: $0.x, y: $0.y, ledges: ledgesHere) } ?? .floor
        let clip = moves.update(
            now: Date.timeIntervalSinceReferenceDate, mood: agents.mood.moveMood, spot: here,
            random: { Double.random(in: 0 ..< 1) }
        )
        if pose.clip != clip { pose.clip = clip }
        let squash = roamer?.squash ?? 0, crouch = roamer?.crouch ?? 0
        if pose.squash != squash { pose.squash = squash }
        if pose.crouch != crouch { pose.crouch = crouch }
        guard pose.action == .stand || pose.action == .sit else { return }
        var facing = pose.facing
        switch clip.move {
        case .watch: facing = NSEvent.mouseLocation.x >= panel.frame.minX + Self.foot.x ? 1 : -1
        case .peek: if case let .edge(toward) = here { facing = toward }
        default: break
        }
        if pose.facing != facing { pose.facing = facing }
    }

    private func roam(_ panel: NSPanel, now: TimeInterval, dt: Double) {
        let origin = panel.frame.origin
        let foot = NSPoint(x: origin.x + Self.foot.x, y: origin.y + Self.foot.y)
        if roamer == nil { roamer = Roamer(x: foot.x, y: foot.y) }
        // 사람이 로키를 누르고 있는 동안은 걷지 않고 포인터에 매달려 있다 — 누른 채 멈춰 있어도(패널이 안 움직여도)
        // 걷거나 떨어지면 포인터를 놓친다. 놓으면 그 자리에서 떨어진다.
        let down = NSEvent.pressedMouseButtons & 1 != 0
        if down, !wasDown, Self.figure(at: origin).contains(NSEvent.mouseLocation) { dragging = true }
        wasDown = down
        let moved = placed.map { $0 != origin } ?? false
        if dragging || moved {
            roamer?.drop(atX: foot.x, y: foot.y)
            pose.action = .fall
            pose.shrunk = false
            hideTube()
            placed = origin
            dragging = down
            if down { return }
        }
        guard var roamer else { return }
        let screen = (NSScreen.screens.first { $0.frame.contains(foot) } ?? NSScreen.main)?.visibleFrame ?? .zero
        let area = Box(x: screen.minX, y: screen.minY, width: screen.width, height: screen.height)
        if now - scanned > 0.5 {
            windows = Self.scanWindows()
            scanned = now
        }
        ledgesHere = ledges(windows: windows, screen: area)
        roamer.step(
            dt: dt, ledges: ledgesHere, walls: walls(windows: windows, screen: area),
            screen: area, mood: agents.mood.roamMood, random: { Double.random(in: 0 ..< 1) }
        )
        roamer.x = min(max(roamer.x, area.minX + Roamer.halfWidth), area.maxX - Roamer.halfWidth)
        self.roamer = roamer
        let next = NSPoint(x: roamer.x - Self.foot.x, y: roamer.y - Self.foot.y)
        if next != origin { panel.setFrameOrigin(next) }
        placed = next
        // 패널은 240×170 이지만 로키는 오른쪽 아래 96×88 뿐 — 그 밖의 클릭은 아래 창으로 흘려보낸다(말풍선이 뜬 동안은 패널 전체).
        let mouse = NSEvent.mouseLocation
        var hot = Self.figure(at: next).contains(mouse)
        if let bubble = agents.mood.bubble, bubble != pose.dismissed {
            hot = NSRect(origin: next, size: Self.size).contains(mouse)
        }
        if panel.ignoresMouseEvents == hot { panel.ignoresMouseEvents = !hot }
        if pose.action != roamer.action { pose.action = roamer.action }
        if pose.facing != roamer.facing { pose.facing = roamer.facing }
        if pose.shrunk != roamer.insideTube { pose.shrunk = roamer.insideTube }
        showTube(roamer)
    }

    /// 패널이 `origin` 에 있을 때 로키 칸(패널 오른쪽 아래 96×88)의 화면 자리.
    private static func figure(at origin: NSPoint) -> NSRect {
        NSRect(x: origin.x + size.width - 96, y: origin.y, width: 96, height: 88)
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
    /// 서 있을 때의 몸짓, 착지 반동의 남은 정도, 점프 전 쪼그림.
    var clip = Clip(move: .mood, start: 0, length: 0, tempo: 1)
    var squash = 0.0
    var crouch = 0.0
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
    /// 몸짓을 고르는 쪽에 넘기는 기분 — 기다리는 세션이 바뀌면 다시 크게 알린다.
    var moveMood: MoveMood {
        switch self {
        case .working: .working
        case let .waiting(name, _, _): .waiting(who: name)
        case .resting, .unknown: .other
        }
    }

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
            Rocky(
                mood: agents.mood, action: pose.action, facing: pose.facing,
                clip: pose.clip, squash: pose.squash, crouch: pose.crouch
            )
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

/// 로키 — 도형으로 그리는 돌 친구(낮은 돔 몸통 · 세 손가락 팔 둘 · 다리 둘 · 얼굴).
/// 모양은 `RockyArt` 가, 순간의 자세(관절 각도 · 표정)는 `RockyPose` 가 정한다. 자세는 늘 오른쪽을 보는 것으로 짠다.
private struct Rocky: View {
    let mood: AvatarMood
    var action: RoamAction = .stand
    var facing: Double = 1
    var clip = Clip(move: .mood, start: 0, length: 0, tempo: 1)
    var squash = 0.0
    var crouch = 0.0

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
        // 그림자는 발이 무언가를 딛고 있을 때만 — 점프는 쪼그린 동안만 딛고 있다.
        let airborne = action == .fall || action == .tube || (action == .jump && crouch == 0)
        if !airborne {
            gc.fill(
                Path(ellipseIn: CGRect(x: 180 - 85, y: 314 - 8, width: 170, height: 16)),
                with: .color(.black.opacity(0.12))
            )
        }
        let q = RockyPose(mood, action: action, t: t, clip: clip, squash: squash, crouch: crouch)
        if q.gray { gc.addFilter(.grayscale(1)) }
        // 왼쪽을 보면 무대를 좌우로 뒤집는다.
        var figure = gc
        if facing < 0 {
            figure.translateBy(x: 360, y: 0)
            figure.scaleBy(x: -1, y: 1)
        }
        // 발바닥이 무대의 발 자리(y 312)에 오게 — 기울기와 눌림은 몸통 가운데를 축으로.
        figure.translateBy(x: 180, y: 312 - (RockyArt.sole - RockyArt.center.y) * RockyArt.scale + q.y)
        figure.rotate(by: .degrees(q.tilt))
        figure.scaleBy(x: RockyArt.scale, y: RockyArt.scale * q.squash)
        figure.translateBy(x: -RockyArt.center.x, y: -RockyArt.center.y)
        RockyArt.draw(q, in: figure)
        if case .resting = mood, action == .sit || action == .stand {
            let rise = (t.truncatingRemainder(dividingBy: 3)) / 3
            gc.draw(
                Text("z").font(.system(size: 40, weight: .semibold)).foregroundColor(.secondary.opacity(1 - rise)),
                at: CGPoint(x: 290 + rise * 20, y: 190 - rise * 60)
            )
        }
    }
}

/// 한 순간의 자세. 팔의 `raise` 는 몸 밖으로 드는 각도(0 이면 시안처럼 비스듬히 내린 팔), `elbow` 는 팔꿈치를 펴는 쪽이 +.
/// 다리는 화면 시계 방향이 + — 왼다리 +, 오른다리 − 면 벌린다. `tilt` 도 시계 방향(오른쪽을 볼 때 앞으로 숙임)이 +.
private struct RockyPose {
    enum Face { case smile, happy, joy, focus, puzzled, surprise, sleep }
    enum Hand { case rest, open, fist }
    enum Side { case left, right }

    struct Arm {
        var raise = 0.0
        var elbow = 0.0
        var hand = Hand.rest
    }

    var y = 0.0
    var tilt = 0.0
    var squash = 1.0
    /// 눈동자가 보는 쪽(−1 … 1).
    var look = 0.0
    var face = Face.smile
    var blink = false
    var armL = Arm()
    var armR = Arm()
    var legL = 0.0
    var legR = 0.0
    /// 몸통 앞으로 내미는 팔 — 머리를 긁거나 닦을 때.
    var front: Side?
    /// 엎드려 자기 — 다리를 감추고 팔을 몸통 앞으로 늘어뜨린다.
    var prone = false
    var gray = false

    /// 두 자세 사이 — 수는 섞고, 표정·손 모양처럼 섞을 수 없는 것은 절반을 넘으면 바꾼다.
    static func mix(_ a: RockyPose, _ b: RockyPose, _ w: Double) -> RockyPose {
        func lerp(_ x: Double, _ y: Double) -> Double { x + (y - x) * w }
        func arm(_ x: Arm, _ y: Arm) -> Arm {
            Arm(raise: lerp(x.raise, y.raise), elbow: lerp(x.elbow, y.elbow), hand: w < 0.5 ? x.hand : y.hand)
        }
        var out = w < 0.5 ? a : b
        out.y = lerp(a.y, b.y)
        out.tilt = lerp(a.tilt, b.tilt)
        out.squash = lerp(a.squash, b.squash)
        out.look = lerp(a.look, b.look)
        out.armL = arm(a.armL, b.armL)
        out.armR = arm(a.armR, b.armR)
        out.legL = lerp(a.legL, b.legL)
        out.legR = lerp(a.legR, b.legR)
        return out
    }

    /// 숨 — 서로 나누어떨어지지 않는 주기의 사인 둘을 더해 박자가 쉽게 되풀이되지 않게.
    private static func breath(_ t: Double) -> Double {
        0.6 * sin(t * 1.9) + 0.4 * sin(t * 3.1 + 1)
    }

    /// 토막의 앞뒤 0.2초는 바탕 자세와 섞는다 — 동작이 바뀔 때 팔이 튀지 않게.
    private static func ease(_ clip: Clip, _ t: Double) -> Double {
        let elapsed = t - clip.start
        return min(max(0, min(1, elapsed / 0.2)), max(0, min(1, (clip.length - elapsed) / 0.2)))
    }

    /// 껑충 — h 가 0(땅)에서 1(가장 높이)으로 갈수록 몸이 뜨고 두 손을 펴서 치켜들고 다리를 벌린다.
    private static func hop(_ h: Double) -> RockyPose {
        var q = RockyPose()
        q.y = -45 * h
        q.face = .joy
        q.armL = Arm(raise: 135 * h, hand: .open)
        q.armR = Arm(raise: 135 * h, hand: .open)
        q.legL = 20 * h
        q.legR = -20 * h
        return q
    }

    /// 도는 중의 바탕 — 집중한 얼굴로 앞을 본다.
    private static let work: RockyPose = {
        var q = RockyPose()
        q.face = .focus
        q.look = 1
        q.armR.hand = .fist
        return q
    }()

    init() {}

    init(_ mood: AvatarMood, action: RoamAction, t: Double, clip: Clip, squash: Double, crouch: Double) {
        base(mood, action: action, t: t, clip: clip, crouch: crouch)
        if case .unknown = mood { gray = true }
        // 눈은 4초쯤에 한 번 깜빡인다.
        if face == .smile || face == .focus || face == .puzzled { blink = t.truncatingRemainder(dividingBy: 3.7) < 0.12 }
        // 착지 반동 — 쭈그렸다 편다.
        if squash > 0 {
            let a = sin(.pi * (1 - squash))
            y += 14 * a
            self.squash *= 1 - 0.12 * a
            legL += 10 * a
            legR -= 10 * a
        }
    }

    private mutating func base(_ mood: AvatarMood, action: RoamAction, t: Double, clip: Clip, crouch: Double) {
        switch action {
        case .walk, .tube:
            // 걷기(튜브 안에서 기어가기도) — 뒤뚱뒤뚱 몸을 흔들며 팔을 젓는다(박자는 ±12% 쯤 천천히 흔들린다).
            // 팔은 몸보다 한 박자 늦게 따라온다.
            let phase = 2 * .pi * t / (action == .tube ? 0.4 : 0.6) + 2.5 * sin(t * 0.5)
            let p = sin(phase)
            let swing = sin(phase - 0.6)
            y = -5 * abs(p)
            tilt = 4 + 2 * p
            look = 1
            legL = 12 * p
            legR = 12 * p
            armL.raise = -8 + 14 * swing
            armR.raise = -8 - 14 * swing
            return
        case .jump:
            if crouch > 0 {
                // 뛰기 전 — 몸을 낮추고 팔을 뒤로 모은다.
                y = 14 * crouch
                squash = 1 - 0.14 * crouch
                face = .focus
                look = 1
                armL.raise = -15 * crouch
                armR.raise = -15 * crouch
                legL = 6 * crouch
                legR = -6 * crouch
            } else {
                self = Self.hop(1)
            }
            return
        case .fall:
            // 떨어지기 — 손을 펴고 허우적거리며 다리를 벌린다.
            face = .surprise
            armL = Arm(raise: 110 + 12 * sin(t * 22), elbow: 20, hand: .open)
            armR = Arm(raise: 110 + 12 * sin(t * 22 + 1), elbow: 20, hand: .open)
            legL = 30
            legR = -30
            return
        case .stand, .sit:
            break
        }
        switch mood {
        case .waiting:
            waiting(t: t, clip: clip)
        case .working:
            working(t: t, clip: clip)
        case .resting:
            // 다 쉼 — 엎드려 잔다. 숨에 맞춰 등이 오르내린다.
            let b = Self.breath(t)
            prone = true
            face = .sleep
            y = 44 + 1.5 * b
            squash = 0.82 + 0.012 * b
            armL = Arm(raise: 40, elbow: -62)
            armR = Arm(raise: 40, elbow: -62)
        case .unknown:
            // 연결 없음 — 회색으로 머리를 긁으며 갸웃.
            tilt = -12
            face = .puzzled
            look = -1
            front = .right
            armR = Arm(raise: 165, elbow: 70)
        }
    }

    /// 내 차례 — 처음 몇 초는 껑충 뛰고 주먹 인사를 하고, 그 뒤엔 마우스 쪽을 보고 숨 쉬다가 가끔 손을 흔든다.
    private mutating func waiting(t: Double, clip: Clip) {
        var watch = RockyPose()
        let b = Self.breath(t)
        watch.y = -1.5 * b
        watch.look = 1
        watch.armL.raise = 3 * Self.breath(t - 0.25)
        watch.armR.raise = watch.armL.raise
        func wave(_ sway: Double) -> RockyPose {
            var q = RockyPose()
            q.y = -8 * abs(sway)
            q.face = .happy
            q.tilt = 5 * sway
            q.armR = Arm(raise: 140, elbow: -10 + 25 * sin(t * 14), hand: .open)
            q.legL = 4 + 4 * sway
            q.legR = -4 + 4 * sway
            return q
        }
        switch clip.move {
        case .watch:
            self = watch
        case .wave:
            let sway = sin(2 * .pi * (t - clip.start) / clip.length)
            self = Self.mix(watch, wave(sway), Self.ease(clip, t))
        default:
            // 크게 알리기(2.4초 한 바퀴) — 토막이 시작된 때부터 세어 늘 뛰기로 시작한다.
            let phase = (t - clip.start).truncatingRemainder(dividingBy: 2.4)
            if phase < 1.2 {
                self = Self.hop(sin(.pi * phase.truncatingRemainder(dividingBy: 0.6) / 0.6))
            } else {
                // 주먹 인사 — 앞으로 주먹을 내밀었다 당긴다. 뛰기와 사이는 0.15초 동안 섞는다.
                let q = phase - 1.2
                var bump = RockyPose()
                bump.face = .joy
                bump.tilt = 4
                bump.armR = Arm(raise: 70 + 10 * sin(2 * .pi * q / 0.6), elbow: -35, hand: .fist)
                let w = min(max(0, min(1, q / 0.15)), max(0, min(1, (1.2 - q) / 0.15)))
                self = Self.mix(Self.hop(0), bump, w)
            }
        }
    }

    /// 도는 중 — 박자(`tempo`)마다 오른주먹을 들어 내리치고, 몇 번 치고 나면 쉬는 동작 하나를 한다.
    private mutating func working(t: Double, clip: Clip) {
        let u = clip.progress(at: t)
        var move = Self.work
        switch clip.move {
        case .look:
            // 두리번 — 몸을 앞뒤로 기울이며 눈을 굴린다.
            let turn = sin(2 * .pi * u)
            move.y = -2
            move.tilt = 7 * turn
            move.look = turn
            move.face = .smile
        case .wipe:
            // 땀 닦기 — 오른손을 머리에 대고 문지른다.
            move.face = .happy
            move.front = .right
            move.armR = Arm(raise: 165, elbow: 70 + 15 * sin(2 * .pi * 2.5 * u))
        case .shoulder:
            // 팔 돌리기 — 두 팔을 엇갈려 크게 돌린다.
            let s = sin(2 * .pi * 2 * u)
            move.face = .smile
            move.armL = Arm(raise: 40 + 40 * s)
            move.armR = Arm(raise: 40 - 40 * s, hand: .fist)
        case .stomp:
            // 발 구르기 — 발을 번갈아 들었다 쿵.
            let s = sin(2 * .pi * 3 * u)
            move.y = 4 * abs(s)
            move.tilt = 3 * s
            move.legL = 10 + 25 * max(0, s)
            move.legR = -10 - 25 * max(0, -s)
        case .sit:
            // 잠깐 앉기 — 주저앉아 숨을 고른다.
            move.face = .happy
            move.y = 22 + 2 * Self.breath(t)
            move.squash = 0.9
            move.legL = 35
            move.legR = -35
            move.armL.raise = -10
            move.armR = Arm(raise: -10)
        case .peek:
            // 내려다보기 — 창 끝 쪽으로 몸을 숙인다(방향은 컨트롤러가 끝 쪽으로 돌린다).
            move.face = .surprise
            move.y = 2
            move.tilt = 18 + 3 * sin(t * 2)
            move.legL = 6
            move.legR = -6
        default:
            hammer(t: t, clip: clip)
            return
        }
        self = Self.mix(Self.work, move, Self.ease(clip, t))
    }

    private mutating func hammer(t: Double, clip: Clip) {
        let phase = ((t - clip.start) / max(clip.tempo, 0.1)).truncatingRemainder(dividingBy: 1)
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
        self = Self.work
        y = -4 * lift + 5 * hit
        tilt = 3 * lift + 5 * hit
        armR = Arm(raise: 20 + 100 * lift, elbow: 40 * lift, hand: .fist)
        legL = 4 * hit
        legR = -4 * hit
    }
}

/// 로키의 모양 — 좌표는 시안(서 있는 그림)을 본뜬 몸통 좌표로, 몸통 가운데가 (258, 160), 발바닥이 y 312 다.
/// 무대에는 `scale` 배로 줄여 올린다.
private enum RockyArt {
    static let scale = 0.6
    static let center = CGPoint(x: 258, y: 160)
    static let sole = 312.0

    private static let line = rgb(0x48221A)
    private static let limb = rgb(0xDCAE76)
    private static let rim = rgb(0xBF8A57)
    private static let teal = rgb(0x2A8C7C)
    private static let tealLight = rgb(0x8FD6C4)
    private static let tongue = rgb(0xE0707A)
    private static let outline = StrokeStyle(lineWidth: 6, lineCap: .round, lineJoin: .round)

    /// 어깨와 엉덩이 — 몸통 좌표.
    private static let shoulderL = CGPoint(x: 90, y: 176)
    private static let shoulderR = CGPoint(x: 426, y: 176)
    private static let hipL = CGPoint(x: 154, y: 236)
    private static let hipR = CGPoint(x: 362, y: 236)

    private static func rgb(_ hex: UInt32, _ alpha: Double = 1) -> Color {
        Color(
            red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255, opacity: alpha
        )
    }

    private static func oval(_ x: Double, _ y: Double, _ rx: Double, _ ry: Double) -> Path {
        Path(ellipseIn: CGRect(x: x - rx, y: y - ry, width: rx * 2, height: ry * 2))
    }

    private static func solid(_ gc: GraphicsContext, _ path: Path, _ color: Color = limb) {
        gc.fill(path, with: .color(color))
        gc.stroke(path, with: .color(line), style: outline)
    }

    private static func pen(_ gc: GraphicsContext, _ path: Path, _ width: Double) {
        gc.stroke(path, with: .color(line), style: StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round))
    }

    /// 뒤에서 앞으로 — 다리, 팔, 몸통, 얼굴, 그리고 앞으로 내민 팔.
    static func draw(_ q: RockyPose, in gc: GraphicsContext) {
        let armL = { arm(gc, at: shoulderL, angle: 52 + q.armL.raise, elbow: -40 + q.armL.elbow, hand: q.armL.hand) }
        let armR = { arm(gc, at: shoulderR, angle: -52 - q.armR.raise, elbow: 40 - q.armR.elbow, hand: q.armR.hand) }
        if !q.prone {
            leg(gc, at: hipL, angle: q.legL)
            leg(gc, at: hipR, angle: q.legR)
            if q.front != .left { armL() }
            if q.front != .right { armR() }
        }
        body(gc)
        var faceLayer = gc
        faceLayer.translateBy(x: 258, y: 172)
        faceLayer.scaleBy(x: 0.8, y: 0.8)
        faceLayer.translateBy(x: -230, y: -188)
        face(faceLayer, q.blink ? .sleep : q.face, look: q.look, sleeping: q.face == .sleep)
        if q.prone || q.front == .left { armL() }
        if q.prone || q.front == .right { armR() }
    }

    /// 몸통 — 아래가 둥근 낮은 돔, 가운데가 밝은 음영, 거친 돌 점, 금 둘, 청록 점 셋.
    private static func body(_ gc: GraphicsContext) {
        var shape = Path()
        shape.move(to: CGPoint(x: 258, y: 38))
        shape.addCurve(to: CGPoint(x: 446, y: 196), control1: CGPoint(x: 376, y: 38), control2: CGPoint(x: 450, y: 116))
        shape.addCurve(to: CGPoint(x: 258, y: 264), control1: CGPoint(x: 444, y: 244), control2: CGPoint(x: 374, y: 264))
        shape.addCurve(to: CGPoint(x: 70, y: 196), control1: CGPoint(x: 142, y: 264), control2: CGPoint(x: 72, y: 244))
        shape.addCurve(to: CGPoint(x: 258, y: 38), control1: CGPoint(x: 66, y: 116), control2: CGPoint(x: 140, y: 38))
        shape.closeSubpath()
        gc.fill(shape, with: .radialGradient(
            Gradient(stops: [
                .init(color: rgb(0xE8C08C), location: 0),
                .init(color: rgb(0xD9AA72), location: 0.7),
                .init(color: rgb(0xC99660), location: 1),
            ]),
            center: CGPoint(x: 245, y: 130), startRadius: 20, endRadius: 230
        ))
        var inside = gc
        inside.clip(to: shape)
        // 아래 그늘 띠와 꼭대기 빛.
        inside.fill(oval(258, 278, 200, 46), with: .color(rim))
        var shine = inside
        shine.translateBy(x: 205, y: 64)
        shine.rotate(by: .radians(-0.15))
        shine.fill(oval(0, 0, 70, 16), with: .color(rgb(0xFFF0D7, 0.18)))
        // 돌 질감 — 고정된 자리의 어둡고 밝은 점.
        for i in 0 ..< 70 {
            let x = Double(80 + (i * 137) % 360), y = Double(30 + (i * 89) % 225)
            let dot = i % 2 == 0 ? rgb(0xFFEBC8, 0.18) : rgb(0x784628, 0.10)
            inside.fill(oval(x, y, 1.8, 1.8), with: .color(dot))
        }
        gc.stroke(shape, with: .color(line), style: outline)
        var cracks = Path()
        cracks.addLines([CGPoint(x: 160, y: 66), CGPoint(x: 182, y: 70), CGPoint(x: 194, y: 88), CGPoint(x: 214, y: 98)])
        cracks.addLines([CGPoint(x: 326, y: 46), CGPoint(x: 304, y: 74), CGPoint(x: 284, y: 88), CGPoint(x: 270, y: 110)])
        cracks.addLines([CGPoint(x: 304, y: 74), CGPoint(x: 313, y: 104)])
        pen(gc, cracks, 3.5)
        for (x, y, r) in [(146.0, 122.0, 11.0), (346, 88, 14), (372, 114, 8)] {
            gc.fill(oval(x, y, r + 3, r + 3), with: .color(line))
            gc.fill(oval(x, y, r, r), with: .color(teal))
            gc.fill(oval(x - r * 0.3, y - r * 0.35, r * 0.32, r * 0.32), with: .color(tealLight))
        }
    }

    /// 팔 — 어깨에서 나오는 굵은 윗마디, 팔꿈치에서 꺾이는 아랫마디, 세 손가락 손(엄지는 없다).
    /// 각도는 도, 0 이면 곧게 아래, + 는 화면 시계 방향.
    private static func arm(_ gc: GraphicsContext, at: CGPoint, angle: Double, elbow: Double, hand: RockyPose.Hand) {
        var g = gc
        g.translateBy(x: at.x, y: at.y)
        g.rotate(by: .degrees(angle))
        var upper = Path()
        upper.move(to: CGPoint(x: -26, y: 0))
        upper.addCurve(to: CGPoint(x: -28, y: 66), control1: CGPoint(x: -36, y: 20), control2: CGPoint(x: -36, y: 48))
        upper.addLine(to: CGPoint(x: 28, y: 66))
        upper.addCurve(to: CGPoint(x: 26, y: 0), control1: CGPoint(x: 36, y: 48), control2: CGPoint(x: 36, y: 20))
        upper.addCurve(to: CGPoint(x: -26, y: 0), control1: CGPoint(x: 14, y: -14), control2: CGPoint(x: -14, y: -14))
        upper.closeSubpath()
        solid(g, upper)
        var crack = Path()
        crack.addLines([CGPoint(x: -16, y: 22), CGPoint(x: 0, y: 34), CGPoint(x: -4, y: 48)])
        pen(g, crack, 3)
        g.translateBy(x: 0, y: 62)
        g.rotate(by: .degrees(elbow))
        var fore = Path()
        fore.move(to: CGPoint(x: -29, y: -4))
        fore.addCurve(to: CGPoint(x: -26, y: 34), control1: CGPoint(x: -34, y: 8), control2: CGPoint(x: -32, y: 24))
        fore.addLine(to: CGPoint(x: 26, y: 34))
        fore.addCurve(to: CGPoint(x: 29, y: -4), control1: CGPoint(x: 32, y: 24), control2: CGPoint(x: 34, y: 8))
        fore.addCurve(to: CGPoint(x: -29, y: -4), control1: CGPoint(x: 18, y: -14), control2: CGPoint(x: -18, y: -14))
        fore.closeSubpath()
        solid(g, fore)
        g.translateBy(x: 0, y: 38)
        if hand == .fist {
            // 주먹 — 손가락을 말아 쥔 덩어리에 마디 금 셋.
            solid(g, oval(0, 10, 28, 24))
            var knuckles = Path()
            for x in [-12.0, 0, 12] {
                knuckles.move(to: CGPoint(x: x, y: 2))
                knuckles.addLine(to: CGPoint(x: x, y: 16))
            }
            pen(g, knuckles, 3.5)
            return
        }
        // 세 손가락 — 굵고 짧게, 바깥으로 벌려서. 편 손은 더 벌린다.
        let spread = hand == .open ? 1.25 : 1
        for (turn, length) in [(-0.75 * spread, 34.0), (0, 36), (0.75 * spread, 34)] {
            var finger = g
            finger.rotate(by: .radians(turn))
            solid(finger, Path(roundedRect: CGRect(x: -10, y: 0, width: 20, height: length), cornerRadius: 10))
        }
        solid(g, oval(0, 0, 24, 14))
    }

    /// 다리 — 짧고 굵은 기둥과 앞이 둥근 신발.
    private static func leg(_ gc: GraphicsContext, at: CGPoint, angle: Double) {
        var g = gc
        g.translateBy(x: at.x, y: at.y)
        g.rotate(by: .degrees(angle))
        var post = Path()
        post.addLines([CGPoint(x: -32, y: -10), CGPoint(x: -30, y: 52), CGPoint(x: 30, y: 52), CGPoint(x: 32, y: -10)])
        post.closeSubpath()
        solid(g, post)
        var shoe = Path()
        shoe.move(to: CGPoint(x: -44, y: 76))
        shoe.addCurve(to: CGPoint(x: 0, y: 44), control1: CGPoint(x: -48, y: 52), control2: CGPoint(x: -24, y: 44))
        shoe.addCurve(to: CGPoint(x: 44, y: 76), control1: CGPoint(x: 26, y: 44), control2: CGPoint(x: 48, y: 52))
        shoe.addCurve(to: CGPoint(x: -44, y: 76), control1: CGPoint(x: 32, y: 84), control2: CGPoint(x: -32, y: 84))
        shoe.closeSubpath()
        solid(g, shoe)
        var crease = Path()
        crease.move(to: CGPoint(x: -20, y: 62))
        crease.addQuadCurve(to: CGPoint(x: 6, y: 62), control: CGPoint(x: -6, y: 54))
        pen(g, crease, 3)
    }

    /// 얼굴 — 반짝이는 큰 눈 둘과 작은 입. `sleeping` 이면 입도 잠든 입.
    private static func face(_ gc: GraphicsContext, _ face: RockyPose.Face, look: Double, sleeping: Bool) {
        let eyes = [156.0, 304], ey = 188.0, lx = look * 7
        switch face {
        case .sleep:
            // 감은 눈 — 아래로 둥근 선.
            var lids = Path()
            for x in eyes {
                lids.move(to: CGPoint(x: x - 17.8, y: ey + 4.9))
                lids.addQuadCurve(to: CGPoint(x: x + 17.8, y: ey + 4.9), control: CGPoint(x: x, y: ey + 23))
            }
            pen(gc, lids, 7)
        case .happy:
            // 웃는 눈 — 위로 둥근 선.
            var lids = Path()
            for x in eyes {
                lids.move(to: CGPoint(x: x - 19.6, y: ey))
                lids.addQuadCurve(to: CGPoint(x: x + 19.6, y: ey), control: CGPoint(x: x, y: ey - 24))
            }
            pen(gc, lids, 7)
        default:
            for x in eyes {
                gc.fill(oval(x + lx, ey, 29, 32), with: .color(line))
                gc.fill(oval(x + lx + 10, ey - 11, 10, 10), with: .color(.white))
                gc.fill(oval(x + lx - 10, ey + 12, 4.5, 4.5), with: .color(.white.opacity(0.6)))
            }
        }
        var mark = Path()
        switch face {
        case .focus:
            mark.move(to: CGPoint(x: 130, y: 140))
            mark.addLine(to: CGPoint(x: 185, y: 152))
            mark.move(to: CGPoint(x: 330, y: 140))
            mark.addLine(to: CGPoint(x: 275, y: 152))
            mark.move(to: CGPoint(x: 214, y: 232))
            mark.addLine(to: CGPoint(x: 246, y: 232))
        case .puzzled:
            mark.move(to: CGPoint(x: 272, y: 142))
            mark.addLine(to: CGPoint(x: 318, y: 132))
            mark.move(to: CGPoint(x: 210, y: 234))
            mark.addQuadCurve(to: CGPoint(x: 230, y: 234), control: CGPoint(x: 220, y: 224))
            mark.addQuadCurve(to: CGPoint(x: 250, y: 234), control: CGPoint(x: 240, y: 244))
        case .joy:
            var mouth = Path()
            mouth.move(to: CGPoint(x: 205, y: 222))
            mouth.addQuadCurve(to: CGPoint(x: 255, y: 222), control: CGPoint(x: 230, y: 268))
            mouth.closeSubpath()
            gc.fill(mouth, with: .color(line))
            gc.fill(oval(230, 243, 9, 6), with: .color(tongue))
        case .surprise:
            gc.fill(oval(230, 236, 13, 16), with: .color(line))
            gc.fill(oval(230, 244, 9, 6), with: .color(tongue))
        case .sleep where sleeping:
            mark.move(to: CGPoint(x: 214, y: 226))
            mark.addQuadCurve(to: CGPoint(x: 246, y: 226), control: CGPoint(x: 230, y: 240))
        default:
            // 한쪽 끝이 말린 작은 미소.
            mark.move(to: CGPoint(x: 204, y: 214))
            mark.addQuadCurve(to: CGPoint(x: 214, y: 224), control: CGPoint(x: 206, y: 224))
            mark.move(to: CGPoint(x: 210, y: 224))
            mark.addQuadCurve(to: CGPoint(x: 254, y: 220), control: CGPoint(x: 232, y: 244))
        }
        pen(gc, mark, 6.5)
    }
}
