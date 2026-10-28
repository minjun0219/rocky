// 데몬 REST — 맥 앱이 쓰는 것만. 보내는 함수(`send`)를 주입받아 테스트가 가짜로 바꾼다. 모든 요청에 actor 와
// `x-rocky-client: mac` 을 붙인다 — 사용 로그가 맥 앱을 따로 센다(`rocky_core::usage::client_of` 가 헤더를 먼저 본다).
import Foundation
#if canImport(FoundationNetworking)
    import FoundationNetworking
#endif

/// 사용 로그의 클라이언트 이름.
public let clientName = "mac"

public struct DaemonError: Error, LocalizedError, Equatable {
    public var method: String
    public var path: String
    /// HTTP 상태 — 연결 자체가 실패했으면 nil.
    public var status: Int?
    public var message: String

    public init(method: String, path: String, status: Int?, message: String) {
        self.method = method
        self.path = path
        self.status = status
        self.message = message
    }

    /// 상태 줄에 그대로 뜬다 — 무엇을 어디로 보냈다가 무엇을 받았는지(TUI 와 같은 모양).
    public var errorDescription: String? {
        let code = status.map { " → \($0)" } ?? " →"
        return "\(method) \(path)\(code)\(message.isEmpty ? "" : " \(message)")"
    }
}

public struct DaemonClient: Sendable {
    public typealias Send = @Sendable (URLRequest) async throws -> (Data, URLResponse)

    /// `http://127.0.0.1:8636` — 끝 슬래시 없이.
    public var base: String
    public var actor: String
    private var send: Send

    public init(base: String, actor: String, send: Send? = nil) {
        self.base = base.hasSuffix("/") ? String(base.dropLast()) : base
        self.actor = actor
        self.send = send ?? { request in try await URLSession.shared.data(for: request) }
    }

    /// 헤더를 단 요청 — SSE 스트림도 이것으로 연다.
    public func request(_ method: String, _ path: String, body: [String: String]? = nil) -> URLRequest {
        // base 는 설정에서 오고 path 는 아래의 고정 경로 + 인코딩한 키라 URL 이 깨지지 않는다.
        var request = URLRequest(url: URL(string: base + path)!)
        request.httpMethod = method
        request.setValue(actor, forHTTPHeaderField: "x-rocky-actor")
        request.setValue(clientName, forHTTPHeaderField: "x-rocky-client")
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "content-type")
            request.httpBody = try? JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        }
        return request
    }

    private func call<T: Decodable>(_ method: String, _ path: String, body: [String: String]? = nil) async throws -> T {
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await send(request(method, path, body: body))
        } catch {
            throw DaemonError(method: method, path: path, status: nil, message: error.localizedDescription)
        }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200 ..< 300).contains(status) else {
            let payload = try? JSONDecoder().decode([String: String].self, from: data)
            throw DaemonError(method: method, path: path, status: status, message: payload?["error"] ?? "")
        }
        do {
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            throw DaemonError(method: method, path: path, status: status, message: "응답을 읽지 못했어요: \(error)")
        }
    }

    public func boards() async throws -> [Board] {
        try await call("GET", "/api/boards")
    }

    /// 전 보드의 할 일(보관 제외) — 메뉴의 "돌고 있는 일" 재료이자, 보고 있는 보드의 목록은 여기서 거른다(요청 하나로).
    public func todos() async throws -> [Todo] {
        try await call("GET", "/api/todos")
    }

    public func sections(board: String) async throws -> [Section] {
        try await call("GET", "/api/sections?board=\(Self.encode(board))")
    }

    /// 열린 PR 스냅숏 — 데몬이 감시하는 레포(보드에 설정된 것)만.
    public func openPrs() async throws -> [PrSnapshot] {
        try await call("GET", "/api/prs?open=true")
    }

    /// 이 기기의 에이전트 세션 — 데몬이 `claude agents --json` 을 몇 초 캐시해 두고 준다.
    public func sessions() async throws -> SessionsResult {
        try await call("GET", "/api/sessions")
    }

    @discardableResult
    public func setStatus(_ id: String, _ action: StatusAction) async throws -> Todo {
        try await call("POST", "/api/todos/\(Self.encode(id))/status", body: ["action": action.rawValue])
    }

    @discardableResult
    public func add(board: String, title: String) async throws -> Todo {
        try await call("POST", "/api/todos", body: ["board": board, "title": title])
    }

    static func encode(_ value: String) -> String {
        var allowed = CharacterSet.urlQueryAllowed
        allowed.remove(charactersIn: "&=?/#+")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
    }
}
