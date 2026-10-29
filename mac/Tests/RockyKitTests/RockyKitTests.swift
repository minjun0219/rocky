import Foundation
import XCTest

@testable import RockyKit

final class EventsTests: XCTestCase {
    func testParserJoinsDataLinesAndDropsComments() {
        var parser = SSEParser()
        XCTAssertNil(parser.feed(line: ": connected"))
        XCTAssertNil(parser.feed(line: ""))
        XCTAssertNil(parser.feed(line: "data: {\"a\":"))
        XCTAssertNil(parser.feed(line: "data:1}\r"))
        XCTAssertEqual(parser.feed(line: ""), "{\"a\":\n1}")
        // 프레임이 끝나면 비운다.
        XCTAssertNil(parser.feed(line: ""))
    }

    func testSlicesFollowWebRules() {
        XCTAssertEqual(slices(for: [#"{"entity":"todo","entityId":"t","action":"update"}"#]), [.todos])
        XCTAssertEqual(slices(for: [#"{"entity":"section","entityId":"s","action":"create"}"#]), [.sections, .todos])
        XCTAssertEqual(slices(for: [#"{"entity":"board","entityId":"b","action":"pr-ready"}"#]), [.prs])
        XCTAssertEqual(slices(for: [#"{"entity":"note","entityId":"n","action":"update"}"#]), [])
        // 보드 자체가 바뀌었거나 모르는 모양이면 전부.
        XCTAssertEqual(slices(for: [#"{"entity":"board","entityId":"b","action":"update"}"#]), Set(Slice.allCases))
        XCTAssertEqual(slices(for: ["not json"]), Set(Slice.allCases))
    }

    func testBackoff() {
        XCTAssertEqual(backoffSeconds(0), 1)
        XCTAssertEqual(backoffSeconds(3), 8)
        XCTAssertEqual(backoffSeconds(10), 30)
        // 붙자마자 끊기면 늘고, 오래 붙어 있던 것만 처음으로.
        XCTAssertEqual(nextAttempt(4, openedFor: 1), 4)
        XCTAssertEqual(nextAttempt(4, openedFor: nil), 4)
        XCTAssertEqual(nextAttempt(4, openedFor: 12), 0)
    }
}

final class BoardTreeTests: XCTestCase {
    func testGroupsBySectionAndNestsChildren() {
        let sections = [Section(id: "s1", boardId: "b1", title: "검증")]
        let todos = [
            Todo(id: "a", number: 1, boardId: "b1", title: "느슨한 것"),
            Todo(id: "b", number: 2, boardId: "b1", title: "부모", sectionId: "s1"),
            Todo(id: "c", number: 3, boardId: "b1", title: "자식", sectionId: "s1", parentId: "b"),
            Todo(id: "d", number: 4, boardId: "b2", title: "다른 보드"),
        ]
        let groups = boardGroups(todos: todos, boardId: "b1", sections: sections)
        XCTAssertEqual(groups.map(\.title), ["일반", "검증"])
        XCTAssertEqual(groups[1].rows.map(\.todo.id), ["b", "c"])
        XCTAssertEqual(groups[1].rows.map(\.depth), [0, 1])
    }

    func testHidesDoneBranchesButKeepsDoneParentOfOpenChild() {
        let todos = [
            Todo(id: "a", number: 1, boardId: "b1", title: "끝난 것", status: .done),
            Todo(id: "b", number: 2, boardId: "b1", title: "끝난 부모", status: .done),
            Todo(id: "c", number: 3, boardId: "b1", title: "열린 자식", parentId: "b"),
        ]
        let groups = boardGroups(todos: todos, boardId: "b1", sections: [])
        XCTAssertEqual(groups.flatMap(\.rows).map(\.todo.id), ["b", "c"])
        XCTAssertTrue(boardGroups(todos: [todos[0]], boardId: "b1", sections: []).isEmpty)
    }

    func testMenuLists() {
        let todos = [
            Todo(id: "a", number: 1, boardId: "b1", title: "도는 것", status: .doing),
            Todo(id: "b", number: 2, boardId: "b2", title: "할 것"),
        ]
        XCTAssertEqual(doingTodos(todos).map(\.id), ["a"])
        var draft = PrSnapshot(repo: "o/r", number: 3, title: "초안", url: "u", ready: true)
        draft.isDraft = true
        let prs = [
            PrSnapshot(repo: "o/r", number: 1, title: "후보", url: "u", ready: true),
            PrSnapshot(repo: "o/r", number: 2, title: "아직", url: "u", ready: false),
            draft,
        ]
        XCTAssertEqual(readyPrs(prs).map(\.number), [1])
    }

    func testStatusActions() {
        XCTAssertEqual(StatusAction.available(for: .todo), [.start, .done])
        XCTAssertEqual(StatusAction.available(for: .doing), [.stop, .done])
        XCTAssertEqual(StatusAction.available(for: .done), [.reopen])
    }
}

final class DaemonClientTests: XCTestCase {
    /// 보낸 요청을 남기고 고정 응답을 돌려주는 가짜.
    final class Recorder: @unchecked Sendable {
        var requests: [URLRequest] = []
        var status = 200
        var body = Data("[]".utf8)
    }

    func client(_ recorder: Recorder) -> DaemonClient {
        DaemonClient(base: "http://127.0.0.1:8636/", actor: "logan") { request in
            recorder.requests.append(request)
            let response = HTTPURLResponse(
                url: request.url!, statusCode: recorder.status, httpVersion: nil, headerFields: nil
            )!
            return (recorder.body, response)
        }
    }

    func testHeadersAndBody() async throws {
        let recorder = Recorder()
        recorder.body = Data(#"{"id":"t1","number":7,"boardId":"b1","title":"x","status":"doing","priority":"p4","labels":[],"links":[],"position":0,"createdAt":"","updatedAt":"","ref":"rocky-7","commentCount":0,"doingState":"live"}"#.utf8)
        let todo = try await client(recorder).setStatus("t1", .start)
        XCTAssertEqual(todo.ref, "rocky-7")
        XCTAssertEqual(todo.doingState, "live")
        let request = try XCTUnwrap(recorder.requests.first)
        XCTAssertEqual(request.url?.absoluteString, "http://127.0.0.1:8636/api/todos/t1/status")
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.value(forHTTPHeaderField: "x-rocky-client"), "mac")
        XCTAssertEqual(request.value(forHTTPHeaderField: "x-rocky-actor"), "logan")
        XCTAssertEqual(request.httpBody.map { String(decoding: $0, as: UTF8.self) }, #"{"action":"start"}"#)
    }

    func testEncodesBoardKey() async throws {
        let recorder = Recorder()
        _ = try await client(recorder).sections(board: "a b&c")
        XCTAssertEqual(recorder.requests.first?.url?.absoluteString, "http://127.0.0.1:8636/api/sections?board=a%20b%26c")
    }

    func testErrorCarriesRouteStatusAndMessage() async {
        let recorder = Recorder()
        recorder.status = 400
        recorder.body = Data(#"{"error":"title is required"}"#.utf8)
        do {
            try await client(recorder).add(board: "rocky", title: "")
            XCTFail("실패해야 한다")
        } catch {
            XCTAssertEqual(error.localizedDescription, "POST /api/todos → 400 title is required")
        }
    }
}

final class SettingsTests: XCTestCase {
    func testDefaultsBeatEnvBeatsBuiltIn() {
        let none = DaemonSettings.resolve(env: [:], defaults: { _ in nil })
        XCTAssertEqual(none.base, "http://127.0.0.1:8636")
        XCTAssertEqual(none.actor, "logan")

        let env = DaemonSettings.resolve(env: ["ROCKY_TODO_PORT": "9000", "ROCKY_TODO_ACTOR": "minjun"], defaults: { _ in nil })
        XCTAssertEqual(env.base, "http://127.0.0.1:9000")
        XCTAssertEqual(env.actor, "minjun")

        let both = DaemonSettings.resolve(
            env: ["ROCKY_TODO_ACTOR": "minjun"],
            defaults: { ["daemonURL": "http://127.0.0.1:1234", "actor": "logan2"][$0] }
        )
        XCTAssertEqual(both.base, "http://127.0.0.1:1234")
        XCTAssertEqual(both.actor, "logan2")
    }
}

final class SessionsTests: XCTestCase {
    func testPhaseFollowsWebRules() {
        XCTAssertEqual(agentPhase(Session(sessionId: "a", name: "a", kind: "background", status: nil, state: "blocked")), .blocked)
        XCTAssertEqual(agentPhase(Session(sessionId: "b", name: "b", kind: "background", state: "working")), .run)
        XCTAssertEqual(agentPhase(Session(sessionId: "c", name: "c", status: "busy")), .run)
        // interactive 의 idle 은 터미널이 알리므로 내 차례가 아니다.
        XCTAssertEqual(agentPhase(Session(sessionId: "d", name: "d", status: "idle")), .idle)
        XCTAssertEqual(agentPhase(Session(sessionId: "e", name: "e", kind: "background", state: "done")), .done)
    }

    func testRowsSortAndDropDone() {
        let rows = agentRows([
            Session(sessionId: "1", name: "zeta", status: "idle"),
            Session(sessionId: "2", name: "beta", status: "busy"),
            Session(sessionId: "3", name: "alpha", kind: "background", status: nil, state: "blocked"),
            Session(sessionId: "4", name: "gone", kind: "background", state: "done"),
            Session(sessionId: "5", name: "alpha", status: "busy"),
        ])
        XCTAssertEqual(rows.map(\.sessionId), ["3", "5", "2", "1"])
    }

    func testLineAndRepoLabel() {
        let waiting = Session(
            sessionId: "1", name: "a", kind: "background", status: nil, state: "blocked",
            job: JobSummary(detail: "테스트 중", needs: "머지해도 될까요?")
        )
        XCTAssertEqual(agentLine(waiting), "머지해도 될까요?")
        let working = Session(sessionId: "2", name: "b", kind: "background", state: "working", job: JobSummary(detail: "테스트 중"))
        XCTAssertEqual(agentLine(working), "테스트 중")
        XCTAssertNil(agentLine(Session(sessionId: "3", name: "c")))
        XCTAssertEqual(repoLabel("/Users/m/dev/rocky/.claude/worktrees/x"), "rocky")
        XCTAssertEqual(repoLabel("/Users/m/dev/rocky"), "rocky")
    }

    func testMoodPicksMostUrgent() {
        XCTAssertEqual(avatarMood(nil), .unknown)
        XCTAssertEqual(avatarMood(SessionsResult(available: false, reason: "없음", sessions: [])), .unknown)
        XCTAssertEqual(avatarMood(SessionsResult(available: true, sessions: [])), .resting)
        let busy = Session(sessionId: "1", name: "run", status: "busy")
        XCTAssertEqual(avatarMood(SessionsResult(available: true, sessions: [busy])), .working(count: 1))
        let blocked = Session(
            sessionId: "2", name: "wait", kind: "background", status: nil, state: "blocked",
            job: JobSummary(needs: "확인 부탁")
        )
        XCTAssertEqual(
            avatarMood(SessionsResult(available: true, sessions: [busy, blocked])),
            .waiting(name: "wait", line: "확인 부탁", count: 1)
        )
    }

    func testDecodesDaemonShape() throws {
        let json = #"{"available":true,"sessions":[{"cwd":"/r","kind":"background","id":"abc","sessionId":"s1","name":"eel","state":"blocked","startedAt":1,"matched":false,"job":{"needs":"답","updatedAt":"x"}}]}"#
        let result = try JSONDecoder().decode(SessionsResult.self, from: Data(json.utf8))
        XCTAssertEqual(result.sessions.first?.status, nil)
        XCTAssertEqual(result.sessions.first?.job?.needs, "답")
        XCTAssertEqual(avatarMood(result), .waiting(name: "eel", line: "답", count: 1))
    }
}

final class RoamTests: XCTestCase {
    let screen = Box(x: 0, y: 0, width: 1000, height: 800)

    func testLedgesDropCoveredPartsAndKeepTheFloor() {
        // 앞 창(위)이 뒤 창 윗면의 가운데를 덮는다.
        let front = Box(x: 300, y: 300, width: 200, height: 300)
        let back = Box(x: 100, y: 200, width: 600, height: 200)
        let out = ledges(windows: [front, back], screen: screen)
        XCTAssertEqual(out.first, Ledge(y: 0, minX: 0, maxX: 1000))
        XCTAssertTrue(out.contains(Ledge(y: 600, minX: 300, maxX: 500)))
        XCTAssertTrue(out.contains(Ledge(y: 400, minX: 100, maxX: 300)))
        XCTAssertTrue(out.contains(Ledge(y: 400, minX: 500, maxX: 700)))
        XCTAssertEqual(out.count, 4)
    }

    func testLedgesSkipTopsWithoutHeadroom() {
        let tall = Box(x: 0, y: 100, width: 400, height: 650)
        XCTAssertEqual(ledges(windows: [tall], screen: screen).count, 1)
        XCTAssertTrue(walls(windows: [tall], screen: screen).isEmpty)
    }

    func testFallsAndLandsOnTheHighestLedgeBelow() {
        let ground = ledges(windows: [Box(x: 100, y: 0, width: 400, height: 300)], screen: screen)
        var rocky = Roamer(x: 200, y: 700)
        for _ in 0 ..< 120 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .roam, random: { 0.99 })
        }
        XCTAssertEqual(rocky.y, 300)
        XCTAssertNotEqual(rocky.action, .fall)
    }

    func testFallsWhenTheWindowUnderneathVanishes() {
        let window = ledges(windows: [Box(x: 100, y: 0, width: 400, height: 300)], screen: screen)
        var rocky = Roamer(x: 200, y: 300)
        rocky.action = .stand
        rocky.step(dt: 1 / 30, ledges: window, walls: [], screen: screen, mood: .stay, random: { 0.99 })
        XCTAssertEqual(rocky.action, .stand)
        let floorOnly = ledges(windows: [], screen: screen)
        rocky.step(dt: 1 / 30, ledges: floorOnly, walls: [], screen: screen, mood: .stay, random: { 0.99 })
        XCTAssertEqual(rocky.action, .fall)
        for _ in 0 ..< 60 {
            rocky.step(dt: 1 / 30, ledges: floorOnly, walls: [], screen: screen, mood: .stay, random: { 0.99 })
        }
        XCTAssertEqual(rocky.y, 0)
        XCTAssertEqual(rocky.action, .stand)
    }

    func testJumpsOntoALowWindowTop() {
        let windows = [Box(x: 300, y: 0, width: 300, height: 200)]
        let ground = ledges(windows: windows, screen: screen)
        let sides = walls(windows: windows, screen: screen)
        var rocky = Roamer(x: 250, y: 0)
        rocky.action = .walk
        // 운이 0.3 이면 닿는 높이의 벽에서 늘 뛰어오르고(0.6 미만), 걷다 멈추지는 않는다.
        var crouched = false
        for _ in 0 ..< 300 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.3 })
            if rocky.crouch > 0 {
                crouched = true
                XCTAssertEqual(rocky.y, 0)
            }
            if rocky.y == 200, rocky.action != .jump { break }
        }
        XCTAssertTrue(crouched)
        XCTAssertEqual(rocky.y, 200)
        XCTAssertGreaterThan(rocky.x, 300)
        XCTAssertGreaterThan(rocky.squash, 0)
    }

    func testTurnsAtAWindowTooTallToJump() {
        let windows = [Box(x: 300, y: 0, width: 300, height: Roamer.jumpReach + 40)]
        let ground = ledges(windows: windows, screen: screen)
        let sides = walls(windows: windows, screen: screen)
        var rocky = Roamer(x: 250, y: 0)
        rocky.action = .walk
        for _ in 0 ..< 60 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.3 })
            XCTAssertNotEqual(rocky.action, .jump)
        }
        XCTAssertEqual(rocky.y, 0)
        XCTAssertEqual(rocky.facing, -1)
    }

    func testSleepAndStayDoNotWander() {
        var rocky = Roamer(x: 500, y: 0)
        rocky.action = .walk
        let floor = ledges(windows: [], screen: screen)
        rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .sleep, random: { 0 })
        XCTAssertEqual(rocky.action, .sit)
        rocky.step(dt: 1, ledges: floor, walls: [], screen: screen, mood: .stay, random: { 0 })
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.x, 500)
    }

    func testTubeArcsAboveBothEnds() {
        let tube = Tube(startX: 100, startY: 0, endX: 700, endY: 300, screen: screen)
        XCTAssertEqual(tube.point(at: 0).x, 100)
        XCTAssertEqual(tube.point(at: 0).y, 0)
        XCTAssertEqual(tube.point(at: 1).x, 700, accuracy: 1e-9)
        XCTAssertEqual(tube.point(at: 1).y, 300, accuracy: 1e-9)
        XCTAssertGreaterThan(tube.point(at: 0.5).y, 300)
        XCTAssertGreaterThan(tube.length, 600)
        // 화면 꼭대기를 넘지 않는다.
        XCTAssertEqual(Tube(startX: 0, startY: 700, endX: 500, endY: 0, screen: screen).peak, 770)
    }

    func testRidesATubeToAnotherLedge() {
        let ground = ledges(windows: [Box(x: 600, y: 0, width: 300, height: 300)], screen: screen)
        var rocky = Roamer(x: 100, y: 0)
        rocky.action = .walk
        rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .roam, random: { 0 })
        XCTAssertEqual(rocky.action, .tube)
        XCTAssertNotNil(rocky.tube)
        XCTAssertFalse(rocky.insideTube)
        var steps = 0
        while rocky.action == .tube, steps < 1000 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .roam, random: { 0 })
            steps += 1
        }
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.y, 300)
        XCTAssertEqual(rocky.x, 600 + Roamer.halfWidth, accuracy: 1e-9)
        // 다 건넌 뒤에도 튜브는 잠깐 남았다가 사라진다.
        XCTAssertNotNil(rocky.tube)
        for _ in 0 ..< 60 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .stay, random: { 0 })
        }
        XCTAssertNil(rocky.tube)
    }
}

final class MovesTests: XCTestCase {
    let screen = Box(x: 0, y: 0, width: 1000, height: 800)

    func testSpotTellsFloorWindowAndEdge() {
        let ground = ledges(windows: [Box(x: 300, y: 0, width: 400, height: 300)], screen: screen)
        XCTAssertEqual(spot(x: 100, y: 0, ledges: ground), .floor)
        XCTAssertEqual(spot(x: 500, y: 300, ledges: ground), .window)
        XCTAssertEqual(spot(x: 310, y: 300, ledges: ground), .edge(toward: -1))
        XCTAssertEqual(spot(x: 690, y: 300, ledges: ground), .edge(toward: 1))
        // 발판이 없으면(떨어지는 중) 바닥으로 본다.
        XCTAssertEqual(spot(x: 500, y: 500, ledges: ground), .floor)
    }

    func testPeekOnlyAtAnEdge() {
        XCTAssertFalse(Choreographer.breaks(at: .window).contains { $0.move == .peek })
        XCTAssertTrue(Choreographer.breaks(at: .edge(toward: 1)).contains { $0.move == .peek })
    }

    func testHammeringAlternatesWithBreaksThatNeverRepeat() {
        var moves = Choreographer()
        var now = 0.0
        var seen: [Clip] = []
        for _ in 0 ..< 12 {
            let clip = moves.update(now: now, mood: .working, spot: .window, random: { 0 })
            seen.append(clip)
            now = clip.end
        }
        XCTAssertEqual(seen.first?.move, .hammer)
        // 박자 0.7초(범위의 아래 끝) × 3번.
        XCTAssertEqual(seen.first?.tempo ?? 0, 0.7, accuracy: 1e-9)
        XCTAssertEqual(seen.first?.length ?? 0, 2.1, accuracy: 1e-9)
        for (i, clip) in seen.enumerated() {
            XCTAssertEqual(clip.move == .hammer, i.isMultiple(of: 2), "\(i): \(clip.move)")
        }
        let breaks = seen.map(\.move).filter { $0 != .hammer }
        for (a, b) in zip(breaks, breaks.dropFirst()) {
            XCTAssertNotEqual(a, b)
        }
    }

    func testWeightedPickFollowsTheWeights() {
        XCTAssertEqual(Choreographer.weighted([4, 3, 2], 0), 0)
        XCTAssertEqual(Choreographer.weighted([4, 3, 2], 0.5), 1)
        XCTAssertEqual(Choreographer.weighted([4, 3, 2], 0.99), 2)
    }

    func testWaitingCheersThenWatchesAndWavesSometimes() {
        var moves = Choreographer()
        let cheer = moves.update(now: 0, mood: .waiting(who: "a"), spot: .floor, random: { 0.5 })
        XCTAssertEqual(cheer.move, .cheer)
        XCTAssertEqual(cheer.length, Choreographer.cheerLength)
        let watch = moves.update(now: cheer.end, mood: .waiting(who: "a"), spot: .floor, random: { 0.5 })
        XCTAssertEqual(watch.move, .watch)
        XCTAssertEqual(watch.length, 15, accuracy: 1e-9)
        let wave = moves.update(now: watch.end, mood: .waiting(who: "a"), spot: .floor, random: { 0.5 })
        XCTAssertEqual(wave.move, .wave)
        XCTAssertEqual(moves.update(now: wave.end, mood: .waiting(who: "a"), spot: .floor, random: { 0.5 }).move, .watch)
        // 기다리는 세션이 바뀌면 다시 크게 알린다.
        XCTAssertEqual(moves.update(now: wave.end + 1, mood: .waiting(who: "b"), spot: .floor, random: { 0.5 }).move, .cheer)
        XCTAssertEqual(moves.update(now: wave.end + 2, mood: .other, spot: .floor, random: { 0.5 }).move, .mood)
    }

    func testLandingSquashesThenSettles() {
        let ground = ledges(windows: [], screen: screen)
        var rocky = Roamer(x: 500, y: 100)
        var steps = 0
        while rocky.action == .fall, steps < 300 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .stay, random: { 0.99 })
            steps += 1
        }
        XCTAssertEqual(rocky.squash, 1, accuracy: 1e-9)
        for _ in 0 ..< 10 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .stay, random: { 0.99 })
        }
        XCTAssertEqual(rocky.squash, 0)
    }
}
