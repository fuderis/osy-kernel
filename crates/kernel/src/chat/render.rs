use super::AppState;
use crate::prelude::*;
use anylm::api::{Content, Messages};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

/// Renders the terminal interface
pub fn render_tui(f: &mut Frame, app: &mut AppState) {
    let area = f.area();
    let msgs = app.messages.dirty_get();

    let prefix = " ❯ ";
    let inner_width = area.width.saturating_sub(2);

    let full_input_text = format!("{}{}", prefix, app.input);
    let input_lines_count = total_wrapped_lines(&full_input_text, inner_width);
    let dynamic_input_height = (input_lines_count + 2).clamp(3, 8);

    let footer_height = calculate_footer_height(app, inner_width);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                    // Хедер
            Constraint::Min(1),                       // Чат
            Constraint::Length(dynamic_input_height), // Инпут
            Constraint::Length(footer_height),        // Футер
        ])
        .split(area);

    app.chat_area = chunks[1];
    app.input_area = chunks[2];

    render_header(f, chunks[0]);
    render_chat(f, chunks[1], app, &msgs);
    render_input(f, chunks[2], app, prefix);
    render_footer(f, chunks[3], app);

    drop(msgs);
}

fn render_header(f: &mut Frame, area: Rect) {
    let current_path = std::env::current_dir()
        .map(|p| {
            let path_str = p.display().to_string();
            let home_str = format!("{}/", env!("HOME"));
            if path_str.starts_with(&home_str) {
                return path_str.replacen(&home_str, "~/", 1);
            }
            path_str
        })
        .unwrap_or_else(|_| "Unknown".to_string());

    let header_line = Line::from(vec![
        Span::raw(" "),
        Span::styled(current_path, Style::default().fg(Color::DarkGray).italic()),
    ]);

    f.render_widget(Paragraph::new(header_line), area);
}

fn render_chat(f: &mut Frame, area: Rect, app: &mut AppState, msgs: &Arc<Messages>) {
    let mut history: Vec<Line> = Vec::new();

    for (i, msg) in msgs.messages.iter().enumerate() {
        let text_content = msg
            .content
            .iter()
            .filter_map(|p| match p {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>()
            .trim()
            .to_string();

        if text_content.is_empty() {
            continue;
        }

        if msg.role.is_user() {
            history.push(Line::raw(""));

            if i != 0 {
                let line_width = area.width.saturating_sub(2) as usize;
                let hr_string = "─".repeat(line_width);
                let hr_line = Line::from(vec![Span::styled(
                    hr_string,
                    Style::default().fg(Color::DarkGray),
                )]);

                history.push(hr_line);
            }

            let time_str = msg
                .timestamp
                .unwrap_or(Utc::now())
                .with_timezone(&Local)
                .format("%A, %I:%M %p (%:z)")
                .to_string();

            let meta_line = Line::from(vec![
                Span::styled("● ", Style::default().fg(Color::Cyan)),
                Span::styled(time_str, Style::default().fg(Color::Gray).bold()),
            ]);

            history.push(meta_line);
        } else if !history.is_empty() {
            history.push(Line::raw(""));
        }

        let msg_lines = super::parse_markdown(&text_content, area.width.saturating_sub(2) as usize);

        if msg.role.is_user() || msg.role.is_tool() {
            for line in msg_lines {
                history.push(line.style(Style::default().dim()));
            }
        } else {
            history.extend(msg_lines);
        }
    }

    history.push(Line::raw(""));

    let total_lines = history.len() as u16;
    let max_chat_scroll = total_lines.saturating_sub(area.height);
    app.chat_scroll = app.chat_scroll.min(max_chat_scroll);

    f.render_widget(
        Paragraph::new(history)
            .scroll((app.chat_scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_input(f: &mut Frame, area: Rect, app: &mut AppState, prefix: &str) {
    let inner_width = area.width.saturating_sub(2);
    let inner_height = area.height.saturating_sub(2);

    let text_before_cursor = format!("{}{}", prefix, &app.input[..app.input_cursor]);
    let cursor_lines = total_wrapped_lines(&text_before_cursor, inner_width);
    let cursor_row = cursor_lines.saturating_sub(1);

    if cursor_row >= app.input_scroll + inner_height {
        app.input_scroll = cursor_row - inner_height + 1;
    }
    if cursor_row < app.input_scroll {
        app.input_scroll = cursor_row;
    }

    let (input_color, title_style) = if app.is_busy {
        (Color::White, Style::default().fg(Color::White).bold())
    } else {
        (Color::Cyan, Style::default().fg(Color::Cyan).bold())
    };
    let input_style = Style::default().fg(input_color);

    let ops = &Settings::get().completions.options;
    let max_tokens = ops.max_tokens.unwrap_or(1) as usize;
    let tokens_count = &app.messages.dirty_get().tokens_count;

    let title_left = Line::from(vec![
        Span::raw(" "),
        Span::styled(ops.model.clone(), title_style),
        Span::raw(" "),
        Span::styled(format!("({}/{})", tokens_count, max_tokens), input_style),
        Span::raw(" "),
    ]);

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title_left)
        .border_style(input_style);

    if app.is_busy {
        let dots_count = (app.tick_count / 25) % 4;
        let thinking_text = format!(" thinking{:<3} ", ".".repeat(dots_count as usize));
        block = block.title_bottom(Span::styled(
            thinking_text,
            Style::default().fg(Color::White),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, input_style),
            Span::styled(app.input.as_str(), Style::default().fg(Color::White)),
        ]))
        .wrap(Wrap { trim: false })
        .scroll((app.input_scroll, 0))
        .block(block),
        area,
    );

    let last_line_width = text_before_cursor
        .lines()
        .last()
        .map(|l| l.chars().count() as u16)
        .unwrap_or(0);

    let cursor_x = area.x + 1 + (last_line_width % inner_width);

    f.set_cursor_position((cursor_x, area.y + 1 + (cursor_row - app.input_scroll)));
}

fn calculate_footer_height(app: &AppState, max_width: u16) -> u16 {
    let text_len = if let Some(status) = app.status.as_ref().filter(|s| !s.is_empty()) {
        status.chars().count()
    } else if !app.commands.is_empty() {
        let index = (app.tick_count / 360) as usize % app.commands.len();
        let (cmd, desc) = app.commands[index];
        cmd.len() + desc.len() + 3
    } else {
        return 1;
    };

    if text_len > max_width as usize { 2 } else { 1 }
}

fn render_footer(f: &mut Frame, area: Rect, app: &AppState) {
    let mut footer_text = Text::default();

    if let Some(status) = app.status.as_ref().filter(|s| !s.is_empty()) {
        let parsed_lines = super::parse_markdown(status, area.width as usize);
        for mut line in parsed_lines {
            line.spans.insert(0, Span::raw("  "));
            footer_text
                .lines
                .push(line.patch_style(Style::default().italic().gray()));
        }
    } else if !app.commands.is_empty() {
        let index = (app.tick_count / 360) as usize % app.commands.len();
        let (cmd, desc) = app.commands[index];
        footer_text.lines.push(Line::from(vec![
            "  ".into(),
            cmd.bold().cyan(),
            " ".into(),
            desc.gray().italic(),
        ]));
    }

    f.render_widget(Paragraph::new(footer_text).wrap(Wrap { trim: false }), area);
}

fn total_wrapped_lines(text: &str, max_width: u16) -> u16 {
    if text.is_empty() || max_width == 0 {
        return 1;
    }
    let mut lines = 0;
    for line in text.lines() {
        let chars = line.chars().count() as u16;
        lines += if chars == 0 {
            1
        } else {
            (chars + max_width - 1) / max_width
        };
    }
    lines.max(1)
}
