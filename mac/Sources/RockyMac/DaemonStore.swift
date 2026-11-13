// 맥 앱 상태 — 데몬에서 열린 PR 을 받아 들고, SSE 프레임이 PR 을 건드렸을 때만 다시 받는다. 저장하지 않는다.
import Foundation
import Observation
import RockyKit

@MainActor
@Observable
final class DaemonStore {
    private(set) var prs: [PrSnapshot] = []

    /// SSE 가 붙어 있나.
    private(set) var connected = false
    /// 끊겼으면 다시 붙기까지(초).
    private(set) var retryIn: Double?
    /// 첫 데이터를 받았나 — 그 전엔 빈 화면 대신 "읽는 중".
    private(set) var ready = false
    /// 마지막 읽기 실패 — 다음 읽기 성공이 지운다.
    private(set) var error: String?

    let client: DaemonClient
    private var pending: [String] = []
    private var debounce: Task<Void, Never>?
    private var stream: Task<Void, Never>?

    init(settings: DaemonSettings) {
        client = DaemonClient(base: settings.base, actor: settings.actor)
    }

    var webURL: URL? { URL(string: client.base + "/") }
    var readyPullRequests: [PrSnapshot] { readyPrs(prs) }

    func start() {
        guard stream == nil else { return }
        stream = Task { await self.runStream() }
    }

    func load() async {
        do {
            prs = try await client.openPrs()
            ready = true
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// 프레임을 150ms 로 모아 PR 을 건드렸을 때만 다시 받는다(웹 `main.tsx` 와 같은 창).
    private func queue(_ frame: String) {
        pending.append(frame)
        debounce?.cancel()
        debounce = Task {
            try? await Task.sleep(for: .milliseconds(150))
            guard !Task.isCancelled else { return }
            let frames = pending
            pending.removeAll()
            if touchesPrs(frames) { await load() }
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
                    self.error = error.localizedDescription
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
