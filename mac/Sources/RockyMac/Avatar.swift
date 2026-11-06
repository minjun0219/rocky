// 떠 있는 아바타 — 화면 구석의 작은 로키(프로젝트 헤일메리의 에리디언을 귀여운 돌 친구로 그린 것)가 에이전트 상태를 알린다.
// 내 차례인 세션이 있으면 껑충 뛰고 주먹 인사를 하며 말풍선에 기다리는 것을 띄우고, 도는 세션이 있으면 주먹으로 두드리고, 다 쉬면 엎드려 잠든다.
// 창은 늘 위에 뜨는 패널이고 포커스를 가져가지 않는다. 돌아다니기를 켜면 창 윗면을 걷고, 끌어다 놓으면 떨어지고,
// 튜브를 놓아 창 윗면으로 오르거나 다른 창으로 건너간다. 두 번 누르면 웹 에이전트 탭이 열리고, 말풍선은 누르면 닫힌다.
// 따라다니기를 켜면 사람이 보는 곳(멈춘 포인터 · 입력 중인 앞 창) 옆으로 느긋하게 다가가고, 가끔 말풍선으로 세션 소식을 혼잣말한다.
import AppKit
import QuartzCore
import RockyKit
import SwiftUI

/// 아바타 패널을 띄우고 걷는다 — `AgentStore.showAvatar` 를 따른다. `roam` 이 켜져 있으면 화면 주사율에 맞춰(초당 60번)
/// 로키를 한 걸음씩 움직여(`RockyKit.Roamer`) 패널을 옮긴다. 창 목록은 0.5초마다 메인 스레드 밖에서 다시 읽는다 —
/// 위치만 읽으니 화면 기록 권한은 필요 없다.
@MainActor
final class AvatarController {
    private var panel: NSPanel?
    /// 튜브를 그리는 패널 — 로키 패널 바로 뒤에 뜨고 클릭은 늘 흘려보낸다.
    private var tubePanel: NSPanel?
    private let tubeArt = TubeArt()
    private var shownTube: Tube?
    private let agents: AgentStore
    private let pose = RoamPose()
    private var frames: CADisplayLink?
    private var roamer: Roamer?
    private var roaming = false
    private var moves = Choreographer()
    /// 마지막 걸음의 발판 — 서 있는 자리(바닥 · 창 윗면 · 창 끝)를 가린다.
    private var ledgesHere: [Ledge] = []
    /// 창 목록(앞에서 뒤 순서)과 그 주인 — 앞 앱의 맨 앞 창을 찾는 데 쓴다.
    private var seen: [SeenWindow] = []
    private var windows: [Box] { seen.map(\.box) }
    private var scanned: TimeInterval = 0
    /// 사람의 눈길과 그 옆에 설 자리 — 0.1초마다 다시 본다.
    private var gaze = GazeTracker()
    private var goal: Goal?
    private var gazed: TimeInterval = 0
    /// 마지막으로 띄운 혼잣말 번호와 내릴 시각.
    private var chatSeen = 0
    private var chatUntil: TimeInterval = 0
    /// 창 목록을 읽는 중인지 — 한 번에 하나만.
    private var scanning = false
    private var last: TimeInterval = 0
    /// 마지막으로 옮겨 놓은 자리 — 이것과 다르면 사람이 끌어다 놓은 것이다.
    private var placed: NSPoint?
    /// 왼쪽 버튼이 로키 위에서 눌린 채인지 — 지난 바퀴에 눌려 있었는지로 "막 눌렀다"를 가린다.
    private var dragging = false
    private var wasDown = false
    /// 화면이 잠들었는지 — 보이지 않는 동안은 박자를 멈춘다(꺼진 화면에서도 CPU 5% 를 쓰던 것, 2026-11-06 실측).
    private var screensAsleep = false
    private var observers: [NSObjectProtocol] = []

    /// 패널 안 로키 발의 자리 — 로키 칸(96×88)은 패널 오른쪽 아래에 있고, 발은 무대 330 중 312 높이다.
    private static let size = NSSize(width: 240, height: 170)
    private static let foot = NSPoint(x: 240 - 48, y: 88 * (330 - 312) / 330)

    init(agents: AgentStore) {
        self.agents = agents
        // App.init 은 NSApplication 이 다 뜨기 전에 돈다 — 패널은 다음 바퀴에 만든다.
        Task { @MainActor in self.follow() }
        let workspace = NSWorkspace.shared.notificationCenter
        for (name, asleep) in [
            (NSWorkspace.screensDidSleepNotification, true), (NSWorkspace.screensDidWakeNotification, false),
        ] {
            observers.append(workspace.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.screensAsleep = asleep
                    self?.updateVisibility()
                }
            })
        }
    }

    /// 보이는지 따라 박자와 그림을 멈추고 다시 돌린다 — 화면이 잠들었거나 패널이 다 가려지면(화면 잠금 포함) 멈춘다.
    private func updateVisibility() {
        let visible = !screensAsleep && (panel?.occlusionState.contains(.visible) ?? false)
        if pose.visible != visible { pose.visible = visible }
        guard let frames, frames.isPaused == visible else { return }
        frames.isPaused = !visible
        // 멈췄던 동안을 한 걸음으로 치지 않게.
        if visible { last = ProcessInfo.processInfo.systemUptime }
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
            observers.append(NotificationCenter.default.addObserver(
                forName: NSWindow.didChangeOcclusionStateNotification, object: panel, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.updateVisibility() }
            })
        }
        panel?.orderFrontRegardless()
        startFrames()
    }

    private func hide() {
        panel?.orderOut(nil)
        hideTube()
        frames?.invalidate()
        frames = nil
    }

    /// 보이는 동안 화면이 그릴 때마다(초당 60번) — 몸짓은 늘 고르고, 돌아다니기가 켜져 있으면 걸음도 잰다.
    /// `Timer` 로 초당 30번 옮기던 것은 화면 주사율(60 · 120Hz)과 박자가 어긋나 패널이 2-5 프레임씩 들쭉날쭉 멈춰 프레임 드랍처럼 보였다.
    /// 디스플레이 링크는 패널이 있는 화면의 주사율을 따른다 — 120Hz 화면에서도 60을 넘지 않게 묶어 고르게 나눠 떨어지게 한다.
    private func startFrames() {
        guard frames == nil, let view = panel?.contentView else { return }
        last = ProcessInfo.processInfo.systemUptime
        let ticker = FrameTicker { [weak self] in
            MainActor.assumeIsolated { self?.tick() }
        }
        let link = view.displayLink(target: ticker, selector: #selector(FrameTicker.step(_:)))
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 30, maximum: 60, preferred: 60)
        link.add(to: .main, forMode: .common)
        frames = link
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
        // 초당 60번 옮기는 동안은 자리를 기억하지 않는다 — 매번 설정 파일에 쓰게 된다.
        panel?.setFrameAutosaveName("")
    }

    private func stopRoaming() {
        roaming = false
        roamer = nil
        gaze = GazeTracker()
        goal = nil
        ledgesHere = []
        placed = nil
        dragging = false
        pose.switchAction(to: .stand)
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
        chat(now: now)
    }

    /// 혼잣말 — 새 말이 오면 6초 띄운다. 사람이 자리를 비웠으면(입력이 `GazeTracker.away` 넘게 없음) 버린다 — 아무도 못 본다.
    private func chat(now: TimeInterval) {
        if let chat = agents.chat, chat.id != chatSeen {
            chatSeen = chat.id
            if Self.idleSeconds() < GazeTracker.away {
                pose.chat = chat.text
                chatUntil = now + 6
            }
        }
        if pose.chat != nil, now > chatUntil { pose.chat = nil }
    }

    /// 포인터가 마지막으로 움직인(끌기 · 스크롤 포함) 뒤 흐른 초 — 권한 없이 읽는 시스템 입력 시계.
    private static func pointerIdle() -> Double {
        let types: [CGEventType] = [.mouseMoved, .leftMouseDragged, .scrollWheel, .leftMouseDown]
        return types.map { CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: $0) }.min() ?? .infinity
    }

    private static func keyIdle() -> Double {
        CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: .keyDown)
    }

    private static func idleSeconds() -> Double { min(pointerIdle(), keyIdle()) }

    /// 눈길 옆에 설 자리를 다시 정한다 — 앞 앱의 맨 앞 창은 방금 읽은 창 목록에서 주인 pid 로 찾는다(로키 자신은 목록에 없다).
    private func lookAround(_ roamer: Roamer, now: TimeInterval) {
        guard agents.follow else {
            if goal != nil || gaze.target != nil {
                gaze = GazeTracker()
                goal = nil
            }
            return
        }
        guard now - gazed >= 0.1 else { return }
        gazed = now
        let mouse = NSEvent.mouseLocation
        let front = NSWorkspace.shared.frontmostApplication?.processIdentifier
        let focus = front.flatMap { pid in seen.first { $0.pid == pid }?.box }
        let target = gaze.update(GazeInput(
            pointerX: mouse.x, pointerY: mouse.y, sincePointer: Self.pointerIdle(), sinceKey: Self.keyIdle(),
            focus: focus, rockyX: roamer.x, rockyY: roamer.y
        ))
        goal = target.flatMap { followGoal(gaze: $0, ledges: ledgesHere, fromX: roamer.x) }
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
            pose.switchAction(to: .fall)
            pose.shrunk = false
            hideTube()
            placed = origin
            dragging = down
            if down { return }
        }
        guard var roamer else { return }
        let screen = (NSScreen.screens.first { $0.frame.contains(foot) } ?? NSScreen.main)?.visibleFrame ?? .zero
        let area = Box(x: screen.minX, y: screen.minY, width: screen.width, height: screen.height)
        if now - scanned > 0.5, !scanning {
            let top = NSScreen.screens.first?.frame.maxY ?? 0
            if scanned == 0 {
                // 처음 한 번은 기다린다 — 창 목록 없이 걸으면 창 윗면에 서 있던 로키가 떨어진다.
                seen = Self.scanWindows(top: top)
            } else {
                // 창이 많으면 수 ms-수십 ms 걸린다 — 메인 스레드에서 읽으면 0.5초마다 한 번씩 걸음이 멈칫한다.
                scanning = true
                Task { [weak self] in
                    let found = await Task.detached(priority: .userInitiated) { Self.scanWindows(top: top) }.value
                    self?.seen = found
                    self?.scanning = false
                }
            }
            scanned = now
        }
        let boxes = windows
        ledgesHere = ledges(windows: boxes, screen: area)
        lookAround(roamer, now: now)
        roamer.step(
            dt: dt, ledges: ledgesHere, walls: walls(windows: boxes, screen: area),
            screen: area, mood: agents.mood.roamMood(during: pose.clip), goal: goal, random: { Double.random(in: 0 ..< 1) }
        )
        roamer.x = min(max(roamer.x, area.minX + Roamer.halfWidth), area.maxX - Roamer.halfWidth)
        self.roamer = roamer
        let next = NSPoint(x: roamer.x - Self.foot.x, y: roamer.y - Self.foot.y)
        if next != origin { panel.setFrameOrigin(next) }
        placed = next
        // 패널은 240×170 이지만 로키는 오른쪽 아래 96×88 뿐 — 그 밖의 클릭은 아래 창으로 흘려보낸다(말풍선이 뜬 동안은 패널 전체).
        let mouse = NSEvent.mouseLocation
        var hot = Self.figure(at: next).contains(mouse)
        if pose.chat != nil || agents.mood.bubble.map({ $0 != pose.dismissed }) == true {
            hot = NSRect(origin: next, size: Self.size).contains(mouse)
        }
        if panel.ignoresMouseEvents == hot { panel.ignoresMouseEvents = !hot }
        if pose.action != roamer.action { pose.switchAction(to: roamer.action) }
        if pose.facing != roamer.facing { pose.facing = roamer.facing }
        if pose.shrunk != roamer.insideTube { pose.shrunk = roamer.insideTube }
        showTube(roamer)
    }

    /// 패널이 `origin` 에 있을 때 로키 칸(패널 오른쪽 아래 96×88)의 화면 자리.
    private static func figure(at origin: NSPoint) -> NSRect {
        NSRect(x: origin.x + size.width - 96, y: origin.y, width: 96, height: 88)
    }

    /// 화면에 떠 있는 보통 창(레이어 0)들 — 앞에서 뒤 순서, AppKit 좌표로. 자기 창과 아주 작은 창은 뺀다.
    /// `top` 은 주 화면의 윗변 — `NSScreen` 은 메인 스레드에서 읽어 넘긴다(이 함수는 메인 스레드 밖에서 돈다).
    nonisolated private static func scanWindows(top: CGFloat) -> [SeenWindow] {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        guard let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else { return [] }
        let me = ProcessInfo.processInfo.processIdentifier
        return list.compactMap { info in
            guard (info[kCGWindowLayer as String] as? Int) == 0,
                  let pid = info[kCGWindowOwnerPID as String] as? Int32, pid != me,
                  (info[kCGWindowAlpha as String] as? Double ?? 1) > 0.1,
                  let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                  let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary),
                  rect.width >= 80, rect.height >= 40
            else { return nil }
            // CG 창 좌표는 주 화면 왼쪽 위가 원점이고 y 가 아래로 자란다.
            return SeenWindow(box: Box(x: rect.minX, y: top - rect.maxY, width: rect.width, height: rect.height), pid: pid)
        }
    }
}

/// 창 목록의 한 줄 — 자리와 주인 앱의 pid.
private struct SeenWindow: Sendable {
    var box: Box
    var pid: Int32
}

/// 디스플레이 링크의 박자를 받는 쪽 — `CADisplayLink` 의 target 은 Objective-C 객체여야 한다.
private final class FrameTicker: NSObject {
    private let action: () -> Void

    init(_ action: @escaping () -> Void) {
        self.action = action
    }

    @objc func step(_: CADisplayLink) {
        action()
    }
}

/// 돌아다니는 동안의 몸짓 — 컨트롤러가 쓰고 로키 그림이 읽는다.
@MainActor
@Observable
final class RoamPose {
    private(set) var action: RoamAction = .stand
    /// 바로 앞의 움직임과 바뀐 시각 — 걷다 서거나 떨어지다 내려앉을 때 두 자세를 섞는다(한 프레임에 갈아 끼우면 팔다리가 튄다).
    private(set) var previous: RoamAction = .stand
    private(set) var changed: Double = 0
    /// 패널이 보이는지 — 안 보이면 그림의 박자도 멈춘다.
    var visible = true
    var facing: Double = 1
    /// 튜브 안에서는 몸을 줄인다.
    var shrunk = false
    /// 서 있을 때의 몸짓, 착지 반동의 남은 정도, 점프 전 쪼그림.
    var clip = Clip(move: .mood, start: 0, length: 0, tempo: 1)
    var squash = 0.0
    var crouch = 0.0
    /// 누른 말풍선 — 같은 글이면 다시 띄우지 않고, 글이 바뀌면(다른 세션이 기다리면) 다시 뜬다.
    var dismissed: String?
    /// 잠깐 띄우는 혼잣말(`Chatter`) — 내 차례 말풍선이 떠 있으면 그 뒤로 숨는다. 누르면 닫힌다.
    var chat: String?

    func switchAction(to next: RoamAction) {
        guard next != action else { return }
        previous = action
        changed = Date.timeIntervalSinceReferenceDate
        action = next
    }
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
        case .awake: .awake
        case .resting, .unknown: .other
        }
    }

    /// 일하는 중·깨어 있으면 돌아다니고, 내 차례면 사람 곁에 서 있다 가끔 서성이고(크게 알리거나 손을 흔드는 동안은 서서 — 걷다가도 멈춘다),
    /// 연결 없음이면 그 자리에 서 있고, 다 쉬면 앉아 존다.
    func roamMood(during clip: Clip) -> RoamMood {
        switch self {
        case .working, .awake: .roam
        case .waiting: clip.move == .cheer || clip.move == .wave ? .stay : .pace
        case .unknown: .stay
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
                Bubble(text: bubble) { pose.dismissed = bubble }
            } else if let chat = pose.chat {
                Bubble(text: chat) { pose.chat = nil }
            }
            Rocky(
                mood: agents.mood, action: pose.action, previous: pose.previous, changed: pose.changed,
                facing: pose.facing, clip: pose.clip, squash: pose.squash, crouch: pose.crouch, visible: pose.visible
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
            Button(agents.follow ? "따라다니기 멈추기" : "따라다니기") { agents.follow.toggle() }
                .disabled(!agents.roam)
            Button("아바타 숨기기") { agents.showAvatar = false }
        }
        .animation(.easeInOut(duration: 0.3), value: agents.mood)
        .animation(.easeInOut(duration: 0.2), value: pose.dismissed)
        .animation(.easeInOut(duration: 0.3), value: pose.chat)
    }
}

/// 로키 머리 위 말풍선 — 누르면 닫힌다.
private struct Bubble: View {
    let text: String
    let close: () -> Void

    var body: some View {
        Text(text)
            .font(.caption)
            .lineLimit(3)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
            .frame(maxWidth: 220, alignment: .trailing)
            .onTapGesture(perform: close)
            .transition(.opacity)
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
        case .awake: "다 쉬고 있어요 — 곧 잘게요"
        case .resting: "다 쉬고 있어요"
        }
    }
}

/// 로키 — 시안 그림을 부위별로 잘라 다시 짜 맞춘 돌 친구(낮은 돔 몸통 · 세 손가락 팔 둘 · 다리 둘 · 얼굴).
/// 조각은 `RockyArt` 가, 순간의 자세(관절 각도 · 표정)는 `RockyPose` 가 정한다. 자세는 늘 오른쪽을 보는 것으로 짠다.
private struct Rocky: View {
    let mood: AvatarMood
    var action: RoamAction = .stand
    var previous: RoamAction = .stand
    var changed: Double = 0
    var facing: Double = 1
    var clip = Clip(move: .mood, start: 0, length: 0, tempo: 1)
    var squash = 0.0
    var crouch = 0.0
    var visible = true

    /// 움직임이 바뀔 때 앞 자세와 섞는 시간(초).
    static let blend = 0.2

    var body: some View {
        // 패널을 옮기는 박자(초당 60번)와 같게 — 30이면 패널은 60번 움직이는데 몸은 두 번에 한 번만 바뀌어 덜컥인다.
        TimelineView(.animation(minimumInterval: 1 / 60, paused: !moving || !visible)) { context in
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
        guard let art = RockyArt.shared else {
            // 그림을 못 읽으면 돌 하나라도.
            gc.fill(Path(ellipseIn: CGRect(x: 100, y: 170, width: 160, height: 130)), with: .color(.gray))
            return
        }
        var q = RockyPose(mood, action: action, t: t, clip: clip, squash: squash, crouch: crouch)
        let k = (t - changed) / Self.blend
        if previous != action, k < 1 {
            // 걷다 서기 · 떨어지다 내려앉기 — 앞 움직임의 자세에서 부드럽게 넘어온다. 통째 자세(뛰는 중 · 잠)는 섞을 관절이 없어 그대로 바꾼다.
            let before = RockyPose(mood, action: previous, t: t, clip: clip, squash: squash, crouch: crouch)
            if before.whole == nil, q.whole == nil { q = RockyPose.mix(before, q, RockyPose.smooth(k)) }
        }
        if q.gray { gc.addFilter(.grayscale(1)) }
        // 왼쪽을 보면 무대를 좌우로 뒤집는다.
        var figure = gc
        if facing < 0 {
            figure.translateBy(x: 360, y: 0)
            figure.scaleBy(x: -1, y: 1)
        }
        if let whole = q.whole {
            // 통째 포즈 — 그림의 아래 끝을 무대의 발 자리(y 312)에, 기울기와 눌림은 그 가운데를 축으로.
            figure.translateBy(x: 180, y: 312 + q.y)
            figure.rotate(by: .degrees(q.tilt))
            figure.scaleBy(x: RockyArt.scale, y: RockyArt.scale * q.squash)
            art.draw(whole, in: figure)
        } else {
            // 발바닥이 무대의 발 자리(y 312)에 오게 — 기울기와 눌림은 몸통 가운데를 축으로.
            figure.translateBy(x: 180, y: 312 - (RockyArt.sole - RockyArt.center.y) * RockyArt.scale + q.y)
            figure.rotate(by: .degrees(q.tilt))
            figure.scaleBy(x: RockyArt.scale, y: RockyArt.scale * q.squash)
            figure.translateBy(x: -RockyArt.center.x, y: -RockyArt.center.y)
            art.draw(q, in: figure)
        }
        if case .resting = mood, action == .sit || action == .stand {
            let rise = (t.truncatingRemainder(dividingBy: 3)) / 3
            gc.draw(
                Text("z").font(.system(size: 40, weight: .semibold)).foregroundColor(.secondary.opacity(1 - rise)),
                at: CGPoint(x: 296 + rise * 20, y: 170 - rise * 60)
            )
        }
    }
}

/// 한 순간의 자세. 팔의 `raise` 는 시안의 내린 팔(0)에서 몸 밖으로 드는 각도, 다리는 화면 시계 방향이 + — 왼다리 +, 오른다리 − 면 벌린다.
/// `tilt` 도 시계 방향(오른쪽을 볼 때 앞으로 숙임)이 +. `whole` 이 있으면 관절 대신 시안의 그 자세를 통째로 그린다.
private struct RockyPose {
    enum Face { case smile, happy, joy, focus, surprise }
    /// 손 — 시안의 내린 손, 점프할 때 편 손, 갸웃할 때 머리에 댄 손(오른손만 있다).
    enum Hand { case rest, open, curl }
    /// 통째로 그리는 시안의 자세 — 엎드려 자기, 머리를 긁으며 갸웃, 손을 흔들며 뛰기.
    enum Whole { case sleep, puzzled, jump }

    /// 팔. 손 모양이 바뀌는 동안은 `from` 의 손에서 `hand` 로 `fade`(0 … 1)만큼 겹쳐 바꾼다 — 조각을 한 프레임에 갈아 끼우면 손이 튄다.
    struct Arm {
        var raise = 0.0
        var hand = Hand.rest
        var from: Hand?
        var fade = 1.0

        /// 겹침이 끝난(0 이나 1) 팔은 보이는 손 하나로 접는다.
        var settled: Arm {
            guard let from, from != hand else { return Arm(raise: raise, hand: hand) }
            if fade <= 0 { return Arm(raise: raise, hand: from) }
            if fade >= 1 { return Arm(raise: raise, hand: hand) }
            return self
        }
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
    var whole: Whole?
    var gray = false

    /// 두 자세 사이 — 수는 섞고, 표정·손 모양처럼 섞을 수 없는 것은 절반을 넘으면 바꾼다.
    static func mix(_ a: RockyPose, _ b: RockyPose, _ w: Double) -> RockyPose {
        func lerp(_ x: Double, _ y: Double) -> Double { x + (y - x) * w }
        func arm(_ x: Arm, _ y: Arm) -> Arm {
            let raise = lerp(x.raise, y.raise)
            let a = x.settled, b = y.settled
            // 둘 다 한 손 모양이면 — 같으면 그대로, 다르면 섞는 정도만큼 겹쳐 바꾼다.
            if a.from == nil, b.from == nil {
                return a.hand == b.hand ? Arm(raise: raise, hand: a.hand) : Arm(raise: raise, hand: b.hand, from: a.hand, fade: RockyPose.smooth(w))
            }
            // 한쪽이 이미 바꾸는 중이면 가까운 쪽의 겹침을 그대로 쓴다.
            var out = w < 0.5 ? a : b
            out.raise = raise
            return out
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

    /// 0 … 1 을 처음과 끝이 느린 곡선으로 — 직선으로 섞으면 시작과 끝에서 팔이 툭 멈추거나 튀어 나간다.
    static func smooth(_ x: Double) -> Double {
        let u = min(max(x, 0), 1)
        return u * u * (3 - 2 * u)
    }

    /// 토막의 앞뒤 0.35초는 바탕 자세와 부드럽게 섞는다 — 0.2초 직선이던 때는 팔이 한두 프레임 만에 튀어 올라 끊겨 보였다.
    private static func ease(_ clip: Clip, _ t: Double) -> Double {
        let elapsed = t - clip.start
        let window = min(0.35, clip.length / 2)
        return smooth(min(elapsed / window, (clip.length - elapsed) / window))
    }

    /// 껑충 — h 가 0(땅)에서 1(가장 높이)으로 갈수록 몸이 뜨고 두 팔을 치켜들며 다리를 벌린다. 높이 뜨면 시안의 점프처럼 손을 편다.
    /// 손은 h 0.3-0.7 동안 겹쳐 편다 — 절반에서 조각을 갈아 끼우던 때는 0.3초마다 손이 툭툭 바뀌었다.
    private static func hop(_ h: Double) -> RockyPose {
        var q = RockyPose()
        q.y = -26 * h
        q.face = .joy
        let open = smooth((h - 0.3) / 0.4)
        q.armL = Arm(raise: 105 * h, hand: .open, from: .rest, fade: open)
        q.armR = Arm(raise: 105 * h, hand: .open, from: .rest, fade: open)
        q.legL = 20 * h
        q.legR = -20 * h
        return q
    }

    /// 껑충 한 번(0.6초) — 처음 0.15초는 땅을 딛고 쪼그렸다 펴고, 나머지 0.45초는 포물선으로 뜬다.
    /// 예전엔 높이를 sin 으로 바로 그려 땅에 닿는 순간 머물지 않고 되튀어 고무공처럼 통통 튀어 보였다.
    private static func bounce(_ local: Double) -> RockyPose {
        let contact = 0.15
        guard local >= contact else {
            let c = sin(.pi * local / contact)
            var q = hop(0)
            q.y = 6 * c
            q.squash = 1 - 0.08 * c
            q.legL = 6 * c
            q.legR = -6 * c
            return q
        }
        let u = (local - contact) / (0.6 - contact)
        return hop(4 * u * (1 - u))
    }

    /// 도는 중의 바탕 — 눈썹을 모은 얼굴로 앞을 본다.
    /// 오른팔은 망치를 쥔 채 20도 — 망치질이 쉬는 자리와 같아야 망치질과 쉬는 동작 사이에 팔이 튀지 않는다.
    private static let work: RockyPose = {
        var q = RockyPose()
        q.face = .focus
        q.look = 1
        q.armR = Arm(raise: 20)
        return q
    }()

    init() {}

    init(_ mood: AvatarMood, action: RoamAction, t: Double, clip: Clip, squash: Double, crouch: Double) {
        base(mood, action: action, t: t, clip: clip, crouch: crouch)
        if case .unknown = mood { gray = true }
        // 눈은 4초쯤에 한 번 깜빡인다.
        if whole == nil, face == .smile || face == .focus { blink = t.truncatingRemainder(dividingBy: 3.7) < 0.12 }
        // 착지 반동 — 쭈그렸다 편다. 통째 자세는 발을 땅에 붙인 채 눌리기만 한다.
        if squash > 0 {
            let a = sin(.pi * (1 - squash))
            self.squash *= 1 - 0.12 * a
            if whole == nil {
                y += 14 * a
                legL += 10 * a
                legR -= 10 * a
            }
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
            // 걸음마다 몸이 아주 살짝만 뜬다 — 박자가 초당 3번 넘어 크게 들썩이면 계속 통통 튀어 정신없다(2026-11-06 오너 지적).
            // sin² 이라 바닥에서 부드럽게 돌아선다(|sin| 은 바닥에서 뾰족하게 꺾여 공처럼 튀었다). 걸음은 기울기로 보인다.
            y = -1.2 * p * p
            tilt = 4 + 2 * p
            look = 1
            legL = 12 * p
            legR = 12 * p
            armL.raise = -8 + 14 * swing
            armR.raise = -8 - 14 * swing
            return
        case .jump:
            if crouch > 0 {
                // 뛰기 전 — 몸을 낮추고 팔을 몸에 붙인다.
                y = 14 * crouch
                squash = 1 - 0.14 * crouch
                face = .focus
                look = 1
                armL.raise = -15 * crouch
                armR.raise = -15 * crouch
                legL = 6 * crouch
                legR = -6 * crouch
            } else {
                // 뛰어오르는 중 — 시안의 점프(한 손을 흔들고 다리를 접은 모습) 그대로.
                whole = .jump
            }
            return
        case .fall:
            // 떨어지기 — 손을 펴고 허우적거리며 다리를 벌린다.
            face = .surprise
            armL = Arm(raise: 95 + 12 * sin(t * 22), hand: .open)
            armR = Arm(raise: 105 + 12 * sin(t * 22 + 1), hand: .open)
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
        case .awake:
            awake(t: t, clip: clip)
        case .resting:
            // 다 쉼 — 시안처럼 엎드려 잔다. 숨에 맞춰 등이 오르내린다.
            whole = .sleep
            squash = 1 + 0.015 * Self.breath(t)
        case .unknown:
            // 연결 없음 — 시안처럼 머리를 긁으며 갸웃(회색으로).
            whole = .puzzled
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
        // 손 흔들기 — 든 팔을 초당 1.6번, ±20도로 크게 젓고 몸이 반 박자 늦게 따라 흔들린다.
        // 예전엔 초당 2.2번 ±12도라 손이 흔드는 게 아니라 떨렸고, 박자가 시작 시각과 무관해 매번 다른 자리에서 튀어 나왔다.
        func wave(_ elapsed: Double, _ amount: Double) -> RockyPose {
            let beat = sin(2 * .pi * 1.6 * elapsed)
            let sway = sin(2 * .pi * 1.6 * elapsed - 1.2)
            // 몸은 기울기만 — 위아래로도 흔들면 초당 3번 넘게 들썩여 정신없다.
            var q = RockyPose()
            q.face = .happy
            q.tilt = 3 * sway
            q.armR = Arm(raise: 118 + 20 * amount * beat, hand: .open)
            q.legL = 4 + 2 * sway
            q.legR = -4 + 2 * sway
            return q
        }
        switch clip.move {
        case .watch:
            self = watch
        case .wave:
            // 팔이 다 올라간 뒤에 크게 젓기 시작하고, 내릴 때는 젓기를 먼저 줄인다.
            let w = Self.ease(clip, t)
            self = Self.mix(watch, wave(t - clip.start, w * w), w)
        default:
            // 크게 알리기(2.4초) — 한 번 껑충 뛰고 주먹 인사를 한 뒤 선다. 네 번씩 뛰던 때는 정신없었다(2026-11-06 오너 지적).
            let phase = t - clip.start
            if phase < 0.6 {
                self = Self.bounce(phase)
            } else {
                // 주먹 인사 — 앞으로 주먹을 내밀었다 당긴다(엄지척은 못 한다). 뛰기와 사이는 0.25초 동안 섞고, 끝나면 그대로 선다.
                let q = phase - 0.6
                var bump = RockyPose()
                bump.face = .joy
                bump.tilt = 4
                bump.armR = Arm(raise: 60 + 10 * sin(2 * .pi * q / 0.6))
                let w = Self.smooth(min(q / 0.25, (1.2 - q) / 0.25))
                self = Self.mix(Self.hop(0), bump, w)
            }
        }
    }

    /// 도는 중 — 박자(`tempo`)마다 오른주먹을 들어 내리치고, 몇 번 치고 나면 쉬는 동작 하나를 한다.
    private mutating func working(t: Double, clip: Clip) {
        guard let move = Self.breakMove(clip, t: t, from: Self.work) else {
            hammer(t: t, clip: clip)
            return
        }
        self = Self.mix(Self.work, move, Self.ease(clip, t))
    }

    /// 깨어 있음(다 쉬지만 방금까지 일했다) — 망치 없이 숨 쉬며 마우스 쪽을 보다가 쉬는 동작 하나를 한다.
    private mutating func awake(t: Double, clip: Clip) {
        var calm = RockyPose()
        calm.y = -1.5 * Self.breath(t)
        calm.look = 1
        calm.armL.raise = 3 * Self.breath(t - 0.25)
        calm.armR.raise = calm.armL.raise
        guard let move = Self.breakMove(clip, t: t, from: calm) else {
            self = calm
            return
        }
        self = Self.mix(calm, move, Self.ease(clip, t))
    }

    /// 쉬는 동작의 자세 — `base` 위에 고친다. 쉬는 동작이 아닌 토막이면 nil.
    private static func breakMove(_ clip: Clip, t: Double, from base: RockyPose) -> RockyPose? {
        let u = clip.progress(at: t)
        var move = base
        switch clip.move {
        case .look:
            // 두리번 — 몸을 앞뒤로 기울이며 눈을 굴린다.
            let turn = sin(2 * .pi * u)
            move.y = -2
            move.tilt = 7 * turn
            move.look = turn
            move.face = .smile
        case .wipe:
            // 땀 닦기 — 시안의 갸웃처럼 오른손을 머리에 대고 문지른다.
            move.face = .happy
            move.armR = Arm(raise: 115 + 6 * sin(2 * .pi * 2.5 * u), hand: .curl)
        case .shoulder:
            // 팔 돌리기 — 두 팔을 엇갈려 크게 돌린다.
            let s = sin(2 * .pi * 2 * u)
            move.face = .smile
            move.armL = Arm(raise: 40 + 40 * s)
            move.armR = Arm(raise: 40 - 40 * s)
        case .stomp:
            // 발 구르기 — 발을 번갈아 들었다 쿵.
            let s = sin(2 * .pi * 3 * u)
            move.y = 3 * s * s
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
            // 내려다보기 — 창 끝 쪽으로 몸을 숙이고 팔을 앞으로 내민다(방향은 컨트롤러가 끝 쪽으로 돌린다).
            // 숙인 만큼 몸을 올려 앞발이 발 자리를 지키게 — 그대로 숙이면 앞발이 무대 아래로 잘린다.
            move.face = .surprise
            move.y = -12
            move.tilt = 14 + 3 * sin(t * 2)
            move.armL.raise = 10
            move.armR = Arm(raise: 30)
            move.legL = 6
            move.legR = -6
        default:
            return nil
        }
        return move
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
            // 내리친 충격 — 0.05 박자에 걸쳐 차올랐다 가라앉는다(한 프레임에 꺾이면 덜컥인다).
            let k = (phase - 0.7) / 0.05
            hit = k * exp(1 - k)
        }
        // 몸은 반만 따라 움직인다 — 내리칠 때마다 크게 쿵 하면 망치질 내내 통통 튄다(2026-11-06 오너 지적).
        self = Self.work
        y = -2 * lift + 2.5 * hit
        tilt = 3 * lift + 4 * hit
        armR = Arm(raise: 20 + 100 * lift)
        legL = 4 * hit
        legR = -4 * hit
    }
}

/// 로키의 모양 — 시안 그림(mac-avatar/concept-face)에서 윤곽대로 잘라 낸 조각 PNG(`Resources/rocky/`)를 짜 맞춘다.
/// 좌표는 시안의 서 있는 그림을 잘라 낸 칸의 1배 픽셀로, 몸통 가운데가 (258, 160), 발바닥이 y 298.75 다. 무대에는 `scale` 배로 줄여 올린다.
/// 팔·눈·입처럼 다른 자세에서 가져온 조각은 그 자세를 잘라 낸 칸의 좌표를 그대로 들고, 붙는 점(`pivot` · `from`)으로 옮겨 놓는다.
/// 조각은 .app 이면 `Contents/Resources/rocky/`, `swift run` 이면 SwiftPM 리소스 번들에서 읽는다 —
/// `Bundle.module` 은 .app 안에서 번들을 못 찾으면 멈추므로 쓰지 않는다.
private final class RockyArt {
    static let scale = 0.6
    static let center = CGPoint(x: 258, y: 160)
    static let sole = 298.75

    static let shared: RockyArt? = RockyArt()

    /// 조각 — 이름은 PNG 파일 이름, `origin` 은 조각을 잘라 낸 칸에서 PNG 의 왼쪽 위 자리.
    enum Sprite: String, CaseIterable {
        case body, legL, legR
        case armLRest = "armL-rest", armRRest = "armR-rest", armLOpen = "armL-open", armROpen = "armR-open"
        case armRCurl = "armR-curl"
        case eyeL = "stand-eyeL", eyeR = "stand-eyeR", mouth = "stand-mouth"
        case happyEyeL = "jump-eyeL", happyEyeR = "jump-eyeR", openMouth = "jump-mouth"
        case brow = "puzzled-browL", lidL = "sleep-lidL", lidR = "sleep-lidR"
        case jump = "whole-jump", puzzled = "whole-puzzled", sleep = "whole-sleep"

        var origin: CGPoint {
            switch self {
            case .body: CGPoint(x: 82, y: 10)
            case .legL: CGPoint(x: 113, y: 203)
            case .legR: CGPoint(x: 305, y: 203)
            case .armLRest: CGPoint(x: 10, y: 138)
            case .armRRest: CGPoint(x: 379, y: 138)
            case .armLOpen: CGPoint(x: 10, y: 68)
            case .armROpen: CGPoint(x: 383, y: 10)
            case .armRCurl: CGPoint(x: 306, y: 11)
            case .eyeL: CGPoint(x: 176, y: 125)
            case .eyeR: CGPoint(x: 288, y: 125)
            case .mouth: CGPoint(x: 237, y: 150)
            case .happyEyeL: CGPoint(x: 185, y: 170)
            case .happyEyeR: CGPoint(x: 295, y: 148)
            case .openMouth: CGPoint(x: 241, y: 184)
            case .brow: CGPoint(x: 145, y: 109)
            case .lidL: CGPoint(x: 120, y: 239)
            case .lidR: CGPoint(x: 237, y: 246)
            case .jump: CGPoint(x: 10, y: 10)
            case .puzzled: CGPoint(x: 11, y: 11)
            case .sleep: CGPoint(x: 10, y: 100)
            }
        }
    }

    private struct Piece {
        let image: Image
        let size: CGSize
    }

    private let pieces: [Sprite: Piece]

    private init?() {
        let swiftpm = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("RockyMac_RockyMac.bundle")
        let bundles = [Bundle.main] + [swiftpm.flatMap { Bundle(url: $0) }].compactMap { $0 }
        var pieces: [Sprite: Piece] = [:]
        for sprite in Sprite.allCases {
            let url = bundles.lazy
                .compactMap { $0.url(forResource: sprite.rawValue, withExtension: "png", subdirectory: "rocky") }
                .first
            guard let url, let cg = NSImage(contentsOf: url)?.cgImage(forProposedRect: nil, context: nil, hints: nil)
            else { return nil }
            // 무대에서 3배쯤 줄여 그리므로 보간을 높여야 테두리가 계단지지 않는다.
            pieces[sprite] = Piece(
                image: Image(decorative: cg, scale: 1).interpolation(.high),
                size: CGSize(width: cg.width, height: cg.height)
            )
        }
        self.pieces = pieces
    }

    private func paint(_ sprite: Sprite, in gc: GraphicsContext) {
        guard let piece = pieces[sprite] else { return }
        gc.draw(piece.image, in: CGRect(origin: sprite.origin, size: piece.size))
    }

    /// 통째 자세 — 그림의 가운데 아래(발 끝, 잘라 낸 칸의 좌표)를 원점에 둔다.
    func draw(_ whole: RockyPose.Whole, in gc: GraphicsContext) {
        var g = gc
        switch whole {
        case .jump:
            g.translateBy(x: -253.38, y: -340.75)
            paint(.jump, in: g)
        case .puzzled:
            g.translateBy(x: -210, y: -297.25)
            paint(.puzzled, in: g)
        case .sleep:
            g.translateBy(x: -255.62, y: -359.5)
            paint(.sleep, in: g)
        }
    }

    /// 뒤에서 앞으로 — 다리, 팔, 몸통, 얼굴. 팔다리는 몸통 뒤로 들어가는 자리까지 그려 두었으니 돌려도 틈이 없다.
    func draw(_ q: RockyPose, in gc: GraphicsContext) {
        leg(gc, .legL, hip: CGPoint(x: 164, y: 236), angle: q.legL)
        leg(gc, .legR, hip: CGPoint(x: 352, y: 236), angle: q.legR)
        arm(gc, left: true, q.armL)
        arm(gc, left: false, q.armR)
        paint(.body, in: gc)
        for piece in Self.face(q.face, blink: q.blink, look: q.look) {
            var g = gc
            g.translateBy(x: piece.to.x, y: piece.to.y)
            g.rotate(by: .degrees(piece.turn))
            g.scaleBy(x: piece.flip ? -piece.scale : piece.scale, y: piece.scale)
            g.translateBy(x: -piece.from.x, y: -piece.from.y)
            paint(piece.sprite, in: g)
        }
    }

    private func leg(_ gc: GraphicsContext, _ sprite: Sprite, hip: CGPoint, angle: Double) {
        var g = gc
        g.translateBy(x: hip.x, y: hip.y)
        g.rotate(by: .degrees(angle))
        g.translateBy(x: -hip.x, y: -hip.y)
        paint(sprite, in: g)
    }

    /// 팔 조각 — 어깨에 붙는 점(`pivot`, 조각을 잘라 낸 칸의 좌표)과, 그 팔이 시안에서 든 각도(`theta`, 똑바로 내린 팔에서 몸 밖으로).
    private struct Limb {
        let sprite: Sprite
        let pivot: CGPoint
        let theta: Double
    }

    /// 머리에 댄 손은 오른손만 있어 왼손은 내린 손으로 그린다.
    private static func limb(left: Bool, _ hand: RockyPose.Hand) -> Limb {
        switch (left, hand) {
        case (true, .open): Limb(sprite: .armLOpen, pivot: CGPoint(x: 114, y: 176), theta: 135.82)
        case (true, _): Limb(sprite: .armLRest, pivot: CGPoint(x: 100, y: 176), theta: 37.33)
        case (false, .open): Limb(sprite: .armROpen, pivot: CGPoint(x: 413, y: 135), theta: 152.53)
        case (false, .curl): Limb(sprite: .armRCurl, pivot: CGPoint(x: 338, y: 112), theta: 152.24)
        case (false, .rest): Limb(sprite: .armRRest, pivot: CGPoint(x: 415.5, y: 176), theta: 37.33)
        }
    }

    /// 팔 — 바라는 각도(내린 팔 37.33도 + `raise`)와 조각의 각도의 차만큼 어깨를 축으로 돌린다.
    /// 화면은 y 가 아래로 자라 시계 방향이 + 이므로, 왼팔은 + 로 돌리면 들리고 오른팔은 − 로 돌리면 들린다.
    private func arm(_ gc: GraphicsContext, left: Bool, _ arm: RockyPose.Arm) {
        guard let from = arm.from, from != arm.hand, arm.fade < 1 else { return paintArm(gc, left: left, arm.hand, arm.raise) }
        // 겹쳐 바꾸기 — 앞 절반은 옛 손이 다 보인 채 새 손이 짙어지고, 뒤 절반은 새 손이 다 보인 채 옛 손이 옅어진다
        // (둘 다 반투명이면 그 사이로 몸통이 비친다).
        var old = gc, new = gc
        old.opacity = min(1, 2 * (1 - arm.fade))
        new.opacity = min(1, 2 * arm.fade)
        paintArm(old, left: left, from, arm.raise)
        paintArm(new, left: left, arm.hand, arm.raise)
    }

    private func paintArm(_ gc: GraphicsContext, left: Bool, _ hand: RockyPose.Hand, _ raise: Double) {
        let limb = Self.limb(left: left, hand)
        let shoulder = left ? CGPoint(x: 100, y: 176) : CGPoint(x: 415.5, y: 176)
        let turn = 37.33 + raise - limb.theta
        var g = gc
        g.translateBy(x: shoulder.x, y: shoulder.y)
        g.rotate(by: .degrees(left ? turn : -turn))
        g.translateBy(x: -limb.pivot.x, y: -limb.pivot.y)
        paint(limb.sprite, in: g)
    }

    /// 얼굴 조각 하나 — 조각의 `from` 점을 서 있는 얼굴의 `to` 점에 놓고 `turn` 도 돌려 `scale` 배로(`flip` 이면 좌우로 뒤집어).
    private struct FacePiece {
        let sprite: Sprite
        let from: CGPoint
        let to: CGPoint
        var turn = 0.0
        var scale = 1.0
        var flip = false
    }

    /// 표정 — 시안 네 자세의 눈·눈썹·입을 서 있는 얼굴 자리(눈 가운데 (202.96, 150.72) · (313.89, 150.68), 입 (262.5, 167.37))에 옮긴다.
    /// 점프 그림은 11.5도 기울어 있어 그만큼 되돌리고, 웃는 눈은 위쪽 테두리를 맞춘다. 눈과 눈썹은 보는 쪽으로 3.5 옮긴다.
    private static func face(_ face: RockyPose.Face, blink: Bool, look: Double) -> [FacePiece] {
        let eyeL = CGPoint(x: 202.96 + 3.5 * look, y: 150.72)
        let eyeR = CGPoint(x: 313.89 + 3.5 * look, y: 150.68)
        var out: [FacePiece] = []
        if blink {
            // 자는 그림의 감은 눈을 조금 줄여 3 위로.
            let lidL = CGPoint(x: eyeL.x, y: eyeL.y - 3), lidR = CGPoint(x: eyeR.x, y: eyeR.y - 3)
            out.append(FacePiece(sprite: .lidL, from: CGPoint(x: 150.75, y: 246.75), to: lidL, turn: 1.5, scale: 0.9))
            out.append(FacePiece(sprite: .lidR, from: CGPoint(x: 267.375, y: 254), to: lidR, turn: -1.5, scale: 0.9))
        } else if face == .happy || face == .joy {
            out.append(FacePiece(sprite: .happyEyeL, from: CGPoint(x: 211.75, y: 195.45), to: eyeL, turn: 11.5))
            out.append(FacePiece(sprite: .happyEyeR, from: CGPoint(x: 321, y: 173.2), to: eyeR, turn: 11.5))
        } else {
            out.append(FacePiece(sprite: .eyeL, from: CGPoint(x: 202.96, y: 150.72), to: eyeL))
            out.append(FacePiece(sprite: .eyeR, from: CGPoint(x: 313.89, y: 150.68), to: eyeR))
        }
        if face == .focus {
            // 갸웃하는 그림의 왼눈썹을 두 눈 위에(오른쪽은 뒤집어서) — 안쪽 끝이 조금 내려가게.
            let brow = CGPoint(x: 169.2, y: 121.7)
            let browL = CGPoint(x: eyeL.x + 7.38, y: eyeL.y - 28.51)
            let browR = CGPoint(x: eyeR.x - 7.38, y: eyeR.y - 28.51)
            out.append(FacePiece(sprite: .brow, from: brow, to: browL, turn: 6.9, scale: 1.047))
            out.append(FacePiece(sprite: .brow, from: brow, to: browR, turn: -6.9, scale: 1.047, flip: true))
        }
        let mouth = CGPoint(x: 262.5, y: 167.37)
        switch face {
        case .joy:
            out.append(FacePiece(sprite: .openMouth, from: CGPoint(x: 269.375, y: 201.15), to: mouth, turn: 11.5))
        case .surprise:
            // 벌린 입을 줄여 동그랗게 놀란 입으로.
            out.append(FacePiece(sprite: .openMouth, from: CGPoint(x: 269.875, y: 203.75), to: mouth, turn: 11.5, scale: 0.55))
        default:
            out.append(FacePiece(sprite: .mouth, from: mouth, to: mouth))
        }
        return out
    }
}
