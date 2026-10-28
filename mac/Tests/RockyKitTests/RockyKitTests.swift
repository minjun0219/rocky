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
