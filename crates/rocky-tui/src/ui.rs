//! 렌더 — `App` 을 읽기만 한다. 한 창, 위 탭줄 · 왼쪽 목록 · 오른쪽 상세 · 아래 상태줄. 피커는 오버레이.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use rocky_core::refs::TodoView;
use rocky_core::types::TodoStatus;

use crate::app::{status_glyph, App, InboxRow, Row, Tab};

pub fn render(frame: &mut Frame, app: &App) {
    let [tabs, main, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
    render_tabs(frame, app, tabs);
    match app.tab {
        Tab::Board => {
            render_list(frame, app, left);
            render_detail(frame, app, right);
        }
        Tab::Inbox => {
            render_inbox(frame, app, left);
            render_inbox_detail(frame, app, right);
        }
    }
    render_status(frame, app, status);
    if let Some(picker) = &app.picker {
        render_picker(frame, picker, main);
    }
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let on = Style::new().reversed();
    let off = Style::new().dim();
    let mut spans: Vec<Span> = vec![Span::styled(" rocky ", Style::new().bold())];
    spans.push(Span::styled(
        "[보드]",
        if app.tab == Tab::Board { on } else { off },
    ));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        "[수집함]",
        if app.tab == Tab::Inbox { on } else { off },
    ));
    spans.push(Span::raw("  "));
    if app.boards.is_empty() {
        spans.push(Span::styled(format!("[{}]", app.board), on));
    }
    for board in &app.boards {
        if board.key == app.board {
            spans.push(Span::styled(format!("[{}]", board.key), on));
        } else {
            spans.push(Span::styled(format!(" {} ", board.key), off));
        }
    }
    if !app.daemon_ok {
        spans.push(Span::styled(
            "  데몬 없음 — 재시도 중",
            Style::new().fg(Color::Red).bold(),
        ));
    } else if !app.connected {
        spans.push(Span::styled(
            "  SSE 끊김 — 재연결 중",
            Style::new().fg(Color::Yellow),
        ));
    }
    frame.render_widget(Line::from(spans), area);
}

fn todo_line(app: &App, todo: &TodoView) -> Line<'static> {
    let t = &todo.todo;
    let glyph = status_glyph(todo);
    let glyph_style = match t.status {
        TodoStatus::Doing => Style::new().fg(Color::Green),
        TodoStatus::Done => Style::new().dim(),
        TodoStatus::Todo => Style::new(),
    };
    let title_style = if t.status == TodoStatus::Done {
        Style::new().dim().add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::new()
    };
    let mut spans = vec![
        Span::styled(format!("{glyph} "), glyph_style),
        Span::styled(format!("{:<4}", t.number), Style::new().dim()),
        Span::styled(t.title.clone(), title_style),
        Span::styled(format!(" {}", t.priority.as_str()), Style::new().dim()),
    ];
    for label in &t.labels {
        spans.push(Span::styled(
            format!(" [{label}]"),
            Style::new().fg(Color::Cyan),
        ));
    }
    if todo.comment_count > 0 {
        spans.push(Span::styled(
            format!(" 💬{}", todo.comment_count),
            Style::new().dim(),
        ));
    }
    let open = app.open_handoffs_for(&t.id);
    if open > 0 {
        spans.push(Span::styled(
            format!(" ⇢{open}"),
            Style::new().fg(Color::Magenta),
        ));
    }
    Line::from(spans)
}

fn render_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| match row {
            Row::Header(title) => ListItem::new(Line::from(Span::styled(
                format!("# {title}"),
                Style::new().bold().fg(Color::Blue),
            ))),
            Row::Todo(i) => ListItem::new(todo_line(app, &app.todos[*i])),
        })
        .collect();
    let title = format!(" {} · {}개 ", app.board, app.todos.len());
    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(Style::new().reversed())
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected(app.selected);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" 상세 ");
    let Some(todo) = app.selected_todo() else {
        frame.render_widget(
            Paragraph::new("항목을 고르면 여기에 상세가 뜬다").block(block),
            area,
        );
        return;
    };
    let t = &todo.todo;
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(todo.r#ref.clone(), Style::new().bold()),
            Span::raw("  "),
            Span::styled(t.title.clone(), Style::new().bold()),
        ]),
        Line::from(format!(
            "{} {} · {}{}",
            status_glyph(todo),
            t.status.as_str(),
            t.priority.as_str(),
            t.due
                .as_deref()
                .map(|d| format!(" · due {d}"))
                .unwrap_or_default()
        )),
    ];
    if let Some(by) = &t.doing_by {
        let state = todo
            .doing_state
            .map(|s| format!(" ({s:?})").to_lowercase())
            .unwrap_or_default();
        lines.push(Line::from(format!("진행중: {by}{state}")));
    }
    let open = app.open_handoffs_for(&t.id);
    if open > 0 {
        lines.push(Line::from(Span::styled(
            format!("핸드오프 대기 {open} — 세션이 다음 턴에 집어간다"),
            Style::new().fg(Color::Magenta),
        )));
    }
    if !t.labels.is_empty() {
        lines.push(Line::from(format!("라벨: {}", t.labels.join(", "))));
    }
    for link in &t.links {
        let label = link.title.as_deref().unwrap_or(link.url.as_str());
        let mut spans = vec![
            Span::raw("↗ "),
            Span::styled(label.to_string(), Style::new().fg(Color::Cyan)),
        ];
        if let Some(gh) = app.gh_line(&link.url) {
            spans.push(Span::styled(format!("  {gh}"), Style::new().dim()));
        }
        lines.push(Line::from(spans));
    }
    if !t.description.trim().is_empty() {
        lines.push(Line::from(""));
        for l in t.description.lines() {
            lines.push(Line::from(l.to_string()));
        }
    }
    if let Some(detail) = app.detail.as_ref().filter(|d| d.todo.r#ref == todo.r#ref) {
        if !detail.comments.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("댓글 {}", detail.comments.len()),
                Style::new().bold(),
            )));
            for c in detail.comments.iter().rev().take(5) {
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} · {}",
                        c.actor,
                        &c.created_at[..c.created_at.len().min(16)]
                    ),
                    Style::new().dim(),
                )));
                for l in c.body.lines() {
                    lines.push(Line::from(format!("  {l}")));
                }
            }
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_inbox(frame: &mut Frame, app: &App, area: Rect) {
    let Some(inbox) = &app.inbox else {
        frame.render_widget(
            Paragraph::new("수집함을 불러오는 중…").block(Block::bordered().title(" 수집함 ")),
            area,
        );
        return;
    };
    if inbox.sources.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "등록된 수집함이 없다 — rocky.json 의 todo.inbox[] 에 어댑터를 추가한다 (docs/board.md \"수집함\")",
            )
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title(" 수집함 ")),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .inbox_rows
        .iter()
        .map(|row| match row {
            InboxRow::Source(s) => {
                let source = &inbox.sources[*s];
                if source.available {
                    ListItem::new(Line::from(Span::styled(
                        format!("# {} ({})", source.name, source.items.len()),
                        Style::new().bold().fg(Color::Blue),
                    )))
                } else {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("# {} ", source.name),
                            Style::new().bold().fg(Color::Red),
                        ),
                        Span::styled(
                            format!("— {}", source.reason.as_deref().unwrap_or("실패")),
                            Style::new().dim(),
                        ),
                    ]))
                }
            }
            InboxRow::Item(s, i) => {
                let item = &inbox.sources[*s].items[*i];
                let promoted = app.is_promoted(item);
                let mut spans = vec![Span::styled(
                    if promoted { "✓ " } else { "○ " }.to_string(),
                    if promoted {
                        Style::new().dim()
                    } else {
                        Style::new()
                    },
                )];
                spans.push(Span::styled(
                    item.title.clone(),
                    if promoted {
                        Style::new().dim()
                    } else {
                        Style::new()
                    },
                ));
                if let Some(due) = &item.due {
                    spans.push(Span::styled(format!(" {due}"), Style::new().dim()));
                }
                if promoted {
                    spans.push(Span::styled(" 올라감", Style::new().dim()));
                }
                ListItem::new(Line::from(spans))
            }
        })
        .collect();
    let total: usize = inbox.sources.iter().map(|s| s.items.len()).sum();
    let list = List::new(items)
        .block(Block::bordered().title(format!(" 수집함 · {total}개 ")))
        .highlight_style(Style::new().reversed())
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected(app.inbox_selected);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_inbox_detail(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" 항목 ");
    let Some((source, item)) = app.selected_inbox_item() else {
        frame.render_widget(
            Paragraph::new("항목을 고르면 여기에 내용이 뜬다. p 로 보드에 올린다.").block(block),
            area,
        );
        return;
    };
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(item.title.clone(), Style::new().bold())),
        Line::from(Span::styled(
            format!("{} · {}", source.name, item.id),
            Style::new().dim(),
        )),
    ];
    if let Some(due) = &item.due {
        lines.push(Line::from(format!("due {due}")));
    }
    if let Some(url) = &item.url {
        lines.push(Line::from(vec![
            Span::raw("↗ "),
            Span::styled(url.clone(), Style::new().fg(Color::Cyan)),
        ]));
    }
    if app.is_promoted(item) {
        lines.push(Line::from(Span::styled(
            format!("이미 {} 보드에 올라가 있다", app.board),
            Style::new().fg(Color::Green),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("p — {} 보드 백로그로 올린다", app.board),
            Style::new().dim(),
        )));
    }
    if let Some(note) = item.note.as_deref().filter(|n| !n.trim().is_empty()) {
        lines.push(Line::from(""));
        for l in note.lines() {
            lines.push(Line::from(l.to_string()));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_picker(frame: &mut Frame, picker: &crate::app::Picker, area: Rect) {
    let height = (picker.choices.len() as u16 + 2)
        .min(area.height.saturating_sub(2))
        .max(3);
    let [popup] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::horizontal([Constraint::Percentage(70)])
        .flex(Flex::Center)
        .areas(popup);
    frame.render_widget(Clear, popup);
    let items: Vec<ListItem> = picker
        .choices
        .iter()
        .map(|c| {
            let mark = if c.matched { "* " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::raw(mark.to_string()),
                Span::styled(c.name.clone(), Style::new().bold()),
                Span::styled(format!("  {}  {}", c.status, c.cwd), Style::new().dim()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(format!(
            " {} 를 넘길 세션 — Enter 선택 · Esc 취소 ",
            picker.todo_ref
        )))
        .highlight_style(Style::new().reversed())
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected(Some(picker.selected));
    frame.render_stateful_widget(list, popup, &mut state);
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let help = match app.tab {
        Tab::Board => " j/k 이동 · s/x/d/o/a 상태 · h 핸드오프 · n 새 세션 · i 이슈 · [ ] 보드 · Tab 수집함 · r 새로고침 · q 종료",
        Tab::Inbox => " j/k 이동 · p 보드로 올리기 · Tab 보드 · r 새로고침(어댑터 다시 실행) · q 종료",
    };
    let text = match &app.notice {
        Some(notice) => notice.clone(),
        None => help.into(),
    };
    let style = if app.notice.is_some() {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new().dim()
    };
    frame.render_widget(Line::from(Span::styled(text, style)), area);
}
