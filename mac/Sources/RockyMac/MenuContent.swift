// 메뉴 막대를 누르면 뜨는 창 — 연결 상태, 에이전트 세션, 돌고 있는 일(전 보드), 머지 후보 PR, 창 열기.
import AppKit
import RockyKit
import SwiftUI

struct MenuContent: View {
    let store: BoardStore
    @Bindable var agents: AgentStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("rocky").font(.headline)
                Spacer()
                ConnectionBadge(store: store)
            }
            if let error = store.error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
            }

            Divider()
            HStack {
                Text("에이전트").font(.subheadline.bold())
                Spacer()
                Toggle("돌아다니기", isOn: $agents.roam)
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .disabled(!agents.showAvatar)
                Toggle("아바타", isOn: $agents.showAvatar)
                    .toggleStyle(.switch)
                    .controlSize(.mini)
            }
            AgentList(agents: agents)

            Divider()
            Text("돌고 있는 일").font(.subheadline.bold())
            if !store.ready {
                Placeholder("읽는 중이에요…")
            } else if store.doing.isEmpty {
                Placeholder("돌고 있는 일이 없어요")
            } else {
                ForEach(store.doing) { todo in
                    DoingRow(todo: todo, store: store)
                }
            }

            Divider()
            Text("머지 후보 PR").font(.subheadline.bold())
            if store.readyPullRequests.isEmpty {
                Placeholder(store.ready ? "머지 후보가 없어요" : "읽는 중이에요…")
            } else {
                ForEach(store.readyPullRequests) { pr in
                    Button {
                        if let url = URL(string: pr.url) { openURL(url) }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("\(pr.repo) #\(pr.number)").font(.caption).foregroundStyle(.secondary)
                            Text(pr.title).lineLimit(2)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help(pr.url)
                }
            }

            Divider()
            HStack {
                Button("할 일 창 열기") {
                    NSApp.activate(ignoringOtherApps: true)
                    openWindow(id: BoardWindow.id)
                }
                .keyboardShortcut("o")
                Button("웹 UI") {
                    if let url = store.webURL { openURL(url) }
                }
                Spacer()
                Button("끝내기") { NSApp.terminate(nil) }
                    .keyboardShortcut("q")
            }
        }
        .padding(14)
        .frame(width: 340)
        .onAppear { agents.refresh() }
    }
}

/// 돌고 있는 일 한 줄 — 참조·제목·누가, 그리고 멈춤·완료.
private struct DoingRow: View {
    let todo: Todo
    let store: BoardStore

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text(todo.title).lineLimit(2)
                Text([todo.ref, todo.doingBy].compactMap { $0 }.joined(separator: " · "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 4)
            Button { store.act(todo, .stop) } label: { Image(systemName: "pause.circle") }
                .buttonStyle(.borderless)
                .help("멈춤")
            Button { store.act(todo, .done) } label: { Image(systemName: "checkmark.circle") }
                .buttonStyle(.borderless)
                .help("완료")
        }
    }
}

struct ConnectionBadge: View {
    let store: BoardStore

    var body: some View {
        if store.connected {
            Label("연결됨", systemImage: "circle.fill")
                .font(.caption)
                .foregroundStyle(.green)
        } else if let retry = store.retryIn {
            Label("\(Int(retry))초 뒤 다시 붙어요", systemImage: "circle")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else {
            Label("붙는 중", systemImage: "circle")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}

struct Placeholder: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text).font(.callout).foregroundStyle(.secondary)
    }
}

/// 메뉴의 에이전트 칸 — 내 차례 → 실행 중 → 쉬는 중, 끝난 세션은 뺀다. 누르면 웹 에이전트 탭(메시지·멈추기는 거기서).
private struct AgentList: View {
    let agents: AgentStore
    @Environment(\.openURL) private var openURL

    /// 메뉴가 길어지지 않게 — 나머지는 "N개 더" 로 웹에.
    private static let limit = 6

    var body: some View {
        if let error = agents.error {
            Placeholder(error)
        } else if let result = agents.result, !result.available {
            Placeholder(result.reason ?? "세션 목록을 얻지 못했어요")
        } else if agents.result == nil {
            Placeholder("읽는 중이에요…")
        } else if agents.rows.isEmpty {
            Placeholder("떠 있는 세션이 없어요")
        } else {
            ForEach(agents.rows.prefix(Self.limit)) { session in
                Button {
                    if let url = agents.agentsURL { openURL(url) }
                } label: {
                    AgentRow(session: session)
                }
                .buttonStyle(.plain)
            }
            if agents.rows.count > Self.limit {
                Button("\(agents.rows.count - Self.limit)개 더 — 웹 에이전트 탭") {
                    if let url = agents.agentsURL { openURL(url) }
                }
                .buttonStyle(.link)
                .font(.caption)
            }
        }
    }
}

private struct AgentRow: View {
    let session: Session

    var body: some View {
        let phase = agentPhase(session)
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Circle()
                .fill(color(phase))
                .frame(width: 7, height: 7)
            VStack(alignment: .leading, spacing: 2) {
                Text("\(session.name) · \(repoLabel(session.cwd))").lineLimit(1)
                Text(agentLine(session) ?? phase.label)
                    .font(.caption)
                    .foregroundStyle(phase == .blocked ? Color.orange : .secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }

    private func color(_ phase: AgentPhase) -> Color {
        switch phase {
        case .blocked: .orange
        case .run: .green
        case .idle, .done: .secondary
        }
    }
}
