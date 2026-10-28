// 화면에 그릴 묶음 — 순수 함수. 보드 트리는 TUI `tui/rows.ts` 의 `buildLines`(웹 TodoPane)와 같은 규칙이다:
// 부모가 이 목록에 있으면 그 아래로, 끝난 것만 남은 가지는 숨긴다, 섹션 없는 것은 "일반" 으로 맨 앞에.
import Foundation

public struct TodoRow: Identifiable, Hashable, Sendable {
    public var todo: Todo
    public var depth: Int
    public var id: String { todo.id }
}

public struct TodoGroup: Identifiable, Hashable, Sendable {
    /// 섹션 id — 섹션 없는 묶음은 `__none`.
    public var id: String
    public var title: String
    public var rows: [TodoRow]
}

/// 보고 있는 보드의 섹션별 트리. `todos` 는 전 보드의 것(보관 제외) — 이 보드 것만 고른다.
public func boardGroups(todos: [Todo], boardId: String, sections: [Section]) -> [TodoGroup] {
    let mine = todos.filter { $0.boardId == boardId }
    let ids = Set(mine.map(\.id))
    var children: [String: [Todo]] = [:]
    var roots: [Todo] = []
    for todo in mine {
        if let parent = todo.parentId, ids.contains(parent) {
            children[parent, default: []].append(todo)
        } else {
            roots.append(todo)
        }
    }
    func isOpen(_ todo: Todo) -> Bool {
        todo.status != .done || (children[todo.id] ?? []).contains(where: isOpen)
    }
    func walk(_ items: [Todo], _ depth: Int, into rows: inout [TodoRow]) {
        for todo in items where isOpen(todo) {
            rows.append(TodoRow(todo: todo, depth: depth))
            walk(children[todo.id] ?? [], depth + 1, into: &rows)
        }
    }

    var groups: [(id: String, title: String, items: [Todo])] = []
    let loose = roots.filter { $0.sectionId == nil }
    if !loose.isEmpty {
        groups.append(("__none", "일반", loose))
    }
    for section in sections {
        let items = roots.filter { $0.sectionId == section.id }
        if !items.isEmpty {
            groups.append((section.id, section.title, items))
        }
    }
    return groups.compactMap { group in
        var rows: [TodoRow] = []
        walk(group.items, 0, into: &rows)
        return rows.isEmpty ? nil : TodoGroup(id: group.id, title: group.title, rows: rows)
    }
}

/// 메뉴의 "돌고 있는 일" — 전 보드의 doing. 데몬이 준 순서 그대로.
public func doingTodos(_ todos: [Todo]) -> [Todo] {
    todos.filter { $0.status == .doing }
}

/// 메뉴의 "머지 후보" — 열린 PR 중 데몬이 ready 로 판정한 것. 머지는 사람 몫이고, 알림(배너)은 데몬이 이미 띄운다.
public func readyPrs(_ prs: [PrSnapshot]) -> [PrSnapshot] {
    prs.filter { $0.state == "OPEN" && $0.ready && !$0.isDraft }
}

