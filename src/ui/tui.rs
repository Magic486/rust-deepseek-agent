use std::io;
use std::time::Duration;

use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::agent::{Agent, AgentEvent};
use crate::ui::state::{TranscriptItem, TuiState, UiStatus};

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub async fn run(mut agent: Agent) -> Result<()> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut state = TuiState::new(
        agent.todo_snapshot(),
        agent.skill_snapshot(),
        agent.mcp_snapshot(),
        agent.local_tool_count(),
    );

    let (command_tx, mut command_rx) = mpsc::channel::<String>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(512);
    let worker = tokio::spawn(async move {
        while let Some(input) = command_rx.recv().await {
            let sender = event_tx.clone();
            let result = agent
                .handle_user_input_stream(&input, |event| {
                    sender
                        .try_send(event.clone())
                        .map_err(|error| anyhow!("TUI 事件通道已满：{error}"))
                })
                .await;

            match result {
                Ok(result) => {
                    let _ = sender.send(agent.runtime_snapshot()).await;
                    if result.should_exit {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender
                        .send(AgentEvent::StatusChanged(crate::agent::AgentStatus::Error(
                            error.to_string(),
                        )))
                        .await;
                }
            }
        }
    });

    let mut running = true;
    while running {
        while let Ok(event) = event_rx.try_recv() {
            state.apply_event(&event);
            if matches!(
                event,
                AgentEvent::StatusChanged(crate::agent::AgentStatus::Ready)
            ) {
                state.scroll_to_bottom();
            }
        }

        terminal.draw(|frame| render(frame, &state))?;

        if event::poll(Duration::from_millis(50))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };

            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Esc => running = false,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    running = false
                }
                KeyCode::PageUp => state.scroll_up(10),
                KeyCode::PageDown => state.scroll_down(10),
                KeyCode::Up => state.scroll_up(1),
                KeyCode::Down => state.scroll_down(1),
                KeyCode::Home => state.scroll_to_top(),
                KeyCode::End => state.scroll_to_bottom(),
                KeyCode::Enter => {
                    let input = state.input.trim().to_string();
                    state.input.clear();
                    state.refresh_command_hint();
                    state.scroll_to_bottom();

                    if matches!(input.as_str(), "/clear" | "/new") {
                        state.reset_session(
                            state.todos.clone(),
                            state.skills.clone(),
                            state.mcp_servers.clone(),
                        );
                    }
                    if !input.is_empty() {
                        command_tx
                            .send(input)
                            .await
                            .map_err(|_| anyhow!("Agent 后台任务已结束"))?;
                    }
                }
                KeyCode::Backspace => {
                    state.input.pop();
                    state.refresh_command_hint();
                }
                KeyCode::Char(ch) => {
                    state.input.push(ch);
                    state.refresh_command_hint();
                }
                _ => {}
            }
        }
    }

    drop(command_tx);
    worker.abort();
    terminal.show_cursor()?;
    Ok(())
}

fn render(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    let has_sidebar = area.width >= 110;
    let columns = if has_sidebar {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(62), Constraint::Length(34)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)])
            .split(area)
    };

    let left = inset(columns[0], 2, 1);

    if state.has_conversation() {
        let left_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(7)])
            .split(left);

        render_workspace(frame, left_rows[0], state);
        render_input_panel(frame, left_rows[1], state);

        set_input_cursor(frame, input_text_area(left_rows[1]), state);
    } else {
        render_welcome(frame, left, state);
        set_input_cursor(frame, welcome_input_text_area(left), state);
    }

    if has_sidebar {
        render_sidebar(frame, inset(columns[1], 1, 1), state);
    }
}

fn status_text(status: &UiStatus) -> String {
    match status {
        UiStatus::Ready => "Ready".to_string(),
        UiStatus::Thinking => "Thinking".to_string(),
        UiStatus::RunningTool(name) => format!("Running tool: {name}"),
        UiStatus::Error(error) => format!("Error: {error}"),
    }
}

fn render_workspace(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    if state.has_conversation() {
        render_transcript(frame, area, state);
    } else {
        render_welcome(frame, area, state);
    }
}

fn render_welcome(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let card = welcome_card_area(area);
    let card_rows = welcome_card_rows(card);

    render_welcome_logo(frame, card_rows[0]);
    render_input_preview(frame, card_rows[2], state);
    render_welcome_shortcuts(frame, card_rows[3]);
    render_welcome_tip(frame, card_rows[5]);
}

fn render_welcome_logo(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let logo_lines = vec![
        Line::styled(
            " █████╗  ██████╗ ███████╗███╗   ██╗████████╗",
            Style::default().fg(Color::DarkGray),
        ),
        Line::styled(
            "██╔══██╗██╔════╝ ██╔════╝████╗  ██║╚══██╔══╝",
            Style::default().fg(Color::DarkGray),
        ),
        Line::styled(
            "███████║██║  ███╗█████╗  ██╔██╗ ██║   ██║",
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "██╔══██║██║   ██║██╔══╝  ██║╚██╗██║   ██║",
            Style::default().fg(Color::Gray),
        ),
        Line::styled(
            "██║  ██║╚██████╔╝███████╗██║ ╚████║   ██║",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═══╝   ╚═╝",
            Style::default().fg(Color::DarkGray),
        ),
        Line::from(vec![
            Span::styled("rust", Style::default().fg(Color::Cyan)),
            Span::styled("-deepseek", Style::default().fg(Color::Gray)),
            Span::styled("  ·  coding agent", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(logo_lines).alignment(Alignment::Center),
        area,
    );
}

fn render_welcome_shortcuts(frame: &mut ratatui::Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "tab",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" agents   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "ctrl+p",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" commands   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "/",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" commands", Style::default().fg(Color::DarkGray)),
        ]))
        .alignment(Alignment::Right),
        area,
    );
}

fn render_welcome_tip(frame: &mut ratatui::Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("● ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "tip ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Ask naturally. Agent will plan, call tools, and continue until it can answer.",
                Style::default().fg(Color::DarkGray),
            ),
        ]))
        .alignment(Alignment::Center),
        area,
    );
}

fn render_input_preview(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(10)])
        .split(area);
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Cyan)),
        columns[0],
    );

    let input = if state.input.is_empty() {
        "Ask anything... \"写一个 Rust 冒泡排序\""
    } else {
        state.input.as_str()
    };
    let style = if state.input.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let input_line = if state.input.is_empty() {
        Line::styled(format!("  {input}"), style.add_modifier(Modifier::BOLD))
    } else {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(input.to_string(), style.add_modifier(Modifier::BOLD)),
        ])
    };
    let meta_line = Line::from(vec![
        Span::styled(
            "  rust-deepseek-agent",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "DeepSeek",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(status_text(&state.status), status_style(&state.status)),
    ]);
    let lines = if area.height >= 5 {
        vec![
            Line::raw(""),
            input_line,
            Line::raw(""),
            meta_line,
            Line::raw(""),
        ]
    } else {
        vec![Line::raw(""), input_line, meta_line]
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().style(Style::default().bg(Color::Rgb(28, 28, 28))))
            .wrap(Wrap { trim: false }),
        columns[1],
    );
}

fn render_transcript(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let all_lines: Vec<Line> = state.transcript.iter().flat_map(transcript_lines).collect();
    let content = Paragraph::new(all_lines)
        .block(Block::default().style(Style::default().bg(Color::Black)))
        .wrap(Wrap { trim: false });
    let wrapped_height = content.line_count(area.width);
    let max_scroll = wrapped_height.saturating_sub(area.height as usize);
    let offset_from_bottom = state.scroll_offset.min(max_scroll);
    let scroll_from_top = max_scroll.saturating_sub(offset_from_bottom);
    let scroll_from_top = u16::try_from(scroll_from_top).unwrap_or(u16::MAX);

    frame.render_widget(content.scroll((scroll_from_top, 0)), area);

    if offset_from_bottom > 0 && area.height > 0 {
        let indicator = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("↑ 已向上滚动 {offset_from_bottom} 行，End 回到底部"),
                Style::default().fg(Color::DarkGray).bg(Color::Black),
            )),
            indicator,
        );
    }
}

fn render_input_panel(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Length(2)])
        .split(area);
    render_input_preview(frame, rows[0], state);

    let hint = if state.input.starts_with('/') {
        state.command_hint.as_str()
    } else if state.scroll_offset > 0 {
        "End 回到底部   PgUp/PgDown 滚动   Enter 发送"
    } else {
        "PgUp/PgDown 滚动   Enter 发送   Esc/Ctrl+C 退出   / 命令"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "tab",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" agents   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "ctrl+p",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" commands   ", Style::default().fg(Color::DarkGray)),
            Span::styled(hint.to_string(), Style::default().fg(Color::DarkGray)),
        ]))
        .alignment(Alignment::Right),
        rows[1],
    );
}

fn render_sidebar(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(18, 18, 18))),
        area,
    );
    let inner = inset(area, 2, 1);
    let (pending, done) = state.todo_counts();
    let mut lines = vec![
        Line::styled(
            format!("New session - {}", state.session_id),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            "Context",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            format!("{} tokens est", state.estimated_tokens()),
            Style::default().fg(Color::DarkGray),
        ),
        Line::styled(
            format!("{} messages", state.message_count()),
            Style::default().fg(Color::DarkGray),
        ),
        Line::styled(
            format!("{} tool calls", state.tool_call_count()),
            Style::default().fg(Color::DarkGray),
        ),
        Line::raw(""),
        Line::styled(
            "Tools",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(vec![
            Span::styled("• ", Style::default().fg(Color::Green)),
            Span::styled(
                "local",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} tools", state.local_tool_count),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::raw(""),
        Line::styled(
            "Todo",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            format!("{pending} pending · {done} done"),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    lines.push(Line::styled(
        "MCP",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    if state.mcp_servers.is_empty() {
        lines.push(Line::styled("未配置", Style::default().fg(Color::DarkGray)));
    } else {
        for server in state.mcp_servers.iter().take(6) {
            let (mark, color) = if server.connected {
                ("• ", Color::Green)
            } else {
                ("× ", Color::Red)
            };
            let suffix = if server.connected {
                format!(" {} tools", server.tool_count)
            } else {
                format!(" {}", server.error.as_deref().unwrap_or("未连接"))
            };
            lines.push(Line::from(vec![
                Span::styled(mark, Style::default().fg(color)),
                Span::styled(server.name.clone(), Style::default().fg(Color::White)),
                Span::styled(suffix, Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Skills",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    if state.skills.is_empty() {
        lines.push(Line::styled("未发现", Style::default().fg(Color::DarkGray)));
    } else {
        for skill in state.skills.iter().filter(|skill| skill.loaded).take(5) {
            lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Yellow)),
                Span::styled(skill.name.clone(), Style::default().fg(Color::White)),
            ]));
        }
        if !state.skills.iter().any(|skill| skill.loaded) {
            lines.push(Line::styled(
                "按需加载",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    let mut all_lines = lines;
    all_lines.extend(todo_sidebar_lines(state));
    all_lines.push(Line::raw(""));
    all_lines.push(Line::styled(
        "Shortcuts",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    all_lines.push(Line::styled(
        "Enter send",
        Style::default().fg(Color::DarkGray),
    ));
    all_lines.push(Line::styled(
        "Esc / Ctrl+C quit",
        Style::default().fg(Color::DarkGray),
    ));
    all_lines.push(Line::styled(
        "/ commands",
        Style::default().fg(Color::DarkGray),
    ));

    frame.render_widget(
        Paragraph::new(all_lines)
            .block(Block::default().style(Style::default().bg(Color::Rgb(18, 18, 18))))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn todo_sidebar_lines(state: &TuiState) -> Vec<Line<'static>> {
    if state.todos.is_empty() {
        return vec![Line::styled(
            "暂无待办",
            Style::default().fg(Color::DarkGray),
        )];
    }

    state
        .todos
        .iter()
        .take(8)
        .map(|todo| {
            let mark = if todo.status == "Done" { "✓" } else { "□" };
            let style = if todo.status == "Done" {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(vec![
                Span::styled(format!("{mark} "), style),
                Span::styled(
                    format!("#{} ", todo.id),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(todo.title.clone(), style),
            ])
        })
        .collect()
}

fn inset(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(horizontal),
        y: area.y.saturating_add(vertical),
        width: area.width.saturating_sub(horizontal.saturating_mul(2)),
        height: area.height.saturating_sub(vertical.saturating_mul(2)),
    }
}

fn welcome_card_area(area: Rect) -> Rect {
    let desired_width = area.width.saturating_sub(4).clamp(42, 94);
    let desired_height = area.height.saturating_sub(2).clamp(10, 19);
    center_rect(area, desired_width, desired_height)
}

fn welcome_card_rows(card: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(card)
}

fn center_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;

    Rect {
        x,
        y,
        width,
        height,
    }
}

fn input_preview_rows(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(area)
}

fn input_text_area(input_panel_area: Rect) -> Rect {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Length(2)])
        .split(input_panel_area);
    input_preview_text_area(rows[0])
}

fn welcome_input_text_area(area: Rect) -> Rect {
    let card = welcome_card_area(area);
    let rows = welcome_card_rows(card);
    input_preview_text_area(rows[2])
}

fn input_preview_text_area(area: Rect) -> Rect {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(10)])
        .split(area);
    let rows = input_preview_rows(columns[1]);
    rows[0]
}

fn set_input_cursor(frame: &mut ratatui::Frame<'_>, text_area: Rect, state: &TuiState) {
    let cursor_prefix = 2;
    let cursor_x = text_area.x + cursor_prefix + display_width(&state.input);
    let cursor_y = text_area.y + 1;
    frame.set_cursor_position((cursor_x.min(text_area.right().saturating_sub(1)), cursor_y));
}

fn display_width(text: &str) -> u16 {
    text.chars()
        .map(|character| {
            if character.is_control() {
                0
            } else if character.is_ascii() {
                1
            } else {
                2
            }
        })
        .sum()
}

fn transcript_lines(item: &TranscriptItem) -> Vec<Line<'static>> {
    match item {
        TranscriptItem::User(content) => {
            message_block("你", Color::Green, content, Color::White, 8)
        }
        TranscriptItem::Assistant(content) => {
            markdown_message_block("Agent", Color::Cyan, content, 180)
        }
        TranscriptItem::Thinking(content) => {
            message_block("思考", Color::LightMagenta, content, Color::DarkGray, 4)
        }
        TranscriptItem::ToolCall { name, input } => tool_call_block(name, input),
        TranscriptItem::ToolResult { name, output } => tool_result_block(name, output),
        TranscriptItem::System(content) => {
            message_block("系统", Color::Gray, content, Color::Gray, 40)
        }
        TranscriptItem::Error(content) => {
            message_block("错误", Color::Red, content, Color::Red, 10)
        }
    }
}

fn message_block(
    label: &str,
    label_color: Color,
    content: &str,
    content_color: Color,
    max_body_lines: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("▌ ", Style::default().fg(label_color)),
            Span::styled(
                label.to_string(),
                Style::default()
                    .fg(label_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    lines.extend(body_lines(content, content_color, max_body_lines));
    lines
}

fn markdown_message_block(
    label: &str,
    label_color: Color,
    content: &str,
    max_body_lines: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("▌ ", Style::default().fg(label_color)),
            Span::styled(
                label.to_string(),
                Style::default()
                    .fg(label_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    lines.extend(markdown_body_lines(content, max_body_lines));
    lines
}

fn markdown_body_lines(text: &str, max_lines: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();

        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;

            if in_code_block {
                code_lang = trimmed.trim_start_matches("```").trim().to_string();
                let title = if code_lang.is_empty() {
                    "code".to_string()
                } else {
                    format!("code: {code_lang}")
                };
                lines.push(Line::styled(
                    format!("  ┌─ {title} ─────────────────────────"),
                    Style::default().fg(Color::DarkGray),
                ));
            } else {
                lines.push(Line::styled(
                    "  └────────────────────────────────────",
                    Style::default().fg(Color::DarkGray),
                ));
                code_lang.clear();
            }

            continue;
        }

        if in_code_block {
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(raw_line.to_string(), Style::default().fg(Color::Gray)),
            ]));
            continue;
        }

        if trimmed.is_empty() {
            lines.push(Line::raw(""));
        } else if let Some(heading) = parse_heading(trimmed) {
            lines.push(Line::styled(
                format!("  {}", heading),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if is_rule(trimmed) {
            lines.push(Line::styled(
                "  ────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            ));
        } else if let Some(item) = parse_unordered_item(trimmed) {
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Yellow)),
                markdown_inline_spans(item, Color::White),
            ]));
        } else if let Some((number, item)) = parse_ordered_item(trimmed) {
            lines.push(Line::from(vec![
                Span::styled(format!("  {number}. "), Style::default().fg(Color::Yellow)),
                markdown_inline_spans(item, Color::White),
            ]));
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                markdown_inline_spans(quote, Color::Gray),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                markdown_inline_spans(trimmed, Color::White),
            ]));
        }
    }

    if in_code_block {
        lines.push(Line::styled(
            "  └────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ));
    }

    if lines.is_empty() {
        lines.push(Line::styled("  (空)", Style::default().fg(Color::DarkGray)));
    }

    truncate_lines(lines, max_lines)
}

fn markdown_inline_spans(text: &str, default_color: Color) -> Span<'static> {
    let mut output = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '`' {
            let mut code = String::new();
            for next in chars.by_ref() {
                if next == '`' {
                    break;
                }
                code.push(next);
            }
            output.push('`');
            output.push_str(&code);
            output.push('`');
        } else if ch == '*' && chars.peek() == Some(&'*') {
            chars.next();
            let mut bold = String::new();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    break;
                }
                bold.push(next);
            }
            output.push_str(&bold);
        } else {
            output.push(ch);
        }
    }

    Span::styled(output, Style::default().fg(default_color))
}

fn parse_heading(line: &str) -> Option<&str> {
    let level = line.chars().take_while(|ch| *ch == '#').count();

    if (1..=6).contains(&level) && line.chars().nth(level) == Some(' ') {
        Some(line[level + 1..].trim())
    } else {
        None
    }
}

fn is_rule(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();

    chars.len() >= 3
        && chars
            .iter()
            .all(|ch| *ch == '-' || *ch == '*' || *ch == '_')
}

fn parse_unordered_item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
}

fn parse_ordered_item(line: &str) -> Option<(&str, &str)> {
    let dot = line.find(". ")?;
    let number = &line[..dot];

    if number.chars().all(|ch| ch.is_ascii_digit()) {
        Some((number, line[dot + 2..].trim()))
    } else {
        None
    }
}

fn truncate_lines(mut lines: Vec<Line<'static>>, max_lines: usize) -> Vec<Line<'static>> {
    if lines.len() <= max_lines {
        return lines;
    }

    let omitted = lines.len() - max_lines;
    lines.truncate(max_lines);
    lines.push(Line::styled(
        format!("  ... 已省略 {omitted} 行"),
        Style::default().fg(Color::DarkGray),
    ));
    lines
}

fn tool_call_block(name: &str, input: &str) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("◇ ", Style::default().fg(Color::LightBlue)),
            Span::styled(
                "工具",
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::styled(
            "  ┌─ tool ─────────────────────────────",
            Style::default().fg(Color::DarkGray),
        ),
        Line::from(vec![
            Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("调用 ", Style::default().fg(Color::Yellow)),
            Span::styled(name.to_string(), Style::default().fg(Color::White)),
        ]),
    ];

    if !input.trim().is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("> ", Style::default().fg(Color::Yellow)),
            Span::raw(input.to_string()),
        ]));
    }

    lines.push(Line::styled(
        "  └────────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    ));
    lines
}

fn tool_result_block(name: &str, output: &str) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("◇ ", Style::default().fg(Color::LightBlue)),
            Span::styled(
                "工具",
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::styled(
            "  ┌─ tool result ──────────────────────",
            Style::default().fg(Color::DarkGray),
        ),
        Line::from(vec![
            Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("完成 ", Style::default().fg(Color::Green)),
            Span::styled(name.to_string(), Style::default().fg(Color::White)),
        ]),
    ];

    lines.extend(card_body_lines(output, 120));
    lines.push(Line::styled(
        "  └────────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    ));
    lines
}

fn card_body_lines(text: &str, max_lines: usize) -> Vec<Line<'static>> {
    let raw_lines: Vec<&str> = text.lines().collect();

    if raw_lines.is_empty() {
        return vec![Line::styled(
            "  │ (空)",
            Style::default().fg(Color::DarkGray),
        )];
    }

    let shown = raw_lines.len().min(max_lines);
    let mut lines: Vec<Line> = raw_lines
        .iter()
        .take(shown)
        .map(|line| {
            Line::from(vec![
                Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::Gray)),
            ])
        })
        .collect();

    if raw_lines.len() > shown {
        lines.push(Line::from(vec![
            Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("... 已省略 {} 行", raw_lines.len() - shown),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    lines
}

fn body_lines(text: &str, color: Color, max_lines: usize) -> Vec<Line<'static>> {
    let raw_lines: Vec<&str> = text.lines().collect();

    if raw_lines.is_empty() {
        return vec![Line::styled("  (空)", Style::default().fg(Color::DarkGray))];
    }

    let shown = raw_lines.len().min(max_lines);
    let mut lines: Vec<Line> = raw_lines
        .iter()
        .take(shown)
        .map(|line| Line::styled(format!("  {line}"), Style::default().fg(color)))
        .collect();

    if raw_lines.len() > shown {
        lines.push(Line::styled(
            format!("  ... 已省略 {} 行", raw_lines.len() - shown),
            Style::default().fg(Color::DarkGray),
        ));
    }

    lines
}

fn status_style(status: &UiStatus) -> Style {
    match status {
        UiStatus::Ready => Style::default().fg(Color::Green),
        UiStatus::Thinking => Style::default().fg(Color::Yellow),
        UiStatus::RunningTool(_) => Style::default().fg(Color::Magenta),
        UiStatus::Error(_) => Style::default().fg(Color::Red),
    }
}
