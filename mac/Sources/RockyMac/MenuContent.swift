// 메뉴 막대를 누르면 뜨는 창 — 연결 상태, 돌고 있는 일(전 보드), 머지 후보 PR, 창 열기.
import AppKit
import RockyKit
import SwiftUI

struct MenuContent: View {
    let store: BoardStore
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
