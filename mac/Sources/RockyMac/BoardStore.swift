// 맥 앱 상태 — 데몬에서 받아 들고, SSE 로 바뀐 묶음만 다시 받는다. 저장하지 않고 낙관적 갱신도 없다:
// 쓰기는 REST 로 보내고 화면은 SSE 를 기다린다(TUI `tui/store.ts` 와 같은 규칙).
import Foundation
import Observation
import RockyKit

@MainActor
@Observable
final class BoardStore {
    private(set) var boards: [Board] = []
    private(set) var todos: [Todo] = []
    private(set) var sections: [Section] = []
    /// `sections` 가 어느 보드 것인가 — 보드를 바꾼 직후 새 섹션이 오기 전엔 `boardKey` 와 다르다(화면은 "읽는 중").
    private(set) var sectionsBoard: String?
    private(set) var prs: [PrSnapshot] = []
    /// 보고 있는 보드의 key — 다음 실행에도 이어서 본다.
    var boardKey: String? {
        didSet { UserDefaults.standard.set(boardKey, forKey: "board") }
    }

    /// SSE 가 붙어 있나.
    private(set) var connected = false
    /// 끊겼으면 다시 붙기까지(초).
    private(set) var retryIn: Double?
    /// 첫 데이터를 받았나 — 그 전엔 빈 화면 대신 "읽는 중".
    private(set) var ready = false
    /// 마지막 실패 — 읽기 실패는 다음 읽기 성공이, 동작 실패는 다음 동작이 지운다.
    private(set) var error: String?
    private var errorFromLoad = false
    /// 방금 한 일의 한 줄.
    private(set) var notice: String?

    let client: DaemonClient
    private var pending: [String] = []
    private var debounce: Task<Void, Never>?
    private var stream: Task<Void, Never>?

    init(settings: DaemonSettings) {
        client = DaemonClient(base: settings.base, actor: settings.actor)
        boardKey = UserDefaults.standard.string(forKey: "board")
    }

    var webURL: URL? { URL(string: client.base + "/") }
    var doing: [Todo] { doingTodos(todos) }
    var readyPullRequests: [PrSnapshot] { readyPrs(prs) }
    var board: Board? { boards.first { $0.key == boardKey } }
    var groups: [TodoGroup] {
        guard let board, sectionsBoard == board.key else { return [] }
        return boardGroups(todos: todos, boardId: board.id, sections: sections)
    }

    func start() {
        guard stream == nil else { return }
        stream = Task { await self.runStream() }
    }

    func load(_ slices: Set<Slice> = Set(Slice.allCases)) async {
        let current = boardKey
        let client = client
        do {
            async let boardsCall = maybe(slices.contains(.boards)) { try await client.boards() }
            async let todosCall = maybe(slices.contains(.todos)) { try await client.todos() }
            async let prsCall = maybe(slices.contains(.prs)) { try await client.openPrs() }
            let (boards, todos, prs) = try await (boardsCall, todosCall, prsCall)
            // 처음이거나 보던 보드가 없어졌으면(이름 바뀜·보관) 첫 보드로.
            let known = boards ?? self.boards
            let key = boards != nil && !known.contains(where: { $0.key == current }) ? known.first?.key : current
            var sections: [Section]?
            if slices.contains(.sections) || key != sectionsBoard {
                sections = try await loadSections(key)
            }
            // 받는 사이 사람이 보드를 바꿨으면 그 선택을 두고, 옛 보드 기준으로 받은 섹션은 버린다.
            let switched = boardKey != current
            if let boards { self.boards = boards }
            if boards != nil, !switched, key != boardKey { boardKey = key }
            if let todos { self.todos = todos }
            if let prs { self.prs = prs }
            if let sections, !switched {
                self.sections = sections
                sectionsBoard = key
            }
            ready = true
            if errorFromLoad { clearError() }
        } catch {
            fail(error, fromLoad: true)
        }
    }

    func select(board key: String?) {
        guard key != boardKey else { return }
        boardKey = key
        Task {
            do {
                let sections = try await loadSections(key)
                // 받는 사이 또 바꿨으면 늦게 온 것을 버린다.
                if boardKey == key {
                    self.sections = sections
                    sectionsBoard = key
                }
            } catch {
                fail(error, fromLoad: true)
            }
        }
    }

    func act(_ todo: Todo, _ action: StatusAction) {
        Task {
            do {
                try await client.setStatus(todo.id, action)
                clearError()
                notice = "#\(todo.number) \(action.label)"
            } catch {
                fail(error, fromLoad: false)
            }
        }
    }

    func add(title raw: String) {
        let title = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty, let key = boardKey else { return }
        Task {
            do {
                let added = try await client.add(board: key, title: title)
                clearError()
                notice = "#\(added.number) 추가"
            } catch {
                fail(error, fromLoad: false)
            }
        }
    }

    private func loadSections(_ key: String?) async throws -> [Section] {
        guard let key else { return [] }
        return try await client.sections(board: key)
    }

    private func fail(_ error: Error, fromLoad: Bool) {
        self.error = error.localizedDescription
        errorFromLoad = fromLoad
    }

    private func clearError() {
        error = nil
        errorFromLoad = false
    }

    /// 프레임을 150ms 로 모아 건드린 묶음만 다시 받는다(웹 `main.tsx`·TUI 와 같은 창).
    private func queue(_ frame: String) {
        pending.append(frame)
        debounce?.cancel()
        debounce = Task {
            try? await Task.sleep(for: .milliseconds(150))
            guard !Task.isCancelled else { return }
            let frames = pending
            pending.removeAll()
            await load(slices(for: frames))
        }
    }

    /// `/api/events` 를 열어 둔다 — 끊기면 1s→2s→…30s 로 다시 붙고, 붙을 때마다(첫 연결도) 전부 다시 받는다:
    /// 끊긴 동안의 변경은 오지 않는다.
    private func runStream() async {
        var attempt = 0
        while !Task.isCancelled {
            var openedAt: Date?
            do {
                var request = client.request("GET", "/api/events")
                // 데몬은 바뀐 것이 있을 때만 보낸다 — 기본 60초 무응답 제한이면 조용한 동안 계속 끊긴다.
                request.timeoutInterval = 86_400
                let (bytes, response) = try await URLSession.shared.bytes(for: request)
                guard (response as? HTTPURLResponse)?.statusCode == 200 else {
                    throw DaemonError(
                        method: "GET",
                        path: "/api/events",
                        status: (response as? HTTPURLResponse)?.statusCode,
                        message: ""
                    )
                }
                openedAt = Date()
                connected = true
                retryIn = nil
                await load()
                var parser = SSEParser()
                var line: [UInt8] = []
                // 줄은 바이트로 직접 자른다 — `bytes.lines` 는 빈 줄(프레임 끝)을 넘겨주지 않는다.
                for try await byte in bytes {
                    if byte == UInt8(ascii: "\n") {
                        if let frame = parser.feed(line: String(decoding: line, as: UTF8.self)) {
                            queue(frame)
                        }
                        line.removeAll(keepingCapacity: true)
                    } else {
                        line.append(byte)
                    }
                }
            } catch {
                // 아래에서 다시 붙는다. 데몬이 꺼져 있으면 읽기 실패로 보인다.
                if openedAt == nil, !ready {
                    fail(error, fromLoad: true)
                }
            }
            if Task.isCancelled { return }
            connected = false
            attempt = nextAttempt(attempt, openedFor: openedAt.map { Date().timeIntervalSince($0) })
            let wait = backoffSeconds(attempt)
            attempt += 1
            retryIn = wait
            try? await Task.sleep(for: .seconds(wait))
        }
    }
}

/// 원하면 부르고 아니면 nil — `async let` 셋을 나란히 띄우려고.
private func maybe<T: Sendable>(_ want: Bool, _ call: @Sendable () async throws -> T) async throws -> T? {
    guard want else { return nil }
    return try await call()
}
