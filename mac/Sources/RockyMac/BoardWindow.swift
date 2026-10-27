// 할 일 창 — 왼쪽은 보드, 오른쪽은 그 보드의 섹션별 트리(열린 것만)와 빠른 추가. 쓰기는 보내고 SSE 를 기다린다.
import AppKit
import RockyKit
import SwiftUI

struct BoardWindow: View {
    static let id = "board"

    let store: BoardStore
    @State private var draft = ""

    var body: some View {
        NavigationSplitView {
            List(store.boards, selection: Binding(get: { store.boardKey }, set: { store.select(board: $0) })) { board in
                Text(board.title).tag(Optional(board.key))
            }
            .navigationSplitViewColumnWidth(min: 140, ideal: 180)
        } detail: {
            VStack(spacing: 0) {
                TextField("할 일 추가 — Enter", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        store.add(title: draft)
                        draft = ""
                    }
                    .disabled(store.board == nil)
                    .padding(10)
                Divider()
                content
                StatusBar(store: store)
            }
            .navigationTitle(store.board?.title ?? "rocky")
        }
    }

    @ViewBuilder private var content: some View {
        if !store.ready {
            Centered("읽는 중이에요…")
        } else if store.board == nil {
            Centered("보드가 없어요")
        } else if store.sectionsBoard != store.boardKey {
            // 섹션 없이 그리면 섹션에 든 할 일이 다 빠져 "없어요" 로 보인다 — 섹션이 올 때까지 가린다.
            Centered("읽는 중이에요…")
        } else if store.groups.isEmpty {
            Centered("열린 할 일이 없어요")
        } else {
            List {
                ForEach(store.groups) { group in
                    SwiftUI.Section("\(group.title) \(group.rows.count)") {
                        ForEach(group.rows) { row in
                            TodoRowView(row: row, store: store)
                        }
                    }
                }
            }
        }
    }
}

private struct TodoRowView: View {
    let row: TodoRow
    let store: BoardStore

    var body: some View {
        let todo = row.todo
        HStack(spacing: 8) {
            Image(systemName: symbol(todo.status))
                .foregroundStyle(todo.status == .doing ? Color.accentColor : .secondary)
            Text("#\(todo.number)").monospacedDigit().foregroundStyle(.secondary)
            Text(todo.title)
                .strikethrough(todo.status == .done)
                .lineLimit(2)
            if let by = todo.doingBy, todo.status == .doing {
                Text(by).font(.caption).foregroundStyle(.secondary)
            }
            if todo.commentCount > 0 {
                Label("\(todo.commentCount)", systemImage: "text.bubble")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 4)
            ForEach(StatusAction.available(for: todo.status), id: \.self) { action in
                Button(action.label) { store.act(todo, action) }
                    .buttonStyle(.borderless)
                    .controlSize(.small)
            }
        }
        .padding(.leading, CGFloat(row.depth) * 16)
        .contextMenu {
            ForEach(StatusAction.available(for: todo.status), id: \.self) { action in
                Button(action.label) { store.act(todo, action) }
            }
            Divider()
            Button("참조 복사 (\(todo.ref))") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(todo.ref, forType: .string)
            }
        }
    }

    private func symbol(_ status: TodoStatus) -> String {
        switch status {
        case .todo: "circle"
        case .doing: "circle.lefthalf.filled"
        case .done: "checkmark.circle.fill"
        }
    }
}

/// 창 아래 한 줄 — 실패가 있으면 그것, 아니면 방금 한 일, 그리고 연결 상태.
private struct StatusBar: View {
    let store: BoardStore

    var body: some View {
        HStack {
            if let error = store.error {
                Text(error).foregroundStyle(.red).lineLimit(1).textSelection(.enabled)
            } else if let notice = store.notice {
                Text(notice).foregroundStyle(.secondary)
            }
            Spacer()
            ConnectionBadge(store: store)
        }
        .font(.caption)
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(.bar)
    }
}

private struct Centered: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Placeholder(text).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
