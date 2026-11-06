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

    func testStaysAwakeForAWhileAfterWorkBeforeSleeping() {
        var sleepy = Drowsiness()
        // 처음부터 다 쉬면 바로 잔다.
        XCTAssertEqual(sleepy.mood(.resting, now: 0), .resting)
        XCTAssertEqual(sleepy.mood(.working(count: 1), now: 10), .working(count: 1))
        // 일을 마쳐도 `awake` 초 동안은 깨어 있다가 잔다.
        XCTAssertEqual(sleepy.mood(.resting, now: 20), .awake)
        XCTAssertEqual(sleepy.mood(.resting, now: 10 + Drowsiness.awake - 1), .awake)
        XCTAssertEqual(sleepy.mood(.resting, now: 10 + Drowsiness.awake), .resting)
        // 기다리는 세션도 일로 친다. 연결 없음은 그대로 보인다.
        let waiting = AvatarMood.waiting(name: "a", line: nil, count: 1)
        XCTAssertEqual(sleepy.mood(waiting, now: 1000), waiting)
        XCTAssertEqual(sleepy.mood(.unknown, now: 1001), .unknown)
        XCTAssertEqual(sleepy.mood(.resting, now: 1002), .awake)
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
        // 운이 0.3 이면 닿는 높이의 벽에서 늘 뛰어오르고(0.7 미만), 걷다 멈추거나 튜브를 놓지는 않는다.
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

    func testJumpCrouchesThenFlies() {
        let windows = [Box(x: 300, y: 0, width: 300, height: 200)]
        let ground = ledges(windows: windows, screen: screen)
        let sides = walls(windows: windows, screen: screen)
        var rocky = Roamer(x: 250, y: 0)
        rocky.action = .walk
        var crouched = 0.0, flight = 0.0, flew = false
        for _ in 0 ..< 300 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.3 })
            guard rocky.action == .jump else {
                if flew { break }
                continue
            }
            // 쪼그리는 동안은 뜨지 않고 뜬 뒤로는 쪼그리지 않는다 — 그림이 둘 사이를 한 번에 건너뛰지 않게 쪼그림은 다 내려간 뒤에 뜬다.
            XCTAssertTrue(rocky.crouch == 0 || rocky.flight == 0)
            if rocky.flight > 0, !flew { XCTAssertGreaterThan(crouched, 0.9) }
            // 뜬 정도는 내려설 때까지 늘기만 한다.
            XCTAssertGreaterThanOrEqual(rocky.flight, flight)
            crouched = max(crouched, rocky.crouch)
            flight = rocky.flight
            if flight > 0 { flew = true }
        }
        XCTAssertTrue(flew)
        XCTAssertGreaterThan(flight, 0.9)
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.flight, 0)
        XCTAssertEqual(rocky.y, 200)
    }

    func testStandsUpBeforeWalkingAfterSleep() {
        let floor = ledges(windows: [], screen: screen)
        var rocky = Roamer(x: 500, y: 0)
        rocky.action = .stand
        rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .sleep, random: { 0 })
        XCTAssertEqual(rocky.action, .sit)
        // 깨면 그림이 엎드린 그림에서 일어서는 동안(`wakeUp`) 제자리에 있다가 걷는다 — 바로 걸으면 엎드린 채 미끄러진다.
        var waited = 0.0
        while rocky.action != .walk, waited < 5 {
            rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .roam, random: { 0 })
            waited += 1 / 30
        }
        XCTAssertEqual(rocky.action, .walk)
        XCTAssertEqual(waited, Roamer.wakeUp, accuracy: 2.0 / 30)
        XCTAssertEqual(rocky.x, 500)
    }

    func testLaysATubeOverAWindowTooTallToJump() {
        let windows = [Box(x: 300, y: 0, width: 300, height: Roamer.jumpReach + 40)]
        let ground = ledges(windows: windows, screen: screen)
        let sides = walls(windows: windows, screen: screen)
        var rocky = Roamer(x: 250, y: 0)
        rocky.action = .walk
        // 운이 0.3 이면 뛰기엔 높은 벽 앞에서 튜브를 놓는다(0.5 미만).
        for _ in 0 ..< 600 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.3 })
            XCTAssertNotEqual(rocky.action, .jump)
            if rocky.y == Roamer.jumpReach + 40, rocky.action != .tube { break }
        }
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.y, Roamer.jumpReach + 40)
        XCTAssertGreaterThan(rocky.x, 300)
        XCTAssertLessThan(rocky.x, 600)
    }

    func testTurnsAtAWindowWithNoRoomOnTop() {
        // 앞 창이 뒤 창 윗면의 왼쪽을 덮어 옆면 너머에 설 자리가 없다.
        let windows = [Box(x: 250, y: 150, width: 200, height: 400), Box(x: 300, y: 0, width: 300, height: 200)]
        let ground = ledges(windows: windows, screen: screen)
        let sides = walls(windows: windows, screen: screen)
        var rocky = Roamer(x: 250, y: 0)
        rocky.action = .walk
        for _ in 0 ..< 60 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.3 })
            XCTAssertNotEqual(rocky.action, .jump)
            XCTAssertNotEqual(rocky.action, .tube)
        }
        XCTAssertEqual(rocky.y, 0)
        XCTAssertEqual(rocky.facing, -1)
    }

    func testHopsAcrossAGapBetweenWindowTops() {
        let windows = [Box(x: 100, y: 0, width: 300, height: 300), Box(x: 500, y: 0, width: 300, height: 300)]
        let ground = ledges(windows: windows, screen: screen)
        let sides = walls(windows: windows, screen: screen)
        var rocky = Roamer(x: 350, y: 300)
        rocky.action = .walk
        var hopped = false
        for _ in 0 ..< 120 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.3 })
            if rocky.action == .jump {
                hopped = true
                // 틈 위로 솟았다가 건너편에 내려앉는다 — 발판 높이 아래로 처지지 않는다.
                XCTAssertGreaterThanOrEqual(rocky.y, 300)
            } else if hopped {
                break
            }
        }
        XCTAssertTrue(hopped)
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.y, 300)
        XCTAssertEqual(rocky.x, 500 + Roamer.halfWidth * 2)
    }

    func testDropsOrTurnsAtTheEndOfAWindowTop() {
        let windows = [Box(x: 100, y: 0, width: 300, height: 300)]
        let ground = ledges(windows: windows, screen: screen)
        let sides = walls(windows: windows, screen: screen)
        // 건너뛸 데가 없으면 운이 `dropChance` 아래일 때만 뛰어내리고 나머지는 돌아선다.
        var turner = Roamer(x: 390, y: 300)
        turner.action = .walk
        for _ in 0 ..< 30 {
            turner.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.5 })
        }
        XCTAssertEqual(turner.facing, -1)
        XCTAssertEqual(turner.y, 300)
        var dropper = Roamer(x: 390, y: 300)
        dropper.action = .walk
        for _ in 0 ..< 60 {
            dropper.step(dt: 1 / 30, ledges: ground, walls: sides, screen: screen, mood: .roam, random: { 0.2 })
        }
        XCTAssertEqual(dropper.y, 0)
    }

    func testTubesUpFromTheFloorMoreOftenThanBetweenWindowTops() {
        // 운 0.004 는 바닥에서 오르는 확률(dt × upRate)보다 작고 창 위에서 건너는 확률(dt × tubeRate)보다 크다.
        let windows = [Box(x: 100, y: 0, width: 300, height: 300), Box(x: 600, y: 0, width: 300, height: 300)]
        let ground = ledges(windows: windows, screen: screen)
        var floor = Roamer(x: 500, y: 0)
        floor.action = .walk
        floor.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .roam, random: { 0.004 })
        XCTAssertEqual(floor.action, .tube)
        XCTAssertEqual(floor.tube?.endY, 300)
        var top = Roamer(x: 250, y: 300)
        top.action = .walk
        top.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .roam, random: { 0.004 })
        XCTAssertEqual(top.action, .walk)
        XCTAssertNil(top.tube)
    }

    func testCrossesAFloorWithoutWindowsThroughAHighTube() {
        let floor = ledges(windows: [], screen: screen)
        var rocky = Roamer(x: 100, y: 0)
        rocky.action = .walk
        rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .roam, random: { 0 })
        XCTAssertEqual(rocky.action, .tube)
        guard let tube = rocky.tube else { return XCTFail("튜브를 놓지 않았다") }
        XCTAssertEqual(tube.endY, 0)
        XCTAssertGreaterThanOrEqual(tube.endX - tube.startX, Roamer.crossing)
        // 화면 높이의 `crossingRise` 만큼 솟는다.
        XCTAssertGreaterThanOrEqual(tube.peak, 800 * Roamer.crossingRise.lowerBound)
        var steps = 0
        while rocky.action == .tube, steps < 1000 {
            rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .roam, random: { 0 })
            steps += 1
        }
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.y, 0)
        XCTAssertEqual(rocky.x, tube.endX, accuracy: 1e-9)
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

    func testFramesStopWhenNobodyCanSeeRocky() {
        // 화면이 잠들었거나 하나도 없으면(화면 없는 맥) 멈춘다.
        XCTAssertFalse(framesRun(screensAsleep: true, screenCount: 1, onScreen: true, visible: true, roaming: true))
        XCTAssertFalse(framesRun(screensAsleep: false, screenCount: 0, onScreen: false, visible: true, roaming: true))
        // 화면 안이면 보이는 동안만 — 다 가려지면(화면 잠금 · 화면 보호기) 멈춘다.
        XCTAssertTrue(framesRun(screensAsleep: false, screenCount: 1, onScreen: true, visible: true, roaming: false))
        XCTAssertFalse(framesRun(screensAsleep: false, screenCount: 1, onScreen: true, visible: false, roaming: true))
        // 화면 밖으로 밀려났으면 돌아다니는 동안은 돌려 걸어 들어오게 한다 — 안 돌아다니면 옮길 일이 없다.
        XCTAssertTrue(framesRun(screensAsleep: false, screenCount: 1, onScreen: false, visible: false, roaming: true))
        XCTAssertFalse(framesRun(screensAsleep: false, screenCount: 1, onScreen: false, visible: false, roaming: false))
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

    func testStartsSlowlyAndEasesIntoItsPacingSpot() {
        // 걷기 시작하면 `walkAccel` 로 빨라지고 설 자리가 다가오면 줄여서 그 자리에 선다 — 한 프레임 만에 45pt/s 로 나가거나 서지 않는다.
        let floor = ledges(windows: [], screen: screen)
        let goal = Goal(x: 500, y: 0)
        var rocky = Roamer(x: 500, y: 0)
        rocky.action = .stand
        let dt = 1.0 / 60
        var moves: [Double] = []
        var steps = 0
        repeat {
            let before = rocky.x
            rocky.step(dt: dt, ledges: floor, walls: [], screen: screen, mood: .pace, goal: goal, random: { 0 })
            if rocky.x != before { moves.append(abs(rocky.x - before)) }
            steps += 1
        } while !(!moves.isEmpty && rocky.action == .stand) && steps < 600
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.x, goal.x - Roamer.near, accuracy: 1e-9)
        guard let first = moves.first, let top = moves.max(), let peak = moves.firstIndex(of: top) else {
            return XCTFail("걷지 않았다")
        }
        XCTAssertEqual(first, Roamer.walkAccel * dt * dt, accuracy: 1e-9)
        XCTAssertEqual(top, Roamer.walkSpeed * dt, accuracy: 1e-9)
        // 다 빨라질 때까지는 줄지 않고, 그 뒤로는 늘지 않는다.
        let rising = moves.prefix(peak + 1)
        for (a, b) in zip(rising, rising.dropFirst()) {
            XCTAssertGreaterThanOrEqual(b, a - 1e-9)
        }
        let falling = moves.suffix(from: peak)
        for (a, b) in zip(falling, falling.dropFirst()) {
            XCTAssertLessThanOrEqual(b, a + 1e-9)
        }
    }

    func testEasesToAStopBeforeResting() {
        // 가끔 쉴 때도 한 프레임에 서지 않는다 — `walkAccel` 로 줄여 다 멈춘 뒤에 선다.
        let top = ledges(windows: [Box(x: 100, y: 0, width: 800, height: 300)], screen: screen)
        var rocky = Roamer(x: 300, y: 300)
        rocky.action = .walk
        let dt = 1.0 / 60
        for _ in 0 ..< 30 {
            rocky.step(dt: dt, ledges: top, walls: [], screen: screen, mood: .roam, random: { 0.99 })
        }
        XCTAssertEqual(rocky.speed, Roamer.walkSpeed)
        // 운이 0 이면 쉬기로 한다 — 이 창 위에서는 건너갈 다른 창이 없어 튜브는 놓지 않는다.
        var speeds: [Double] = []
        while rocky.action == .walk, speeds.count < 120 {
            rocky.step(dt: dt, ledges: top, walls: [], screen: screen, mood: .roam, random: { 0 })
            speeds.append(rocky.speed)
        }
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.pause, 1.5, accuracy: 1e-9)
        XCTAssertGreaterThan(speeds.count, 10)
        for (a, b) in zip(speeds, speeds.dropFirst()) {
            XCTAssertLessThanOrEqual(b, a)
        }
    }

    func testTurnsAroundFromAStandstill() {
        // 발판 끝에서 돌아서면 섰다가 다시 빨라진다 — 45pt/s 그대로 되돌아가지 않는다.
        let top = ledges(windows: [Box(x: 100, y: 0, width: 300, height: 300)], screen: screen)
        var rocky = Roamer(x: 300, y: 300)
        rocky.action = .walk
        let dt = 1.0 / 60
        var steps = 0
        while rocky.facing > 0, steps < 600 {
            rocky.step(dt: dt, ledges: top, walls: [], screen: screen, mood: .roam, random: { 0.5 })
            steps += 1
        }
        XCTAssertEqual(rocky.facing, -1)
        XCTAssertEqual(rocky.speed, 0)
        let before = rocky.x
        rocky.step(dt: dt, ledges: top, walls: [], screen: screen, mood: .roam, random: { 0.5 })
        XCTAssertEqual(before - rocky.x, Roamer.walkAccel * dt * dt, accuracy: 1e-9)
    }
}

final class ShiftsTests: XCTestCase {
    let watch = Clip(move: .watch, start: 0, length: 30, tempo: 1)
    let hammer = Clip(move: .hammer, start: 0, length: 30, tempo: 0.8)
    let still = Clip(move: .mood, start: 0, length: 0, tempo: 1)

    func testFirstMomentIsAlreadySettled() {
        let shifts = Shifts(Moment(mood: .resting, action: .sit, clip: still))
        XCTAssertEqual(shifts.layers.count, 1)
        XCTAssertEqual(shifts.layers[0].weight(at: 0), 1)
        // 앱을 켜자마자 잠들기 동작을 하지 않게 — 기분은 오래전에 시작된 것으로 친다.
        XCTAssertEqual(shifts.current.since, -.infinity)
    }

    func testSameSceneOnlyUpdatesFlowingValues() {
        var shifts = Shifts(Moment(mood: .awake, action: .jump, clip: watch))
        shifts.update(Moment(mood: .awake, action: .jump, clip: watch, crouch: 0.5), now: 10)
        shifts.update(Moment(mood: .awake, action: .jump, clip: watch, flight: 0.3), now: 10.1)
        XCTAssertEqual(shifts.layers.count, 1)
        XCTAssertEqual(shifts.current.crouch, 0)
        XCTAssertEqual(shifts.current.flight, 0.3)
    }

    func testChangeLayersOnTopUntilItCoversTheRest() {
        var shifts = Shifts(Moment(mood: .awake, action: .stand, clip: watch))
        shifts.update(Moment(mood: .awake, action: .walk, clip: watch), now: 10)
        XCTAssertEqual(shifts.layers.count, 2)
        XCTAssertEqual(shifts.layers[1].start, 10)
        XCTAssertEqual(shifts.layers[1].length, Shifts.actionShift)
        XCTAssertEqual(shifts.layers[1].weight(at: 10.1), 0.5, accuracy: 1e-9)
        shifts.update(Moment(mood: .awake, action: .walk, clip: watch), now: 10.1)
        XCTAssertEqual(shifts.layers.count, 2)
        // 다 섞여 들어오면 아래 층은 버린다.
        shifts.update(Moment(mood: .awake, action: .walk, clip: watch), now: 10.5)
        XCTAssertEqual(shifts.layers.count, 1)
        XCTAssertEqual(shifts.current.action, .walk)
    }

    func testOlderLayersKeepTheValuesTheyHadWhenLeft() {
        // 뛰다가 내려앉으면 아래 층은 내려앉을 때의 뜬 정도 그대로 섞여 사라진다.
        var shifts = Shifts(Moment(mood: .awake, action: .jump, clip: watch))
        shifts.update(Moment(mood: .awake, action: .jump, clip: watch, flight: 1), now: 10)
        shifts.update(Moment(mood: .awake, action: .stand, clip: watch), now: 10.02)
        XCTAssertEqual(shifts.layers.count, 2)
        XCTAssertEqual(shifts.layers[0].moment.flight, 1)
        XCTAssertEqual(shifts.current.flight, 0)
    }

    func testMoodStartMovesOnlyWhenTheMoodChanges() {
        var shifts = Shifts(Moment(mood: .awake, action: .stand, clip: watch))
        shifts.update(Moment(mood: .resting, action: .stand, clip: watch), now: 10)
        XCTAssertEqual(shifts.current.since, 10)
        // 움직임만 바뀌면(서 있다 앉기) 잠들기 동작을 처음부터 다시 하지 않는다.
        shifts.update(Moment(mood: .resting, action: .sit, clip: watch), now: 11)
        XCTAssertEqual(shifts.current.since, 10)
        shifts.update(Moment(mood: .working, action: .sit, clip: hammer), now: 12)
        XCTAssertEqual(shifts.current.since, 12)
    }

    func testShiftLengthFollowsWhatChanged() {
        let stand = Moment(mood: .awake, action: .stand, clip: watch)
        var walk = stand
        walk.action = .walk
        var work = stand
        work.mood = .working
        work.clip = hammer
        var wave = stand
        wave.clip = Clip(move: .wave, start: 5, length: 2.4, tempo: 1)
        let sleep = Moment(mood: .resting, action: .sit, clip: still)
        var dozing = sleep
        dozing.action = .stand
        var puzzled = sleep
        puzzled.mood = .unknown
        puzzled.action = .stand
        XCTAssertEqual(Shifts.length(from: stand, to: walk), Shifts.actionShift)
        XCTAssertEqual(Shifts.length(from: stand, to: work), Shifts.moveShift)
        XCTAssertEqual(Shifts.length(from: stand, to: wave), Shifts.moveShift)
        XCTAssertEqual(Shifts.length(from: stand, to: sleep), Shifts.intoWhole)
        XCTAssertEqual(Shifts.length(from: sleep, to: work), Shifts.outOfWhole)
        // 자다가 연결이 끊기면 잠 그림이 옅어진 뒤 갸웃 그림이 짙어진다.
        XCTAssertEqual(Shifts.length(from: sleep, to: puzzled), Shifts.outOfWhole)
        // 같은 통째 그림 그대로면 짧게.
        XCTAssertEqual(Shifts.length(from: dozing, to: sleep), Shifts.actionShift)
        // 뛰거나 떨어지는 동안은 통째 장면이 아니다.
        var falling = sleep
        falling.action = .fall
        XCTAssertFalse(falling.whole)
        XCTAssertEqual(Shifts.length(from: falling, to: sleep), Shifts.intoWhole)
    }

    func testKeepsAtMostAFewLayers() {
        var shifts = Shifts(Moment(mood: .awake, action: .stand, clip: watch))
        let actions: [RoamAction] = [.walk, .stand, .walk, .stand, .walk, .stand]
        for (i, action) in actions.enumerated() {
            shifts.update(Moment(mood: .awake, action: action, clip: watch), now: 10 + 0.01 * Double(i))
        }
        XCTAssertEqual(shifts.layers.count, Shifts.most)
        XCTAssertEqual(shifts.current.action, .stand)
    }

    func testPoseMoodDropsNamesAndCounts() {
        XCTAssertEqual(AvatarMood.working(count: 2).poseMood, AvatarMood.working(count: 1).poseMood)
        XCTAssertEqual(AvatarMood.waiting(name: "a", line: nil, count: 1).poseMood, .waiting)
        XCTAssertEqual(AvatarMood.unknown.poseMood, .unknown)
    }
}

final class FeetTests: XCTestCase {
    let dt = 1.0 / 60

    /// 걷는 몸의 자리(60Hz) — `walkAccel` 로 빨라져 `cruise` 초 동안 `walkSpeed` 로 가다가 같은 만큼 줄여 서고, `hold` 초 서 있는다.
    func path(toward: Double, cruise: Double, hold: Double) -> [Double] {
        let ramp = Roamer.walkSpeed / Roamer.walkAccel
        var out: [Double] = []
        var x = 0.0, t = 0.0
        while t < ramp * 2 + cruise + hold {
            t += dt
            let v: Double
            if t < ramp {
                v = Roamer.walkAccel * t
            } else if t < ramp + cruise {
                v = Roamer.walkSpeed
            } else {
                v = max(0, Roamer.walkSpeed - Roamer.walkAccel * (t - ramp - cruise))
            }
            x += toward * v * dt
            out.append(x)
        }
        return out
    }

    func testPlantedFeetStayPutAndTakeTurns() {
        for toward in [1.0, -1.0] {
            var feet = Feet(x: 0)
            var last = 0.0
            var lifts: [Int] = []
            var walkingLifts: [Int] = []
            for x in path(toward: toward, cruise: 2, hold: 1) {
                let before = feet.feet
                let walking = abs(x - last) / dt > 0.5
                feet.step(x: x, dt: dt)
                last = x
                for i in 0 ..< 2 {
                    // 디딘 발은 화면에 붙어 있다 — 빨라지든 줄든 몸만 그 위를 지나간다.
                    if before[i].age == nil, feet.feet[i].age == nil {
                        XCTAssertEqual(feet.feet[i].x, before[i].x)
                    }
                    if before[i].age == nil, feet.feet[i].age != nil {
                        lifts.append(i)
                        if walking { walkingLifts.append(i) }
                    }
                }
                // 두 발을 한꺼번에 들지 않는다.
                XCTAssertFalse(feet.feet.allSatisfy { $0.age != nil })
            }
            // 가는 쪽 발이 먼저 나가고, 걷는 동안은 한 발씩 번갈아 든다.
            let front = toward > 0 ? 1 : 0
            XCTAssertEqual(lifts.first, front)
            XCTAssertGreaterThanOrEqual(walkingLifts.count, 8)
            for (a, b) in zip(walkingLifts, walkingLifts.dropFirst()) {
                XCTAssertNotEqual(a, b)
            }
            // 다 서면 벌어진 발을 모아 엉덩이 아래에 선다.
            let stance = feet.stance(at: last)
            XCTAssertLessThanOrEqual(abs(stance.left.offset), Feet.settle)
            XCTAssertLessThanOrEqual(abs(stance.right.offset), Feet.settle)
            XCTAssertEqual(stance.left.lift, 0)
            XCTAssertEqual(stance.right.lift, 0)
        }
    }

    func testFeetTuckUnderTheBodyInTheAir() {
        var feet = Feet(x: 0)
        var x = 0.0
        // 걸어서 두 발이 벌어진 때에 뛰어오른다.
        for next in path(toward: 1, cruise: 2, hold: 0) {
            feet.step(x: next, dt: dt)
            x = next
            let stance = feet.stance(at: x)
            if max(abs(stance.left.offset), abs(stance.right.offset)) > 3 { break }
        }
        let spread = feet.stance(at: x)
        XCTAssertGreaterThan(max(abs(spread.left.offset), abs(spread.right.offset)), 3)
        // 떠 있는 동안 발은 몸과 같이 가며 0.3초 안에 엉덩이 아래로 모인다 — 한 프레임에 붙이면 다리가 툭 튄다.
        x += 300 * dt
        feet.airborne(x: x, dt: dt)
        let first = feet.stance(at: x)
        XCTAssertGreaterThan(max(abs(first.left.offset), abs(first.right.offset)), 1)
        for _ in 1 ..< 18 {
            x += 300 * dt
            feet.airborne(x: x, dt: dt)
        }
        let tucked = feet.stance(at: x)
        XCTAssertLessThan(abs(tucked.left.offset), 0.2)
        XCTAssertLessThan(abs(tucked.right.offset), 0.2)
        XCTAssertLessThan(max(tucked.left.lift, tucked.right.lift), 0.2)
    }

    func testDrawnLegsSwapWhenFacingLeft() {
        // 왼쪽을 보면 그림을 뒤집으므로 화면의 오른발이 그림의 왼다리이고 앞뒤도 바뀐다.
        let stance = Stance(left: Footing(offset: 3, lift: 1), right: Footing(offset: -2, lift: 0))
        XCTAssertEqual(stance.drawn(facing: 1).left, stance.left)
        XCTAssertEqual(stance.drawn(facing: 1).right, stance.right)
        XCTAssertEqual(stance.drawn(facing: -1).left, Footing(offset: 2, lift: 0))
        XCTAssertEqual(stance.drawn(facing: -1).right, Footing(offset: -3, lift: 1))
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

    func testSameSessionWaitingAgainSoonDoesNotCheerAgain() {
        var moves = Choreographer()
        XCTAssertEqual(moves.update(now: 0, mood: .waiting(who: "a"), spot: .floor, random: { 0.5 }).move, .cheer)
        XCTAssertEqual(moves.update(now: 10, mood: .working, spot: .floor, random: { 0.5 }).move, .hammer)
        // 같은 세션이 곧 다시 기다리면 뛰지 않고 지켜본다.
        XCTAssertEqual(moves.update(now: 20, mood: .waiting(who: "a"), spot: .floor, random: { 0.5 }).move, .watch)
        // 다른 세션이면 바로 크게 알린다.
        XCTAssertEqual(moves.update(now: 30, mood: .waiting(who: "b"), spot: .floor, random: { 0.5 }).move, .cheer)
        // 쉼 시간(cheerCooldown)이 지나면 같은 세션도 다시 크게 알린다.
        XCTAssertEqual(moves.update(now: 40, mood: .working, spot: .floor, random: { 0.5 }).move, .hammer)
        let later = 30 + Choreographer.cheerCooldown + 1
        XCTAssertEqual(moves.update(now: later, mood: .waiting(who: "b"), spot: .floor, random: { 0.5 }).move, .cheer)
    }

    func testAwakeWatchesAndTakesBreaksWithoutHammering() {
        var moves = Choreographer()
        var now = 0.0
        var seen: [Move] = []
        for _ in 0 ..< 7 {
            let clip = moves.update(now: now, mood: .awake, spot: .floor, random: { 0 })
            seen.append(clip.move)
            now = clip.end
        }
        XCTAssertFalse(seen.contains(.hammer))
        for (i, move) in seen.enumerated() {
            XCTAssertEqual(move == .watch, i.isMultiple(of: 2), "\(i): \(move)")
        }
        let breaks = seen.filter { $0 != .watch }
        for (a, b) in zip(breaks, breaks.dropFirst()) {
            XCTAssertNotEqual(a, b)
        }
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

final class GazeTests: XCTestCase {
    let screen = Box(x: 0, y: 0, width: 1000, height: 800)

    func input(
        _ x: Double, _ y: Double, pointer: Double = 1, key: Double = 999, focus: Box? = nil,
        rocky: (Double, Double) = (900, 0)
    ) -> GazeInput {
        GazeInput(
            pointerX: x, pointerY: y, sincePointer: pointer, sinceKey: key, focus: focus,
            rockyX: rocky.0, rockyY: rocky.1
        )
    }

    func testLooksAtTheSettledPointerOnlyAfterItStops() {
        var gaze = GazeTracker()
        XCTAssertNil(gaze.update(input(300, 400, pointer: 0.1)))
        XCTAssertEqual(gaze.update(input(300, 400)), GazePoint(x: 300, y: 400))
        // 조금 옮긴 것은 무시하고, 멀리 옮기면 따라간다.
        XCTAssertEqual(gaze.update(input(400, 400)), GazePoint(x: 300, y: 400))
        XCTAssertEqual(gaze.update(input(600, 500)), GazePoint(x: 600, y: 500))
        // 움직이는 동안은 하던 눈길 그대로.
        XCTAssertEqual(gaze.update(input(100, 100, pointer: 0.2)), GazePoint(x: 600, y: 500))
    }

    func testTypingLooksAtTheFrontWindowTop() {
        var gaze = GazeTracker()
        let editor = Box(x: 100, y: 100, width: 600, height: 400)
        // 포인터가 창 밖이면 창 가운데, 창 위면 포인터 쪽.
        XCTAssertEqual(gaze.update(input(900, 700, pointer: 30, key: 0.5, focus: editor)), GazePoint(x: 400, y: 500))
        var other = GazeTracker()
        XCTAssertEqual(other.update(input(150, 300, pointer: 30, key: 0.5, focus: editor)), GazePoint(x: 150, y: 500))
    }

    func testIgnoresAPointerReachingForRockyAndLetsGoWhenAway() {
        var gaze = GazeTracker()
        XCTAssertNil(gaze.update(input(880, 40, rocky: (900, 0))))
        XCTAssertNotNil(gaze.update(input(300, 400)))
        XCTAssertNil(gaze.update(input(300, 400, pointer: 200, key: 200)))
        XCTAssertNil(gaze.target)
    }

    func testGoalStandsBesideTheGazeOnTheHighestLedgeBelow() {
        let ground = ledges(windows: [Box(x: 100, y: 0, width: 600, height: 300)], screen: screen)
        // 로키가 오른쪽에 있으면 눈길 오른쪽으로 비켜 선다 — 눈길 아래의 창 윗면에.
        XCTAssertEqual(
            followGoal(gaze: GazePoint(x: 400, y: 450), ledges: ground, fromX: 900), Goal(x: 510, y: 300)
        )
        XCTAssertEqual(
            followGoal(gaze: GazePoint(x: 400, y: 450), ledges: ground, fromX: 0), Goal(x: 290, y: 300)
        )
        // 그쪽에 설 데가 없으면 반대쪽, 눈길이 창 윗면보다 낮으면 바닥.
        XCTAssertEqual(
            followGoal(gaze: GazePoint(x: 650, y: 450), ledges: ground, fromX: 900), Goal(x: 540, y: 300)
        )
        XCTAssertEqual(
            followGoal(gaze: GazePoint(x: 400, y: 200), ledges: ground, fromX: 900), Goal(x: 510, y: 0)
        )
    }

    func testWaitingWalksToTheGoalAndStands() {
        let floor = ledges(windows: [], screen: screen)
        var rocky = Roamer(x: 500, y: 0)
        rocky.action = .stand
        let goal = Goal(x: 700, y: 0)
        var steps = 0
        while abs(rocky.x - goal.x) > Roamer.near, steps < 600 {
            rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .stay, goal: goal, random: { 0.99 })
            steps += 1
        }
        XCTAssertLessThan(steps, 600)
        XCTAssertEqual(rocky.facing, 1)
        rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .stay, goal: goal, random: { 0.99 })
        XCTAssertEqual(rocky.action, .stand)
        // 다 쉬는 중엔 따라가지 않는다.
        var sleeper = Roamer(x: 100, y: 0)
        sleeper.action = .stand
        sleeper.step(dt: 1, ledges: floor, walls: [], screen: screen, mood: .sleep, goal: goal, random: { 0 })
        XCTAssertEqual(sleeper.action, .sit)
        XCTAssertEqual(sleeper.x, 100)
    }

    func testWaitingPacesBesideTheGazeNowAndThen() {
        let floor = ledges(windows: [], screen: screen)
        let goal = Goal(x: 500, y: 0)
        var rocky = Roamer(x: 500, y: 0)
        rocky.action = .stand
        // 운이 안 맞으면 그 자리에 서 있다.
        rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .pace, goal: goal, random: { 0.99 })
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertEqual(rocky.x, 500)
        // 운이 맞으면 곁(`near`) 안의 다른 자리로 걸어가 다시 선다 — 눈길 곁을 떠나지 않는다.
        var walked = false
        var steps = 0
        repeat {
            rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .pace, goal: goal, random: { 0 })
            walked = walked || rocky.action == .walk
            XCTAssertLessThanOrEqual(abs(rocky.x - goal.x), Roamer.near + 1e-9)
            steps += 1
        } while !(walked && rocky.action == .stand) && steps < 300
        XCTAssertTrue(walked)
        XCTAssertEqual(rocky.x, goal.x - Roamer.near, accuracy: 1e-9)
        // 다다른 뒤엔 `paceRest` 동안 서 있는다.
        for _ in 0 ..< 60 {
            rocky.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .pace, goal: goal, random: { 0 })
            XCTAssertEqual(rocky.action, .stand)
        }
        // 창 옆면이 길을 막으면 그쪽으로 서성이지 않고, 연결 없음(`stay`)은 서성이지 않는다.
        let wall = Wall(x: 470, bottom: 0, top: 300, faces: 1)
        var blocked = Roamer(x: 500, y: 0)
        blocked.action = .stand
        var still = Roamer(x: 500, y: 0)
        still.action = .stand
        for _ in 0 ..< 60 {
            blocked.step(dt: 1 / 30, ledges: floor, walls: [wall], screen: screen, mood: .pace, goal: goal, random: { 0 })
            still.step(dt: 1 / 30, ledges: floor, walls: [], screen: screen, mood: .stay, goal: goal, random: { 0 })
        }
        XCTAssertEqual(blocked.x, 500)
        XCTAssertEqual(still.x, 500)
        XCTAssertEqual(still.action, .stand)
    }

    func testTubesToAGoalOnAnotherLedgeThenRests() {
        let ground = ledges(windows: [Box(x: 400, y: 0, width: 400, height: 300)], screen: screen)
        let goal = Goal(x: 600, y: 300)
        var rocky = Roamer(x: 100, y: 0)
        rocky.action = .stand
        // 서두르지 않는다 — 운이 안 맞으면 그 자리에 서서 기다린다.
        rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .stay, goal: goal, random: { 0.99 })
        XCTAssertEqual(rocky.action, .stand)
        XCTAssertNil(rocky.tube)
        rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .stay, goal: goal, random: { 0 })
        XCTAssertEqual(rocky.action, .tube)
        XCTAssertEqual(rocky.tube?.endX, 600)
        XCTAssertEqual(rocky.tube?.endY, 300)
        var steps = 0
        while rocky.action == .tube, steps < 1000 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .stay, goal: goal, random: { 0 })
            steps += 1
        }
        XCTAssertEqual(rocky.y, 300)
        XCTAssertEqual(rocky.x, 600, accuracy: 1e-9)
        // 눈길이 바로 다시 옮겨도 `followRest` 동안은 튜브를 또 놓지 않는다.
        let back = Goal(x: 100, y: 0)
        for _ in 0 ..< 30 {
            rocky.step(dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .stay, goal: back, random: { 0 })
        }
        XCTAssertNotEqual(rocky.action, .tube)
    }

    func testWorkingRoamsWithinTheLeashWithoutWanderingTubes() {
        let ground = ledges(windows: [Box(x: 600, y: 0, width: 300, height: 300)], screen: screen)
        var rocky = Roamer(x: 200, y: 0)
        rocky.action = .walk
        // 목줄 안이면 평소처럼 걷지만, 다른 창으로 건너가는 튜브는 놓지 않는다(goal 없으면 random 0 이면 놓는다).
        rocky.step(
            dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .roam, goal: Goal(x: 250, y: 0), random: { 0 }
        )
        XCTAssertNil(rocky.tube)
        XCTAssertNotEqual(rocky.action, .tube)
        // 목줄 밖이면 그쪽으로 돌아선다.
        var far = Roamer(x: 200, y: 0)
        far.action = .walk
        far.facing = -1
        far.step(
            dt: 1 / 30, ledges: ground, walls: [], screen: screen, mood: .roam, goal: Goal(x: 520, y: 0), random: { 0.99 }
        )
        XCTAssertEqual(far.facing, 1)
        XCTAssertGreaterThan(far.x, 200)
    }
}

final class ChatterTests: XCTestCase {
    func testPrimesSilentlyThenTellsStartsAndFinishes() {
        var chatter = Chatter()
        let idle = Session(sessionId: "a", name: "eel", status: "idle")
        let busy = Session(sessionId: "a", name: "eel", status: "busy")
        XCTAssertNil(chatter.update([busy], now: 0))
        XCTAssertNil(chatter.update([busy], now: 15))
        XCTAssertEqual(chatter.update([idle], now: 30), "eel 일을 마쳤어요")
        XCTAssertEqual(
            chatter.update([busy, Session(sessionId: "b", name: "fox", status: "busy")], now: 45), "eel 외 1개 일을 시작했어요"
        )
        // 못 읽은 목록은 지나간다.
        XCTAssertNil(chatter.update(nil, now: 60))
    }

    func testNudgesWhatIsRunningNowAndThen() {
        var chatter = Chatter()
        let busy = Session(sessionId: "a", name: "eel", status: "busy", job: JobSummary(detail: "테스트 고치는 중"))
        XCTAssertNil(chatter.update([busy], now: 0))
        XCTAssertNil(chatter.update([busy], now: Chatter.nudge - 1))
        XCTAssertEqual(chatter.update([busy], now: Chatter.nudge), "eel — 테스트 고치는 중")
        XCTAssertNil(chatter.update([busy], now: Chatter.nudge + 15))
        // 다 쉬면 조르지 않는다.
        XCTAssertNil(chatter.update([Session(sessionId: "c", name: "owl")], now: Chatter.nudge * 3))
    }

    func testAvatarStyleFallsBackToConcept() {
        XCTAssertEqual(AvatarStyle.resolve(nil), .concept)
        XCTAssertEqual(AvatarStyle.resolve(""), .concept)
        XCTAssertEqual(AvatarStyle.resolve("pixel"), .concept)
        // 비교용으로 잠깐 있던 값은 원작으로 돌아간다.
        XCTAssertEqual(AvatarStyle.resolve("minimal"), .concept)
        XCTAssertEqual(AvatarStyle.resolve("all"), .concept)
        XCTAssertEqual(AvatarStyle.resolve(" pebble "), .pebble)
        XCTAssertEqual(AvatarStyle.allCases, [.concept, .pebble])
    }

    func testExtraMovesOnlyWhenAsked() {
        XCTAssertFalse(Choreographer.breaks(at: .window).contains { $0.move.isExtra })
        let extras = Set(Choreographer.breaks(at: .window, extras: true).map(\.move).filter(\.isExtra))
        XCTAssertEqual(extras, Set(Move.allCases.filter(\.isExtra)))
        XCTAssertEqual(extras.count, 8)

        // 원작만 보이면 새 동작을 고르지 않는다.
        var plain = Choreographer()
        var moves = Choreographer()
        moves.extras = true
        var sawExtra = false
        for i in 0 ..< 400 {
            let r = Double(i % 97) / 97
            let now = Double(i) * 10
            if plain.update(now: now, mood: .working, spot: .window, random: { r }).move.isExtra { XCTFail("원작에 새 동작") }
            if moves.update(now: now, mood: .working, spot: .window, random: { r }).move.isExtra { sawExtra = true }
        }
        XCTAssertTrue(sawExtra)
    }
}

final class EyeCoverTests: XCTestCase {
    func testCoversWhileSecureInputIsOnAndUncoversAfter() {
        var cover = EyeCover()
        XCTAssertEqual(cover.update(secure: false, now: 0, dt: 0.1), 0)
        XCTAssertEqual(cover.update(secure: true, now: 1, dt: 0.15), 0.5, accuracy: 1e-9)
        XCTAssertEqual(cover.update(secure: true, now: 1.2, dt: 0.15), 1, accuracy: 1e-9)
        XCTAssertEqual(cover.update(secure: true, now: 1.3, dt: 0.15), 1)
        XCTAssertTrue(cover.covering)
        XCTAssertEqual(cover.update(secure: false, now: 2, dt: 0.15), 0.5, accuracy: 1e-9)
        XCTAssertEqual(cover.update(secure: false, now: 2.2, dt: 0.15), 0, accuracy: 1e-9)
        XCTAssertFalse(cover.covering)
    }

    func testGivesUpOnSecureInputLeftOn() {
        var cover = EyeCover()
        cover.update(secure: true, now: 0, dt: 1)
        XCTAssertEqual(cover.amount, 1)
        cover.update(secure: true, now: EyeCover.giveUp - 1, dt: 0.1)
        XCTAssertEqual(cover.amount, 1)
        cover.update(secure: true, now: EyeCover.giveUp, dt: 1)
        XCTAssertEqual(cover.amount, 0)
        // 한 번 껐다 켜면(새 비밀번호 칸) 다시 가린다.
        cover.update(secure: false, now: EyeCover.giveUp + 1, dt: 1)
        cover.update(secure: true, now: EyeCover.giveUp + 2, dt: 1)
        XCTAssertEqual(cover.amount, 1)
    }
}
