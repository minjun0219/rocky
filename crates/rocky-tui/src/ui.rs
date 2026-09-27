//! 렌더 — `App` 을 읽기만 한다. 한 창, 위 보드 탭 · 왼쪽 목록 · 오른쪽 상세 · 아래 상태줄.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use rocky_core::refs::TodoView;
use rocky_core::types::TodoStatus;

use crate::app::{status_glyph, App, Row};

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
    render_list(frame, app, left);
    render_detail(frame, app, right);
    render_status(frame, app, status);
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = vec![Span::styled(" rocky ", Style::new().bold())];
    if app.boards.is_empty() {
        spans.push(Span::styled(
            format!("[{}]", app.board),
            Style::new().reversed(),
        ));
    }
    for board in &app.boards {
        if board.key == app.board {
            spans.push(Span::styled(
                format!("[{}]", board.key),
                Style::new().reversed(),
            ));
        } else {
            spans.push(Span::styled(format!(" {} ", board.key), Style::new().dim()));
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

fn todo_line(todo: &TodoView) -> Line<'static> {
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
    ];
    spans.push(Span::styled(
        format!(" {}", t.priority.as_str()),
        Style::new().dim(),
    ));
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
            Row::Todo(i) => ListItem::new(todo_line(&app.todos[*i])),
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
    if !t.labels.is_empty() {
        lines.push(Line::from(format!("라벨: {}", t.labels.join(", "))));
    }
    for link in &t.links {
        let label = link.title.as_deref().unwrap_or(link.url.as_str());
        lines.push(Line::from(vec![
            Span::raw("↗ "),
            Span::styled(label.to_string(), Style::new().fg(Color::Cyan)),
        ]));
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

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let text = match &app.notice {
        Some(notice) => notice.clone(),
        None => " j/k 이동 · s start · x stop · d done · o reopen · a archive · Tab 보드 · r 새로고침 · q 종료".into(),
    };
    let style = if app.notice.is_some() {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new().dim()
    };
    frame.render_widget(Line::from(Span::styled(text, style)), area);
}
