// SSE `/api/events` — 줄을 프레임으로 모으고, 프레임이 PR 을 건드렸을 때만 다시 받는다. 다시 붙는 간격은 TUI(`tui/api.ts`)와 같다.
import Foundation

/// SSE 줄 파서 — `data:` 줄을 모았다가 빈 줄에서 한 프레임으로 내놓는다. 주석 프레임(`: connected`)은 버린다.
public struct SSEParser: Sendable {
    private var data: [String] = []

    public init() {}

    /// 한 줄(개행 없이)을 먹인다. 프레임이 끝났으면 그 `data` 를 돌려준다.
    public mutating func feed(line raw: String) -> String? {
        let line = raw.hasSuffix("\r") ? String(raw.dropLast()) : raw
        if line.isEmpty {
            defer { data.removeAll() }
            return data.isEmpty ? nil : data.joined(separator: "\n")
        }
        if line.hasPrefix("data:") {
            var value = line.dropFirst(5)
            if value.hasPrefix(" ") {
                value = value.dropFirst()
            }
            data.append(String(value))
        }
        return nil
    }
}

/// 프레임들이 열린 PR 을 건드렸을 수 있나 — 맥 앱이 드는 것은 PR 뿐이다. 할 일 · 섹션 · 노트 프레임만 건너뛰고, 모르는 모양이면
/// 다시 받는다.
public func touchesPrs(_ frames: [String]) -> Bool {
    for frame in frames {
        guard let event = try? JSONDecoder().decode(ChangeEvent.self, from: Data(frame.utf8)) else {
            return true
        }
        // 보드 행의 PR 전이(`pr-*`)든 다른 무엇이든 다시 받는다 — 열린 PR 하나 읽기라 싸다.
        if !["todo", "section", "note"].contains(event.entity) {
            return true
        }
    }
    return false
}

/// 이만큼 붙어 있었으면 다시 붙기 간격을 처음(1s)으로 되돌린다 — 받자마자 닫히는 서버에 1초마다 붙지 않게.
public let stableSeconds: Double = 10

/// 다음 대기에 쓸 시도 번호 — 한동안 붙어 있다가 끊긴 것만 처음으로 되돌린다.
public func nextAttempt(_ attempt: Int, openedFor: Double?) -> Int {
    if let openedFor, openedFor >= stableSeconds {
        return 0
    }
    return attempt
}

/// 다시 붙기까지 기다리는 시간(초) — 1s 에서 두 배씩, 30s 까지.
public func backoffSeconds(_ attempt: Int) -> Double {
    min(30, pow(2, Double(max(0, attempt))))
}
