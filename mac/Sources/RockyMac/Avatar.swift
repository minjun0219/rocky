// 떠 있는 아바타 — 화면 구석의 작은 로키(프로젝트 헤일메리의 에리디언을 귀여운 돌 친구로 그린 것)가 에이전트 상태를 알린다.
// 내 차례인 세션이 있으면 껑충 뛰고 주먹 인사를 하며 말풍선에 기다리는 것을 띄우고, 도는 세션이 있으면 주먹으로 두드리고, 다 쉬면 엎드려 잠든다.
// 창은 늘 위에 뜨는 패널이고 포커스를 가져가지 않는다. 돌아다니기를 켜면 창 윗면을 걷고, 끌어다 놓으면 떨어지고,
// 튜브를 놓아 창 윗면으로 오르거나 다른 창으로 건너간다. 두 번 누르면 웹 에이전트 탭이 열리고, 말풍선은 누르면 닫힌다.
// 따라다니기를 켜면 사람이 보는 곳(멈춘 포인터 · 입력 중인 앞 창) 옆으로 느긋하게 다가가고, 가끔 말풍선으로 세션 소식을 혼잣말한다.
import AppKit
import Carbon
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
    /// 두 발 — 걸을 때 한 발씩 디딘다(`RockyKit.Feet`).
    private var feet: Feet?
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
    /// 비밀번호를 칠 때 눈 가리기.
    private var eyes = EyeCover()
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
    /// 화면이 잠들었는지 — 아무도 못 보는 동안은 박자를 멈춘다(`RockyKit.framesRun`).
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
        // 화면이 붙거나 빠지면 다시 본다 — 하나도 없으면 멈추고, 패널이 화면 밖으로 밀려났으면 걸어 들어오게 돌린다.
        observers.append(NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.updateVisibility() }
        })
    }

    /// 보이는지 따라 박자와 그림을 멈추고 다시 돌린다 — 판정은 `RockyKit.framesRun`(화면이 잠들었거나 하나도 없거나,
    /// 패널이 화면 안에서 다 가려지면 멈춘다. 화면 밖으로 밀려났으면 돌아다니는 동안은 돌린다).
    private func updateVisibility() {
        guard let panel else { return }
        let screens = NSScreen.screens
        let visible = framesRun(
            screensAsleep: screensAsleep, screenCount: screens.count,
            onScreen: screens.contains { $0.frame.intersects(panel.frame) },
            visible: panel.occlusionState.contains(.visible), roaming: roaming
        )
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
        // 알림은 바뀔 때만 온다 — 처음부터 안 보이는 패널(화면 없는 맥 · 잠긴 화면에서 뜸)은 여기서 재야 멈춘다.
        updateVisibility()
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
        updateVisibility()
    }

    private func stopRoaming() {
        roaming = false
        roamer = nil
        feet = nil
        pose.stance = .rest
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
        updateVisibility()
    }

    private func tick() {
        guard let panel else { return }
        let now = ProcessInfo.processInfo.systemUptime
        let dt = min(now - last, 0.1)
        last = now
        // 비밀번호 칸 — 시스템의 보안 입력이 켜졌는지는 권한 없이 읽는다(어느 앱이 켰는지는 모른다).
        let cover = eyes.update(secure: IsSecureEventInputEnabled() == true, now: now, dt: dt)
        if pose.cover != cover { pose.cover = cover }
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
    /// 끝에 이번 프레임의 장면(기분 · 움직임 · 몸짓)을 적는다 — 바뀌었으면 그림이 앞 자세에서 섞어 넘어간다(`RockyKit.Shifts`).
    private func choreograph(_ panel: NSPanel) {
        let now = Date.timeIntervalSinceReferenceDate
        let here = roamer.map { spot(x: $0.x, y: $0.y, ledges: ledgesHere) } ?? .floor
        // 돌멩이면 돌멩이에만 있는 동작도 고른다.
        moves.extras = agents.avatarStyle != .concept
        let clip = moves.update(
            now: now, mood: agents.mood.moveMood, spot: here,
            random: { Double.random(in: 0 ..< 1) }
        )
        if pose.clip != clip { pose.clip = clip }
        let squash = roamer?.squash ?? 0
        if pose.squash != squash { pose.squash = squash }
        pose.show(
            Moment(
                mood: agents.mood.poseMood, action: pose.action, clip: clip,
                crouch: roamer?.crouch ?? 0, flight: roamer?.flight ?? 0
            ),
            now: now
        )
        // 보는 쪽은 한 바퀴에 한 번만 정한다 — 걷는 쪽에, 서 있을 때의 몸짓(마우스 쪽 보기 · 내려다보기)을 얹어서.
        // 걸음과 몸짓이 따로 바꾸면 돌아서기(`RoamPose.turn`)가 바퀴마다 처음부터 다시 시작해 끝내 돌아서지 못한다.
        var facing = roamer?.facing ?? pose.facing
        if pose.action == .stand || pose.action == .sit {
            switch clip.move {
            case .watch: facing = NSEvent.mouseLocation.x >= panel.frame.minX + Self.foot.x ? 1 : -1
            case .peek: if case let .edge(toward) = here { facing = toward }
            default: break
            }
        }
        pose.turn(to: facing)
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
        // 눈을 가린 동안은 걷지 않고(앞을 못 본다) 눈길도 따라가지 않는다 — 비밀번호 칸으로 다가가면 안 된다.
        var mood = agents.mood.roamMood(during: pose.clip)
        if eyes.covering, mood == .roam { mood = .stay }
        roamer.step(
            dt: dt, ledges: ledgesHere, walls: walls(windows: boxes, screen: area),
            screen: area, mood: mood, goal: eyes.covering ? nil : goal, random: { Double.random(in: 0 ..< 1) }
        )
        roamer.x = min(max(roamer.x, area.minX + Roamer.halfWidth), area.maxX - Roamer.halfWidth)
        self.roamer = roamer
        let next = NSPoint(x: roamer.x - Self.foot.x, y: roamer.y - Self.foot.y)
        if next != origin { panel.setFrameOrigin(next) }
        // 놓인 자리는 패널에서 다시 읽는다 — AppKit 은 창 위치를 정수 pt 로 맞춰 `next` 의 소수점을 버린다. `next` 를 기억하면
        // 다음 바퀴에 그 차이를 사람이 끌어 옮긴 것으로 읽어 매 걸음 그 자리에 떨어뜨리고, 로키는 한 발도 못 나간다(2026-11-09 실측).
        placed = panel.frame.origin
        // 패널은 240×170 이지만 로키는 오른쪽 아래 96×88 뿐 — 그 밖의 클릭은 아래 창으로 흘려보낸다(말풍선이 뜬 동안은 패널 전체).
        let mouse = NSEvent.mouseLocation
        var hot = Self.figure(at: next).contains(mouse)
        if pose.chat != nil || agents.mood.bubble.map({ $0 != pose.dismissed }) == true {
            hot = NSRect(origin: next, size: Self.size).contains(mouse)
        }
        if panel.ignoresMouseEvents == hot { panel.ignoresMouseEvents = !hot }
        if pose.action != roamer.action { pose.switchAction(to: roamer.action) }
        // 발 — 발판 위(뛰기 전 쪼그림까지)면 한 발씩 디디고, 떠 있으면 몸과 같이 옮기며 엉덩이 아래로 끌어온다.
        var feet = self.feet ?? Feet(x: roamer.x)
        switch roamer.action {
        case .walk, .stand, .sit: feet.step(x: roamer.x, dt: dt)
        case .jump where roamer.flight == 0: feet.step(x: roamer.x, dt: dt)
        case .jump, .fall, .tube: feet.airborne(x: roamer.x, dt: dt)
        }
        self.feet = feet
        let stance = feet.stance(at: roamer.x)
        if pose.stance != stance { pose.stance = stance }
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
    /// 그리는 장면의 층(`RockyKit.Shifts`) — 기분 · 움직임 · 몸짓이 바뀌면 그림이 앞 자세에서 섞어 넘어간다(한 프레임에 갈아 끼우면
    /// 팔다리 · 표정이 튀고, 서 있다가 한 프레임 만에 엎드렸다). 컨트롤러가 처음 적기 전에는 없다.
    private(set) var shifts: Shifts?
    /// 그림을 멈춰도 되는지 — 연결이 없어 갸웃한 채 다 넘어왔으면 그림이 바뀌지 않는다.
    private(set) var still = false
    /// 패널이 보이는지 — 안 보이면 그림의 박자도 멈춘다.
    var visible = true
    /// 보는 쪽 — 바꾸면(`turn`) 그림이 `Rocky.turn` 초 동안 몸을 좁혔다 펴며 돌아선다. 돌아선 시각과 그 앞에 보이던 쪽.
    private(set) var facing: Double = 1
    private(set) var turned = -Double.infinity
    private(set) var wasFacing: Double = 1
    /// 두 발의 자리(`RockyKit.Feet`) — 디딘 발은 화면에 붙어 있다.
    var stance = Stance.rest
    /// 튜브 안에서는 몸을 줄인다.
    var shrunk = false
    /// 서 있을 때의 몸짓과 착지 반동의 남은 정도.
    var clip = Clip(move: .mood, start: 0, length: 0, tempo: 1)
    var squash = 0.0
    /// 누른 말풍선 — 같은 글이면 다시 띄우지 않고, 글이 바뀌면(다른 세션이 기다리면) 다시 뜬다.
    var dismissed: String?
    /// 잠깐 띄우는 혼잣말(`Chatter`) — 내 차례 말풍선이 떠 있으면 그 뒤로 숨는다. 누르면 닫힌다.
    var chat: String?
    /// 두 손으로 눈을 가린 정도(0 … 1, `EyeCover`).
    var cover = 0.0

    func switchAction(to next: RoamAction) {
        guard next != action else { return }
        action = next
    }

    /// 이번 프레임의 장면을 적는다 — 같은 장면이면 흐르는 값(쪼그림 · 뜬 정도)만 고치고, 바뀌었으면 층을 얹는다.
    /// 값이 그대로면 쓰지 않는다(쓰면 그림이 매 바퀴 다시 짜인다).
    func show(_ moment: Moment, now: Double) {
        var next = shifts ?? Shifts(moment)
        next.update(moment, now: now)
        if next != shifts { shifts = next }
        let top = next.current
        let settled = next.layers.count == 1 && top.mood == .unknown && top.action == .stand && now - top.since > Rocky.settle
        if still != settled { still = settled }
    }

    /// 돌아선다 — 아직 돌아서는 앞 절반(그림이 옛 쪽을 보는 동안)에 다시 돌면 보이던 쪽에서 이어 돈다. 보이던 쪽으로 돌아오면 그냥 멈춘다.
    func turn(to next: Double) {
        guard next != facing else { return }
        let now = Date.timeIntervalSinceReferenceDate
        let shown = now - turned < Rocky.turn / 2 ? wasFacing : facing
        facing = next
        if shown == next {
            turned = -.infinity
        } else {
            wasFacing = shown
            turned = now
        }
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
                shifts: pose.shifts ?? Shifts(Moment(mood: agents.mood.poseMood, action: pose.action, clip: pose.clip)),
                facing: pose.facing, turned: pose.turned, wasFacing: pose.wasFacing, stance: pose.stance,
                squash: pose.squash, visible: pose.visible, still: pose.still, cover: pose.cover,
                look: agents.avatarStyle
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
            Picker("모양", selection: Binding(get: { agents.avatarStyle }, set: { agents.avatarStyle = $0 })) {
                ForEach(AvatarStyle.allCases, id: \.self) { Text($0.title).tag($0) }
            }
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
    let shifts: Shifts
    var facing: Double = 1
    var turned = -Double.infinity
    var wasFacing: Double = 1
    var stance = Stance.rest
    var squash = 0.0
    var visible = true
    /// 그림이 더 바뀌지 않는다(연결 없이 갸웃한 채) — 박자를 멈춘다.
    var still = false
    /// 두 손으로 눈을 가린 정도(0 … 1, `RockyPose.coverEyes`).
    var cover = 0.0
    /// 그림체 — 원작 조각(`RockyArt`)이나 돌멩이(`PebbleRocky`). 자세(`RockyPose`)는 둘이 같다.
    var look = AvatarStyle.concept

    /// 돌아서는 시간과 그동안 몸을 좁히는 정도 — 가장 좁을 때(가운데) 좌우를 바꾼다. 한 프레임에 뒤집던 때는 툭 튀어 보였다(2026-11-06).
    static let turn = 0.22
    static let squeeze = 0.14
    /// 연결이 끊긴 뒤 이만큼 지나면 갸웃 그림으로 다 넘어와 그림이 더 바뀌지 않는다(`RoamPose.still`).
    static let settle = 1.0

    var body: some View {
        // 패널을 옮기는 박자(초당 60번)와 같게 — 30이면 패널은 60번 움직이는데 몸은 두 번에 한 번만 바뀌어 덜컥인다.
        // 쉬는 중에도 숨은 쉰다 — 멈추는 건 연결이 없어 갸웃한 채 다 넘어왔을 때뿐.
        TimelineView(.animation(minimumInterval: 1 / 60, paused: still || !visible)) { context in
            let t = context.date.timeIntervalSinceReferenceDate
            Canvas { gc, size in
                draw(in: &gc, size: size, t: t)
            }
        }
    }

    /// 한 층의 자세. 원작은 돌멩이 동작(`Move.isExtra`)을 하지 않는다 — 돌멩이에서 원작으로 바꾼 직후 남은 토막은 두리번으로 대신한다.
    private func pose(_ moment: Moment, t: Double) -> RockyPose {
        var moment = moment
        if look == .concept, moment.clip.move.isExtra {
            let clip = moment.clip
            moment.clip = Clip(move: .look, start: clip.start, length: clip.length, tempo: clip.tempo)
        }
        return RockyPose(moment, t: t, style: look)
    }

    private func draw(in gc: inout GraphicsContext, size: CGSize, t: Double) {
        // 무대는 360×330 — 프레임에 맞춰 늘린다.
        gc.scaleBy(x: size.width / 360, y: size.height / 330)
        // 층마다 그 장면의 자세를 짜서 아래 층부터 섞는다 — 늦게 얹은 층일수록 덜 섞여 들어와 있다.
        var q = pose(shifts.layers[0].moment, t: t)
        for layer in shifts.layers.dropFirst() {
            q = RockyPose.mix(q, pose(layer.moment, t: t), RockyPose.smooth(layer.weight(at: t)))
        }
        q.land(squash)
        // 돌아서는 중이면 몸을 좁혔다 펴고, 가운데까지는 앞에 보던 쪽을 본다.
        var facing = self.facing, width = 1.0
        let turning = (t - turned) / Self.turn
        if turning >= 0, turning < 1 {
            width = 1 - Self.squeeze * sin(.pi * turning)
            if turning < 0.5 { facing = wasFacing }
        }
        if cover > 0 { q.coverEyes(cover) }
        // 발 — 디딘 발은 땅에 붙어 있게 다리를 그 자리로 옮기고, 든 발 반대쪽으로 무게를 싣는다.
        let feet = stance.drawn(facing: facing)
        q.plant(left: feet.left, right: feet.right)
        // 그림자는 발이 무언가를 딛고 있을 때만 — 뛰어오르거나 떨어지면 옅어지고 내려앉으면 짙어진다.
        if q.shadow > 0 {
            gc.fill(
                Path(ellipseIn: CGRect(x: 180 - 85, y: 314 - 8, width: 170, height: 16)),
                with: .color(.black.opacity(0.12 * q.shadow))
            )
        }
        let art = RockyArt.shared
        if look == .concept, art == nil {
            // 그림을 못 읽으면 돌 하나라도.
            gc.fill(Path(ellipseIn: CGRect(x: 100, y: 170, width: 160, height: 130)), with: .color(.gray))
            return
        }
        if q.gray > 0 { gc.addFilter(.grayscale(q.gray)) }
        // 왼쪽을 보면 무대를 좌우로 뒤집는다.
        var figure = gc
        if facing < 0 {
            figure.translateBy(x: 360, y: 0)
            figure.scaleBy(x: -1, y: 1)
        }
        if width != 1 {
            figure.translateBy(x: 180, y: 0)
            figure.scaleBy(x: width, y: 1)
            figure.translateBy(x: -180, y: 0)
        }
        let show = q.whole == nil ? 0 : q.show
        // 발바닥이 무대의 발 자리(y 312)에 오게 — 기울기와 눌림은 몸통 가운데를 축으로.
        let sole = (RockyArt.sole - RockyArt.center.y) * RockyArt.scale
        if look == .pebble {
            // 돌멩이 — 조각과 같은 칸 좌표로 관절만 그린다. 겹쳐 바꿀 통째 그림이 없어 통째 자세도 관절로 짜 두었다(`PebbleRocky.twin`).
            // 통째 자세는 원작 그림처럼 발 자리를 축으로 눌린다 — 몸통 가운데를 축으로 누르면 발이 뜨니 그 자세가 보이는 만큼 내린다.
            // 잘 때는 팔다리를 다 넣고(원작 그림이 짙어지는 동안 옅어진다) 바닥에 내려앉아 그냥 돌이 된다.
            let sink = sole * (1 - q.squash) * show
            figure.translateBy(x: 180 + q.x, y: 312 - sole + q.y + sink)
            figure.rotate(by: .degrees(q.tilt))
            figure.scaleBy(x: RockyArt.scale, y: RockyArt.scale * q.squash)
            figure.translateBy(x: -RockyArt.center.x, y: -RockyArt.center.y)
            PebbleRocky.draw(q, limbs: q.whole == .sleep ? 1 - show : 1, in: figure)
        } else if let art {
            // 관절 자세와 시안의 통째 그림 — 겹쳐 바꾸는 동안 앞 절반은 관절이 다 보인 채 그림이 짙어지고, 뒤 절반은 그림이 다 보인 채
            // 관절이 옅어진다(둘 다 반투명이면 그 사이로 뒤가 비친다). 그림은 늘 관절 위에 얹는다.
            if show < 1 {
                var rig = figure
                rig.translateBy(x: 180 + q.x, y: 312 - sole + q.y)
                rig.rotate(by: .degrees(q.tilt))
                rig.scaleBy(x: RockyArt.scale, y: RockyArt.scale * q.squash)
                rig.translateBy(x: -RockyArt.center.x, y: -RockyArt.center.y)
                let opacity = min(1, 2 * (1 - show))
                if opacity < 1 {
                    // 조각이 겹친 자리(몸통 뒤의 팔다리)가 비치지 않게 한 장으로 모아 옅게 한다.
                    rig.opacity = opacity
                    rig.drawLayer { layer in art.draw(q, in: layer) }
                } else {
                    art.draw(q, in: rig)
                }
            }
            if let whole = q.whole, show > 0 {
                // 통째 그림 — 아래 끝을 무대의 발 자리(y 312)에. 그 쌍둥이 자세에서 벗어난 만큼(숨 · 착지 자세로 가기)만 같이 움직인다.
                let anchor = RockyPose.twin(whole)
                var sprite = figure
                sprite.translateBy(x: 180 + q.x - anchor.x, y: 312 + q.y - anchor.y)
                sprite.rotate(by: .degrees(q.tilt - anchor.tilt))
                sprite.scaleBy(x: RockyArt.scale, y: RockyArt.scale * q.squash / anchor.squash)
                sprite.opacity = min(1, 2 * show)
                art.draw(whole, in: sprite)
            }
        }
        // 춤추면 음표, 하트면 하트가 머리 위로 떠오른다(돌멩이).
        let top = shifts.current
        let clip = top.clip
        if look != .concept, top.action == .stand || top.action == .sit, t >= clip.start, t < clip.end,
           let mark = clip.move == .dance ? "♪" : clip.move == .heart ? "♥" : nil {
            let rise = ((t - clip.start).truncatingRemainder(dividingBy: 1.2)) / 1.2
            gc.draw(
                Text(mark).font(.system(size: 44, weight: .bold))
                    .foregroundColor((clip.move == .heart ? Color.pink : Color.secondary).opacity(1 - rise)),
                at: CGPoint(x: 290 + rise * 16, y: 150 - rise * 60)
            )
        }
        if q.whole == .sleep, show > 0 {
            let rise = (t.truncatingRemainder(dividingBy: 3)) / 3
            gc.draw(
                Text("z").font(.system(size: 40, weight: .semibold)).foregroundColor(.secondary.opacity((1 - rise) * show)),
                at: CGPoint(x: 296 + rise * 20, y: 170 - rise * 60)
            )
        }
    }
}

/// 한 순간의 자세. 팔의 `raise` 는 시안의 내린 팔(0)에서 몸 밖으로 드는 각도, 다리는 화면 시계 방향이 + — 왼다리 +, 오른다리 − 면 벌린다.
/// `tilt` 도 시계 방향(오른쪽을 볼 때 앞으로 숙임)이 +. `whole` 이 보이면(`show`) 관절 위에 시안의 그 자세를 통째로 겹쳐 그린다
/// (돌멩이는 그 자세도 관절로 짜 두어 관절만 그린다 — `PebbleRocky.twin`).
struct RockyPose {
    /// 표정 — `sleep` 은 감은 눈(자는 그림의 눈꺼풀), `wink` 는 왼눈만 감는다(돌인 척하기 — 돌멩이에만 있다).
    enum Face { case smile, happy, joy, focus, surprise, sleep, wink }
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

    /// 몸을 옆으로 옮기는 만큼(무대 단위, 보는 쪽이 +) — 쌍둥이 자세를 통째 그림에 맞추려고.
    var x = 0.0
    var y = 0.0
    var tilt = 0.0
    var squash = 1.0
    /// 눈동자가 보는 쪽(−1 … 1).
    var look = 0.0
    var face = Face.smile
    /// 표정을 바꾸는 동안은 `faceFrom` 에서 `face` 로 `faceFade`(0 … 1)만큼 겹쳐 바꾼다 — 눈 · 입 조각을 한 프레임에 갈아 끼우면 표정이 튄다.
    var faceFrom: Face?
    var faceFade = 1.0
    var blink = false
    var armL = Arm()
    var armR = Arm()
    var legL = 0.0
    var legR = 0.0
    /// 다리 조각을 옮기는 만큼(조각 칸) — 발을 디딜 자리에 놓으려고. 엉덩이 위쪽은 몸통 뒤라 옮겨도 틈이 없다.
    var stepL = CGSize.zero
    var stepR = CGSize.zero
    /// 윗몸(팔 · 몸통 · 얼굴)만 엉덩이 줄 가운데를 축으로 기울이는 각도(시계 방향 +)와 들어 올리는 높이(조각 칸, 위가 −) — 디딘 다리는 그대로 둔다.
    var lean = 0.0
    var rise = 0.0
    /// 시안의 통째 그림과 그것이 보이는 정도(0 … 1) — 1 이면 그림만, 그 사이면 관절 자세(그 그림의 쌍둥이 가까이) 위에 겹친다.
    var whole: Whole?
    var show = 0.0
    /// 회색(연결 없음)과 그림자의 짙기(0 … 1).
    var gray = 0.0
    var shadow = 1.0
    /// 눈 가리기(0 … 1) — 어깨를 안쪽 위로 올리고 팔을 몸 앞으로 그린다(`coverEyes`).
    var shy = 0.0
    /// 팔다리를 몸 안에 넣었나(구르기 · 돌인 척하기) — 돌멩이만 따른다.
    var tuck = false

    /// 통째 그림이 들 때 — 관절은 앞 75% 동안 그 그림의 쌍둥이 자세로 가고 그림은 뒤 40% 동안 짙어진다.
    /// 날 때 — 그림은 앞 40% 동안 옅어지고 관절은 25% 부터 움직인다. 겹쳐 보이는 동안 두 모습이 거의 같은 자리에 있게.
    private static let entering = (rig: 0.0 ... 0.75, show: 0.6 ... 1.0)
    private static let leaving = (rig: 0.25 ... 1.0, show: 0.0 ... 0.4)

    /// 표정이 다 바뀌었으면 겹침을 접는다.
    private var settledFace: (face: Face, from: Face?, fade: Double) {
        guard let from = faceFrom, from != face, faceFade < 1 else { return (face, nil, 1) }
        return faceFade <= 0 ? (from, nil, 1) : (face, from, faceFade)
    }

    /// 두 자세 사이 — 수는 섞고, 표정 · 손 모양은 섞는 정도만큼 겹쳐 바꾼다. 통째 그림이 들고 날 때는 관절과 그림을 차례로(`entering` · `leaving`).
    static func mix(_ a: RockyPose, _ b: RockyPose, _ w: Double) -> RockyPose {
        let sa = a.whole == nil ? 0 : a.show, sb = b.whole == nil ? 0 : b.show
        var wr = w, whole: Whole?, show = 0.0
        if sa == 0, sb == 0 {
            whole = nil
        } else if a.whole == b.whole {
            whole = a.whole
            show = sa + (sb - sa) * w
        } else if sa == 0 {
            wr = span(w, entering.rig)
            whole = b.whole
            show = sb * span(w, entering.show)
        } else if sb == 0 {
            wr = span(w, leaving.rig)
            whole = a.whole
            show = sa * (1 - span(w, leaving.show))
        } else if w < 0.5 {
            // 다른 통째 그림 둘(자다가 연결이 끊김) — 앞 그림이 다 옅어진 뒤 관절을 거쳐 뒤 그림이 짙어진다.
            whole = a.whole
            show = sa * (1 - smooth(w / 0.3))
        } else {
            whole = b.whole
            show = sb * smooth((w - 0.7) / 0.3)
        }
        func lerp(_ x: Double, _ y: Double) -> Double { x + (y - x) * wr }
        func arm(_ x: Arm, _ y: Arm) -> Arm {
            let raise = lerp(x.raise, y.raise)
            let a = x.settled, b = y.settled
            // 둘 다 한 손 모양이면 — 같으면 그대로, 다르면 섞는 정도만큼 겹쳐 바꾼다.
            if a.from == nil, b.from == nil {
                return a.hand == b.hand ? Arm(raise: raise, hand: a.hand) : Arm(raise: raise, hand: b.hand, from: a.hand, fade: RockyPose.smooth(wr))
            }
            // 한쪽이 이미 바꾸는 중이면 가까운 쪽의 겹침을 그대로 쓴다.
            var out = wr < 0.5 ? a : b
            out.raise = raise
            return out
        }
        var out = wr < 0.5 ? a : b
        out.x = lerp(a.x, b.x)
        out.y = lerp(a.y, b.y)
        out.tilt = lerp(a.tilt, b.tilt)
        out.squash = lerp(a.squash, b.squash)
        out.look = lerp(a.look, b.look)
        out.armL = arm(a.armL, b.armL)
        out.armR = arm(a.armR, b.armR)
        out.legL = lerp(a.legL, b.legL)
        out.legR = lerp(a.legR, b.legR)
        out.stepL = CGSize(width: lerp(a.stepL.width, b.stepL.width), height: lerp(a.stepL.height, b.stepL.height))
        out.stepR = CGSize(width: lerp(a.stepR.width, b.stepR.width), height: lerp(a.stepR.height, b.stepR.height))
        out.lean = lerp(a.lean, b.lean)
        out.rise = lerp(a.rise, b.rise)
        out.whole = whole
        out.show = show
        out.gray = a.gray + (b.gray - a.gray) * w
        out.shadow = a.shadow + (b.shadow - a.shadow) * w
        // 표정 — 둘 다 한 표정이면 같으면 그대로, 다르면 섞는 정도만큼 겹쳐 바꾼다. 한쪽이 바꾸는 중이면 가까운 쪽의 겹침을 쓴다.
        let fa = a.settledFace, fb = b.settledFace
        if fa.from == nil, fb.from == nil {
            out.face = fb.face
            out.faceFrom = fa.face == fb.face ? nil : fa.face
            out.faceFade = fa.face == fb.face ? 1 : smooth(wr)
        } else {
            let near = wr < 0.5 ? fa : fb
            out.face = near.face
            out.faceFrom = near.from
            out.faceFade = near.fade
        }
        return out
    }

    /// 0 … 1 의 `range` 구간에서 처음과 끝이 느린 곡선으로 0 → 1.
    static func span(_ w: Double, _ range: ClosedRange<Double>) -> Double {
        smooth((w - range.lowerBound) / (range.upperBound - range.lowerBound))
    }

    /// 발 디딤(`RockyKit.Feet`)을 얹는다 — 두 발은 그림의 왼다리 · 오른다리(앞이 +). 다리를 디딜 자리로 옮겨 그쪽으로 기울이고,
    /// 발을 들면 윗몸을 반대쪽으로 3도 기울여 무게를 옮기며 아주 조금(0.4pt) 들고, 든 발 쪽 팔을 벌려 균형을 잡는다.
    /// 들썩임은 든 높이의 제곱(`sin²`)이라 발을 들고 디딜 때 바닥에서 꺾이지 않는다. 통째 그림만 보이면 얹을 다리가 없다.
    mutating func plant(left: Footing, right: Footing) {
        guard whole == nil || show < 1 else { return }
        let l = RockyArt.place(left: true, left), r = RockyArt.place(left: false, right)
        legL += l.angle
        stepL.width += l.step.width
        stepL.height += l.step.height
        legR += r.angle
        stepR.width += r.step.width
        stepR.height += r.step.height
        let hl = left.lift / Feet.height, hr = right.lift / Feet.height, up = max(hl, hr)
        lean += 3 * (hl - hr)
        rise -= 0.4 * RockyArt.cellsPerPoint * up * up
        armL.raise += 10 * hl - 4 * hr
        armR.raise += 10 * hr - 4 * hl
    }

    /// 숨 — 서로 나누어떨어지지 않는 주기의 사인 둘을 더해 박자가 쉽게 되풀이되지 않게.
    private static func breath(_ t: Double) -> Double {
        0.6 * sin(t * 1.9) + 0.4 * sin(t * 3.1 + 1)
    }

    /// 두 손으로 눈 가리기 — 편 손을 몸 앞으로 140도 모아 올리고 어깨를 안쪽 위로 끌어올린다. 손 아래 눈은 감는다.
    /// 시안 조각만으로 짠 자세다(새로 그리지 않는다) — 각도와 어깨 자리는 조각을 짜 맞춰 본 시트로 골랐다(2026-11-06).
    /// 통째 자세(잠 · 갸웃 · 뛰기)면 가리는 앞 절반 동안 서 있는 자세로 돌아온다 — 통째 그림에서 날 때처럼 그림이 옅어진 뒤 일어난다(`mix`).
    mutating func coverEyes(_ amount: Double) {
        if whole != nil {
            var stand = RockyPose()
            stand.gray = gray
            stand.shadow = shadow
            self = Self.mix(self, stand, Self.span(amount, 0 ... 0.5))
        }
        let s = Self.smooth(amount)
        func up(_ arm: Arm) -> Arm {
            let from = arm.settled
            let raise = from.raise + (Self.coverRaise - from.raise) * s
            return from.hand == .open ? Arm(raise: raise, hand: .open) : Arm(raise: raise, hand: .open, from: from.hand, fade: s)
        }
        armL = up(armL)
        armR = up(armR)
        shy = s
        look = 0
        if amount > 0.6 { blink = true }
    }

    static let coverRaise = -140.0

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

    /// 깨어 있을 때 서서 숨 쉬는 자세 — 마우스 쪽을 본다(지켜보기 · 깨어 있음의 바탕, 잠들기 · 갸웃의 출발).
    private static func calm(_ t: Double) -> RockyPose {
        var q = RockyPose()
        q.y = -1.5 * breath(t)
        q.look = 1
        q.armL.raise = 3 * breath(t - 0.25)
        q.armR.raise = q.armL.raise
        return q
    }

    /// 쌍둥이 자세 — 시안의 통째 그림과 겹쳐 놓았을 때 몸통 · 팔 · 다리가 거의 같은 자리에 오는 관절 자세(프로젝트 파일
    /// `mac-avatar/pose/` 의 `tune.js` 로 겹쳐 보며 맞췄다). 통째 그림과 겹쳐 바꾸는 동안 관절은 이 자세 가까이에 있고,
    /// 통째 그림은 이 자세의 x · y · tilt · squash 에서 벗어난 만큼만 같이 움직인다(숨 쉬기).
    /// 돌멩이는 겹쳐 바꿀 통째 그림이 없어 그 자세를 관절로 짠 것(`PebbleRocky.twin`)이 쌍둥이다 — 관절이 곧장 그 자세로 섞여 간다.
    static func twin(_ whole: Whole, style: AvatarStyle = .concept) -> RockyPose {
        if style == .pebble { return PebbleRocky.twin(whole) }
        var q = RockyPose()
        switch whole {
        case .sleep:
            // 다리는 몸통 뒤로 접어 넣고(올려서 가린다), 몸통을 조금 낮춰 눌러 엎드린 몸과 겹친다. 팔은 바닥으로 늘어뜨린다.
            q.face = .sleep
            q.x = -4
            q.y = 10.6
            q.tilt = -1.5
            q.squash = 0.925
            q.armL.raise = 12
            q.armR.raise = 4
            q.legL = -20
            q.legR = 20
            q.stepL = CGSize(width: 30, height: -60)
            q.stepR = CGSize(width: -30, height: -60)
        case .jump:
            q.face = .joy
            q.x = 10
            q.y = -8
            q.tilt = -11.5
            q.armL = Arm(raise: 108, hand: .open)
            q.armR = Arm(raise: 122, hand: .open)
            q.legL = 18
            q.legR = 8
        case .puzzled:
            // 오른손은 머리 옆까지만 — 시안처럼 머리 위로 올리면 주먹이 몸통 뒤로 숨는다.
            q.face = .focus
            q.x = 6
            q.tilt = -9
            q.armL.raise = -2
            q.armR = Arm(raise: 135, hand: .curl)
            q.legL = 2
            q.legR = -24
        }
        return q
    }

    /// 잠들기 — 기분이 바뀐 뒤 0-0.5초 주저앉으며 눈을 감고, 0.35-1.15초 엎드려 쌍둥이 자세로 가고, 1.0-1.4초 시안의 잠 그림으로
    /// 겹쳐 바꾼다. 그 뒤엔 숨만 쉰다(등이 오르내린다). 서 있다가 한 프레임 만에 엎드리던 때는 툭 튀었다(2026-11-06).
    private static let lie = (sit: 0.5, flat: 0.35 ... 1.15, show: 1.0 ... 1.4)
    private static func lieDown(_ e: Double, t: Double, style: AvatarStyle) -> RockyPose {
        let b = breath(t)
        var flat = twin(.sleep, style: style)
        flat.squash *= 1 + 0.015 * b
        flat.whole = .sleep
        guard e < lie.show.upperBound else {
            flat.show = 1
            return flat
        }
        var sit = RockyPose()
        sit.face = .sleep
        sit.y = 20 + 1.5 * b
        sit.squash = 0.9
        sit.legL = 32
        sit.legR = -32
        sit.armL.raise = -8
        sit.armR.raise = -8
        flat.whole = nil
        var q = mix(calm(t), sit, smooth(e / lie.sit))
        if e > lie.flat.lowerBound { q = mix(q, flat, span(e, lie.flat)) }
        q.whole = .sleep
        q.show = span(e, lie.show)
        return q
    }

    /// 갸웃 — 연결이 끊긴 뒤 0-0.45초 오른손을 머리 옆으로 올리며 눈썹을 모으고, 0.3-0.6초 시안의 갸웃 그림으로 겹쳐 바꾼다.
    /// 회색은 `init` 이 0.6초에 걸쳐 입힌다.
    private static func puzzle(_ e: Double, t: Double, style: AvatarStyle) -> RockyPose {
        let target = twin(.puzzled, style: style)
        var q = e >= 0.6 ? target : mix(calm(t), target, smooth(e / 0.45))
        q.whole = .puzzled
        q.show = span(e, 0.3 ... 0.6)
        return q
    }

    /// 점프 — 쪼그림은 앞 60% 동안 다 내려가 버틴다. 뛰면 뜬 정도 0.1 까지 쌍둥이 자세로 펴고 0.08-0.2 에 시안의 점프 그림으로 겹쳐 바꾼다
    /// (관절이 쌍둥이 자세에 닿은 뒤에 겹쳐야 두 모습이 어긋나 보이지 않는다). 내려앉기 전 0.72-0.84 에 다시 관절로 겹쳐 바꾸고
    /// 0.82-1 동안 다리를 아래로 뻗는다. 그림자는 뜨면서 옅어진다. 쪼그린 자세에서 한 프레임 만에 점프 그림으로 바꾸던 때는 툭 튀었다.
    private static let leap = (down: 0.6, unfold: 0.1, show: 0.08 ... 0.2, hide: 0.72 ... 0.84, reach: 0.82 ... 1.0)
    private static func crouched(_ c: Double) -> RockyPose {
        // 뛰기 전 — 몸을 낮추고 팔을 몸에 붙인다.
        var q = RockyPose()
        q.y = 14 * c
        q.squash = 1 - 0.14 * c
        q.face = .focus
        q.look = 1
        q.armL.raise = -15 * c
        q.armR.raise = -15 * c
        q.legL = 6 * c
        q.legR = -6 * c
        return q
    }

    private static func air(_ u: Double, style: AvatarStyle) -> RockyPose {
        var reach = RockyPose()
        reach.face = .joy
        reach.y = -6
        reach.armL = Arm(raise: 95, hand: .open)
        reach.armR = Arm(raise: 95, hand: .open)
        reach.legL = 8
        reach.legR = -8
        var q = mix(crouched(1), twin(.jump, style: style), smooth(u / leap.unfold))
        q = mix(q, reach, span(u, leap.reach))
        q.whole = .jump
        q.show = min(span(u, leap.show), 1 - span(u, leap.hide))
        q.shadow = 1 - smooth(u / 0.12)
        return q
    }

    init() {}

    init(_ moment: Moment, t: Double, style: AvatarStyle = .concept) {
        base(moment, t: t, style: style)
        // 연결 없음 — 0.6초에 걸쳐 회색이 된다(떨어지거나 끌려가도).
        if moment.mood == .unknown { gray = Self.smooth((t - moment.since) / 0.6) }
        // 눈은 4초쯤에 한 번 깜빡인다.
        if whole == nil || show < 1, faceFrom == nil, face == .smile || face == .focus {
            blink = t.truncatingRemainder(dividingBy: 3.7) < 0.12
        }
    }

    /// 착지 반동 — 쭈그렸다 편다. 통째 그림은 발을 땅에 붙인 채 눌리기만 한다.
    mutating func land(_ squash: Double) {
        guard squash > 0 else { return }
        let a = sin(.pi * (1 - squash))
        let rig = whole == nil ? 1 : 1 - show
        self.squash *= 1 - 0.12 * a
        y += 14 * a * rig
        legL += 10 * a * rig
        legR -= 10 * a * rig
    }

    private mutating func base(_ moment: Moment, t: Double, style: AvatarStyle) {
        switch moment.action {
        case .walk:
            // 걷기 — 다리와 무게는 발 디딤(`plant`)이 옮긴다. 여기선 가는 쪽을 보며 윗몸을 조금 숙이고 팔을 살짝 내린다.
            // 예전엔 두 다리를 시간 박자로 같은 쪽으로만 흔들어 발이 몸과 같이 미끄러졌다(2026-11-06 캡처 실측).
            look = 1
            lean = 2
            armL.raise = -4
            armR.raise = -4
            return
        case .tube:
            // 튜브 안에서 기어가기 — 줄어든 채 빨리 지나가니 발을 디디지 않고 뒤뚱뒤뚱 몸을 흔들며 팔을 젓는다.
            // 팔은 몸보다 한 박자 늦게 따라온다.
            let phase = 2 * .pi * t / 0.4 + 2.5 * sin(t * 0.5)
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
            // 관 안이라 그림자가 없다.
            shadow = 0
            return
        case .jump:
            // 쪼그렸다가(앞 60% 에 다 내려가 버틴다) 뛰어 시안의 점프 그림으로 겹쳐 바꾸고, 내려앉기 전에 다시 관절로 다리를 뻗는다.
            self = moment.flight > 0
                ? Self.air(moment.flight, style: style) : Self.crouched(Self.smooth(min(moment.crouch / Self.leap.down, 1)))
            return
        case .fall:
            // 떨어지기 — 손을 펴고 허우적거리며 다리를 벌린다. 디딘 데가 없어 그림자도 없다.
            face = .surprise
            armL = Arm(raise: 95 + 12 * sin(t * 22), hand: .open)
            armR = Arm(raise: 105 + 12 * sin(t * 22 + 1), hand: .open)
            legL = 30
            legR = -30
            shadow = 0
            return
        case .stand, .sit:
            break
        }
        switch moment.mood {
        case .waiting:
            waiting(t: t, clip: moment.clip)
        case .working:
            working(t: t, clip: moment.clip)
        case .awake:
            awake(t: t, clip: moment.clip)
        case .resting:
            // 다 쉼 — 주저앉았다 엎드려 시안처럼 잔다. 숨에 맞춰 등이 오르내린다.
            self = Self.lieDown(t - moment.since, t: t, style: style)
        case .unknown:
            // 연결 없음 — 손을 머리로 올려 시안처럼 머리를 긁으며 갸웃(회색은 `init` 이).
            self = Self.puzzle(t - moment.since, t: t, style: style)
        }
    }

    /// 내 차례 — 처음 몇 초는 껑충 뛰고 주먹 인사를 하고, 그 뒤엔 마우스 쪽을 보고 숨 쉬다가 가끔 손을 흔든다.
    private mutating func waiting(t: Double, clip: Clip) {
        let watch = Self.calm(t)
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
        let calm = Self.calm(t)
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
        case .roll:
            // 데굴데굴 — 팔다리를 넣고 바닥에 내려앉아 한 바퀴 구른다.
            move.tuck = true
            move.face = .surprise
            move.y = 20
            // 토막의 75% 에 다 구르고 0도로 적는다 — 끝의 0.35초는 바탕 자세와 섞으니(`ease`) 360도로 남기면 섞으며 거꾸로 한 바퀴 되돌아 돈다.
            move.tilt = (360 * smooth((u - 0.15) / 0.6)).truncatingRemainder(dividingBy: 360)
        case .shake:
            // 몸 털기 — 빠르게 좌우로 흔들며 팔을 벌려 턴다.
            let s = sin(2 * .pi * 7 * u)
            move.face = .focus
            move.tilt = 8 * s
            move.armL = Arm(raise: 30 + 10 * s, hand: .open)
            move.armR = Arm(raise: 30 - 10 * s, hand: .open)
        case .stretch:
            // 기지개 — 눈을 감고 두 팔을 쭉 뻗으며 몸을 늘인다.
            let e = sin(.pi * u)
            move.face = .joy
            move.blink = true
            move.armL = Arm(raise: 150 * e, hand: .open)
            move.armR = Arm(raise: 150 * e, hand: .open)
            move.squash = 1 + 0.08 * e
            move.y = -6 * e
        case .dance:
            // 춤 — 좌우로 몸을 흔들며 팔을 번갈아 올리고 발을 구른다.
            let s = sin(2 * .pi * 2 * u)
            move.face = .happy
            move.tilt = 10 * s
            move.y = -3 * s * s
            move.armL = Arm(raise: 90 + 40 * s, hand: .open)
            move.armR = Arm(raise: 90 - 40 * s, hand: .open)
            move.legL = 8 + 10 * max(0, s)
            move.legR = -8 - 10 * max(0, -s)
        case .hide:
            // 돌인 척하기 — 팔다리를 넣고 바닥에 내려앉아 한쪽 눈만 슬쩍 뜬다.
            move.tuck = true
            move.face = .wink
            move.y = 27
        case .yawn:
            // 하품 — 눈을 감고 입을 벌리며 한 팔을 쭉 뻗는다.
            let e = sin(.pi * u)
            move.face = .surprise
            move.blink = true
            move.armL = Arm(raise: 120 * e, hand: .open)
            move.tilt = -4 * e
            move.squash = 1 + 0.04 * e
        case .clap:
            // 박수 — 두 손을 펴 앞에서 짝짝.
            let s = sin(2 * .pi * 4 * u)
            move.face = .joy
            move.armL = Arm(raise: 80 + 15 * s, hand: .open)
            move.armR = Arm(raise: 80 + 15 * s, hand: .open)
        case .heart:
            // 하트 — 두 주먹을 머리 위로 모은다(하트는 화면이 그린다).
            move.face = .happy
            move.armL = Arm(raise: 160, hand: .curl)
            move.armR = Arm(raise: 160, hand: .curl)
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
    /// 1pt(패널)가 조각 칸으로 몇인지 — 무대 360 이 96pt 이고 조각은 무대에 `scale` 배로 올라간다.
    static let cellsPerPoint = 360.0 / 96.0 / scale
    /// 다리 — 엉덩이(돌리는 축)와 엉덩이에서 본 발바닥 가운데(조각의 아래 끝에서 잰 것).
    private static let hips = (left: CGPoint(x: 164, y: 236), right: CGPoint(x: 352, y: 236))
    private static let soles = (left: CGVector(dx: 154.8 - 164, dy: 298 - 236), right: CGVector(dx: 360.86 - 352, dy: 298 - 236))
    /// 발을 1pt 옮길 때 다리를 기울이는 각도 — 나머지는 조각을 옮긴다.
    private static let legLean = 1.6

    /// 발을 엉덩이 아래 제자리에서 `footing.offset`(pt, 앞이 +)만큼 옮기고 `footing.lift`(pt)만큼 든 다리 — 기울기(도)와 조각을 옮길 만큼.
    /// 발바닥 가운데가 그 자리에 오게 기울여 생긴 어긋남을 옮김으로 메운다.
    static func place(left: Bool, _ footing: Footing) -> (angle: Double, step: CGSize) {
        let sole = left ? soles.left : soles.right
        let angle = -legLean * footing.offset
        let c = cos(angle * .pi / 180), s = sin(angle * .pi / 180)
        let turned = CGVector(dx: sole.dx * c - sole.dy * s, dy: sole.dx * s + sole.dy * c)
        return (angle, CGSize(
            width: footing.offset * cellsPerPoint - (turned.dx - sole.dx),
            height: -footing.lift * cellsPerPoint - (turned.dy - sole.dy)
        ))
    }

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
        leg(gc, .legL, hip: Self.hips.left, angle: q.legL, step: q.stepL)
        leg(gc, .legR, hip: Self.hips.right, angle: q.legR, step: q.stepR)
        // 윗몸 — 엉덩이 줄 가운데(258, 236)를 축으로 기울이고 들어 올린다. 다리는 디딘 자리에 그대로.
        var upper = gc
        upper.translateBy(x: 258, y: 236 + q.rise)
        upper.rotate(by: .degrees(q.lean))
        upper.translateBy(x: -258, y: -236)
        // 눈을 가리는 동안은 팔을 몸 앞에 — 몸 뒤에 두면 손이 얼굴에 닿지 못한다. 조금만 올렸을 때(0.15 아래)는 그대로 뒤에.
        let front = q.shy > 0.15
        if !front {
            arm(upper, left: true, q.armL, shy: q.shy)
            arm(upper, left: false, q.armR, shy: q.shy)
        }
        paint(.body, in: upper)
        paintFace(q, in: upper)
        if front {
            arm(upper, left: true, q.armL, shy: q.shy)
            arm(upper, left: false, q.armR, shy: q.shy)
        }
    }

    /// 얼굴 — 표정을 바꾸는 중이면 겹쳐 바꾼다. 두 표정에 같은 조각(같은 눈 · 입)은 한 번만 다 그리고, 다른 조각만 앞 절반은 옛 것이 다 보인 채
    /// 새 것이 짙어지고 뒤 절반은 새 것이 다 보인 채 옛 것이 옅어진다. 같은 조각까지 둘 다 반투명으로 겹치면 눈 · 입이 흐려 보인다.
    private func paintFace(_ q: RockyPose, in upper: GraphicsContext) {
        let now = Self.face(q.face, blink: q.blink, look: q.look)
        guard let from = q.faceFrom, from != q.face, q.faceFade < 1 else { return paint(now, in: upper) }
        let was = Self.face(from, blink: q.blink, look: q.look)
        var old = upper, new = upper
        old.opacity = min(1, 2 * (1 - q.faceFade))
        new.opacity = min(1, 2 * q.faceFade)
        paint(was.filter { !now.contains($0) }, in: old)
        paint(now.filter { was.contains($0) }, in: upper)
        paint(now.filter { !was.contains($0) }, in: new)
    }

    private func paint(_ face: [FacePiece], in gc: GraphicsContext) {
        guard gc.opacity > 0 else { return }
        for piece in face {
            var g = gc
            g.translateBy(x: piece.to.x, y: piece.to.y)
            g.rotate(by: .degrees(piece.turn))
            g.scaleBy(x: piece.flip ? -piece.scale : piece.scale, y: piece.scale)
            g.translateBy(x: -piece.from.x, y: -piece.from.y)
            paint(piece.sprite, in: g)
        }
    }

    private func leg(_ gc: GraphicsContext, _ sprite: Sprite, hip: CGPoint, angle: Double, step: CGSize) {
        var g = gc
        g.translateBy(x: hip.x + step.width, y: hip.y + step.height)
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
    private func arm(_ gc: GraphicsContext, left: Bool, _ arm: RockyPose.Arm, shy: Double) {
        guard let from = arm.from, from != arm.hand, arm.fade < 1 else {
            return paintArm(gc, left: left, arm.hand, arm.raise, shy: shy)
        }
        // 겹쳐 바꾸기 — 앞 절반은 옛 손이 다 보인 채 새 손이 짙어지고, 뒤 절반은 새 손이 다 보인 채 옛 손이 옅어진다
        // (둘 다 반투명이면 그 사이로 몸통이 비친다).
        var old = gc, new = gc
        old.opacity = min(1, 2 * (1 - arm.fade))
        new.opacity = min(1, 2 * arm.fade)
        paintArm(old, left: left, from, arm.raise, shy: shy)
        paintArm(new, left: left, arm.hand, arm.raise, shy: shy)
    }

    /// `shy` 만큼 어깨를 안쪽으로 15, 위로 30 옮긴다 — 내린 어깨 자리에서는 140도 올려도 손이 볼에 머문다.
    private func paintArm(_ gc: GraphicsContext, left: Bool, _ hand: RockyPose.Hand, _ raise: Double, shy: Double) {
        let limb = Self.limb(left: left, hand)
        let inward = 15 * shy, lift = 30 * shy
        let shoulder = left ? CGPoint(x: 100 + inward, y: 176 - lift) : CGPoint(x: 415.5 - inward, y: 176 - lift)
        let turn = 37.33 + raise - limb.theta
        var g = gc
        g.translateBy(x: shoulder.x, y: shoulder.y)
        g.rotate(by: .degrees(left ? turn : -turn))
        g.translateBy(x: -limb.pivot.x, y: -limb.pivot.y)
        paint(limb.sprite, in: g)
    }

    /// 얼굴 조각 하나 — 조각의 `from` 점을 서 있는 얼굴의 `to` 점에 놓고 `turn` 도 돌려 `scale` 배로(`flip` 이면 좌우로 뒤집어).
    private struct FacePiece: Equatable {
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
        if blink || face == .sleep {
            // 자는 그림의 감은 눈을 조금 줄여 3 위로 — 깜빡일 때와 잠들며 눈을 감을 때.
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
