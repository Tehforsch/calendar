use jiff::{ToSpan, civil::Date};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction as LayoutDirection, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    app::{AgendaItem, AgendaState, App, EditField, EditMode, EditState, Mode, SearchState},
    config::ViewMode,
    hotkey::{AgendaAction, ConfirmAction, NormalAction, SearchAction},
    store::{CalendarEvent, EventOccurrence},
};

const PALETTE: [Color; 8] = [
    Color::Rgb(86, 156, 214),
    Color::Rgb(255, 183, 77),
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::LightRed,
    Color::LightCyan,
    Color::LightMagenta,
];

pub fn draw(frame: &mut Frame, app: &App) {
    let has_status = app.status.is_some();
    let areas = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(8),
        Constraint::Length(if has_status { 2 } else { 1 }),
    ])
    .split(frame.area());

    let search = match &app.mode {
        Mode::Search(search) => Some(search),
        _ => None,
    };
    let agenda = match &app.mode {
        Mode::Agenda(agenda) => Some(agenda),
        Mode::ConfirmDelete(agenda) => Some(agenda.as_ref()),
        _ => None,
    };
    if let Some(search) = search {
        draw_search_header(frame, search, areas[0]);
        draw_search(frame, app, search, areas[1]);
        draw_search_footer(frame, app, areas[2]);
    } else if let Some(agenda) = agenda {
        draw_agenda_header(frame, app, agenda, areas[0]);
        draw_agenda(frame, app, agenda, areas[1]);
        draw_agenda_footer(frame, app, areas[2]);
    } else {
        draw_header(frame, app, areas[0]);
        match app.view {
            ViewMode::Month => draw_month(frame, app, areas[1]),
            ViewMode::Week => draw_week(frame, app, areas[1]),
        }
        draw_footer(frame, app, areas[2]);
    }

    match &app.mode {
        Mode::Edit(editor) => draw_editor(frame, app, editor),
        Mode::Help => draw_help(frame, app),
        Mode::ConfirmDelete(agenda) => draw_delete_confirmation(frame, app, agenda),
        Mode::Normal | Mode::Agenda(_) | Mode::Search(_) => {}
    }
    draw_pending(frame, app);
}

fn draw_agenda_header(frame: &mut Frame, app: &App, agenda: &AgendaState, area: Rect) {
    let start = agenda.center_date - 14.days();
    let end = agenda.center_date + 14.days();
    let calendars = app
        .events
        .iter()
        .map(|event| event.calendar.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut spans = vec![
        Span::styled(
            " agenda ",
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  {} – {}  ·  centered on {}  ",
            format_date(start),
            format_date(end),
            agenda.center_date.strftime("%A %-d %B")
        )),
    ];
    for (index, calendar) in calendars.into_iter().enumerate() {
        spans.push(Span::styled(
            format!("■ {calendar}  "),
            Style::default().fg(calendar_color(app, calendar, index)),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_agenda(frame: &mut Frame, app: &App, agenda: &AgendaState, area: Rect) {
    let columns =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area);
    draw_agenda_list(
        frame,
        app,
        &agenda.items,
        agenda.selected,
        Some(agenda.center_date),
        format!(
            "No appointments within 14 days of {}",
            format_date(agenda.center_date)
        ),
        columns[0],
    );
    draw_event_details(frame, app, agenda, columns[1]);
}

fn draw_event_details(frame: &mut Frame, app: &App, agenda: &AgendaState, area: Rect) {
    let Some(item) = agenda.selected_item() else {
        frame.render_widget(
            Paragraph::new("Select an appointment to see its details")
                .block(Block::default().borders(Borders::ALL).title(" details ")),
            area,
        );
        return;
    };
    let event = &app.events[item.event_index];
    let timezone = app.timezone().clone();
    let start = item.start.timestamp().to_zoned(timezone.clone());
    let end = item.end.timestamp().to_zoned(timezone);
    let timing = if event.all_day {
        format!("{} · all day", start.strftime("%A, %-d %B %Y"))
    } else {
        format!(
            "{} {} – {}",
            start.strftime("%a %-d %b %Y"),
            start.strftime("%H:%M"),
            end.strftime("%a %-d %b %Y %H:%M")
        )
    };
    let mut lines = vec![
        Line::styled(
            event.summary.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(timing),
        Line::raw(format!("Calendar: {}", event.calendar)),
        Line::raw(""),
    ];
    let mut links = Vec::new();
    for metadata in &event.metadata {
        if matches!(
            metadata.name.as_str(),
            "UID" | "DTSTAMP" | "CREATED" | "LAST-MODIFIED" | "DTSTART" | "DTEND" | "SUMMARY"
        ) {
            continue;
        }
        let mut label = metadata.name.replace('-', " ");
        for (name, value) in &metadata.parameters {
            label.push_str(&format!(";{name}={value}"));
        }
        let mut values = metadata.value.lines();
        if let Some(value) = values.next() {
            let row = lines.len() as u16;
            let (line, line_links) = metadata_line(&label, value);
            links.extend(
                line_links
                    .into_iter()
                    .map(|(column, url)| (row, column, url)),
            );
            lines.push(line);
        }
        for value in values {
            let row = lines.len() as u16;
            let (line, line_links) = metadata_line("", value);
            links.extend(
                line_links
                    .into_iter()
                    .map(|(column, url)| (row, column, url)),
            );
            lines.push(line);
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" appointment details "),
        ),
        area,
    );
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    for (row, column, url) in links {
        if row >= inner.height || column >= inner.width {
            continue;
        }
        for (offset, chunk) in url.as_bytes().chunks(2).enumerate() {
            let x = inner.x + column + (offset as u16 * 2);
            if x >= inner.right() {
                break;
            }
            let visible = std::str::from_utf8(chunk).unwrap_or("");
            let symbol = format!("\x1b]8;;{url}\x07{visible}\x1b]8;;\x07");
            frame.buffer_mut()[(x, inner.y + row)].set_symbol(&symbol);
        }
    }
}

fn metadata_line<'a>(label: &str, value: &'a str) -> (Line<'a>, Vec<(u16, String)>) {
    let mut spans = Vec::new();
    let mut column;
    if !label.is_empty() {
        spans.push(Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ));
        column = label.len() as u16 + 2;
    } else {
        spans.push(Span::raw("  "));
        column = 2;
    }
    let mut links = Vec::new();
    for (text, link) in link_parts(value) {
        let style = if link {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
        };
        if link {
            links.push((column, text.to_string()));
        }
        spans.push(Span::styled(text, style));
        column = column.saturating_add(text.len() as u16);
    }
    (Line::from(spans), links)
}

fn link_parts(value: &str) -> Vec<(&str, bool)> {
    let mut parts = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find("https://").or_else(|| rest.find("http://")) {
        if start > 0 {
            parts.push((&rest[..start], false));
        }
        let tail = &rest[start..];
        let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        let (url, remaining) = tail.split_at(end);
        parts.push((url.trim_end_matches([',', '.', ')', ']', ';']), true));
        let trimmed = url.trim_end_matches([',', '.', ')', ']', ';']);
        if trimmed.len() < url.len() {
            parts.push((&url[trimmed.len()..], false));
        }
        rest = remaining;
    }
    if !rest.is_empty() {
        parts.push((rest, false));
    }
    parts
}

fn draw_agenda_list(
    frame: &mut Frame,
    app: &App,
    items: &[AgendaItem],
    selected_index: usize,
    center_date: Option<Date>,
    empty_message: String,
    area: Rect,
) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let timezone = app.timezone().clone();
    let mut lines = Vec::new();
    let mut selected_line = 0;
    let mut previous_date = None;
    for (index, item) in items.iter().enumerate() {
        let event = &app.events[item.event_index];
        let start = item.start.timestamp().to_zoned(timezone.clone());
        let end = item.end.timestamp().to_zoned(timezone.clone());
        let date = start.date();
        if previous_date != Some(date) {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            let center = if center_date == Some(date) {
                "  · selected day"
            } else {
                ""
            };
            lines.push(Line::styled(
                format!("{}{}", date.strftime("%A · %-d %B %Y"), center),
                Style::default()
                    .fg(if center_date == Some(date) {
                        Color::Cyan
                    } else {
                        Color::Gray
                    })
                    .add_modifier(Modifier::BOLD),
            ));
            previous_date = Some(date);
        }
        let time = if event.all_day {
            "all day".to_string()
        } else if end.date() == date {
            format!("{}–{}", start.strftime("%H:%M"), end.strftime("%H:%M"))
        } else {
            format!(
                "{}–{} {}",
                start.strftime("%H:%M"),
                end.strftime("%-d %b"),
                end.strftime("%H:%M")
            )
        };
        let selected = index == selected_index;
        if selected {
            selected_line = lines.len();
        }
        let style = if selected {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color_for_event(app, event, index))
        };
        lines.push(Line::styled(
            format!(
                " {}  {:<17}  {}  [{}]",
                if selected { "▶" } else { " " },
                time,
                event.summary,
                event.calendar
            ),
            style,
        ));
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            empty_message,
            Style::default().fg(Color::DarkGray),
        ));
    }
    let max_start = lines.len().saturating_sub(inner_height);
    let scroll = selected_line
        .saturating_sub(inner_height / 2)
        .min(max_start);
    let visible = lines
        .into_iter()
        .skip(scroll)
        .take(inner_height)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(visible).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} appointment(s) ", items.len()))
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

fn draw_search_header(frame: &mut Frame, search: &SearchState, area: Rect) {
    let prefix = " search  /";
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " search ",
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                " /{}  ·  {} match(es)",
                search.query.value(),
                search.items.len()
            )),
        ])),
        area,
    );
    let cursor = search.query.visual_cursor() as u16;
    frame.set_cursor_position((
        area.x + (prefix.len() as u16 + cursor).min(area.width.saturating_sub(1)),
        area.y,
    ));
}

fn draw_search(frame: &mut Frame, app: &App, search: &SearchState, area: Rect) {
    draw_agenda_list(
        frame,
        app,
        &search.items,
        search.selected,
        None,
        "No appointments match this search".to_string(),
        area,
    );
}

fn draw_search_footer(frame: &mut Frame, app: &App, area: Rect) {
    let key = |action| {
        app.config
            .hotkeys
            .search
            .key_for(&action)
            .unwrap_or_else(|| "—".to_string())
    };
    let line = Line::from(vec![
        Span::styled(
            format!(
                " {}/{} ",
                key(SearchAction::Navigate(crate::hotkey::Direction::Down)),
                key(SearchAction::Navigate(crate::hotkey::Direction::Up))
            ),
            key_style(),
        ),
        Span::raw(" matches  "),
        Span::styled(format!(" {} ", key(SearchAction::Select)), key_style()),
        Span::raw(" focus  "),
        Span::styled(format!(" {} ", key(SearchAction::Cancel)), key_style()),
        Span::raw(" back"),
    ]);
    frame.render_widget(Paragraph::new(line), Rect { height: 1, ..area });
}

fn draw_agenda_footer(frame: &mut Frame, app: &App, area: Rect) {
    let key = |action| {
        app.config
            .hotkeys
            .agenda
            .bindings
            .iter()
            .filter(|(candidate, _)| candidate == &action)
            .map(|(_, sequence)| sequence.display())
            .min_by_key(String::len)
            .unwrap_or_else(|| "—".to_string())
    };
    let line = Line::from(vec![
        Span::styled(
            format!(
                " {}/{} ",
                key(AgendaAction::Navigate(crate::hotkey::Direction::Down)),
                key(AgendaAction::Navigate(crate::hotkey::Direction::Up))
            ),
            key_style(),
        ),
        Span::raw(" appointments  "),
        Span::styled(format!(" {} ", key(AgendaAction::AddEvent)), key_style()),
        Span::raw(" new  "),
        Span::styled(format!(" {} ", key(AgendaAction::Edit)), key_style()),
        Span::raw(" edit  "),
        Span::styled(format!(" {} ", key(AgendaAction::Delete)), key_style()),
        Span::raw(" delete  "),
        Span::styled(format!(" {} ", key(AgendaAction::MonthView)), key_style()),
        Span::raw(" views  "),
        Span::styled(format!(" {} ", key(AgendaAction::Close)), key_style()),
        Span::raw(" back"),
    ]);
    frame.render_widget(Paragraph::new(line), Rect { height: 1, ..area });
    if let Some(status) = &app.status
        && area.height > 1
    {
        frame.render_widget(
            Paragraph::new(status.as_str()).style(Style::default().fg(Color::Yellow)),
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
        );
    }
}

fn draw_delete_confirmation(frame: &mut Frame, app: &App, agenda: &AgendaState) {
    let Some(item) = agenda.selected_item() else {
        return;
    };
    let event = &app.events[item.event_index];
    let area = centered(frame.area(), 64, 7);
    frame.render_widget(Clear, area);
    let confirm = app
        .config
        .hotkeys
        .confirm
        .key_for(&ConfirmAction::Confirm)
        .unwrap_or_else(|| "Enter".to_string());
    let cancel = app
        .config
        .hotkeys
        .confirm
        .key_for(&ConfirmAction::Cancel)
        .unwrap_or_else(|| "Esc".to_string());
    let detail = if event.is_recurring() {
        "Only this occurrence will be deleted."
    } else {
        "This appointment will be deleted."
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                event.summary.as_str(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Line::raw(detail),
            Line::raw(""),
            Line::styled(
                format!("{confirm} confirm · {cancel} back"),
                Style::default().fg(Color::DarkGray),
            ),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Delete appointment? ")
                .border_style(Style::default().fg(Color::LightRed)),
        )
        .style(Style::default().bg(Color::Black)),
        area,
    );
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let start = app.view_start();
    let end = match app.view {
        ViewMode::Month => start + 27.days(),
        ViewMode::Week => start + 6.days(),
    };
    let view_name = match app.view {
        ViewMode::Month => "FOUR WEEKS",
        ViewMode::Week => "WEEK",
    };
    let mut spans = vec![
        Span::styled(
            " calendar ",
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  {view_name}  {} – {}  ",
            format_date(start),
            format_date(end)
        )),
    ];
    let mut calendars = app
        .events
        .iter()
        .map(|event| event.calendar.as_str())
        .collect::<Vec<_>>();
    calendars.sort_unstable();
    calendars.dedup();
    for (index, calendar) in calendars.into_iter().enumerate() {
        spans.push(Span::styled(
            format!("■ {calendar}  "),
            Style::default().fg(calendar_color(app, calendar, index)),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_month(frame: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ])
    .split(area);
    let header_cells = Layout::horizontal([Constraint::Ratio(1, 7); 7]).split(rows[0]);
    for (index, weekday) in ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"]
        .iter()
        .enumerate()
    {
        frame.render_widget(
            Paragraph::new(*weekday).alignment(Alignment::Center).style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            header_cells[index],
        );
    }

    let start = app.view_start();
    for week in 0..4 {
        let cells = Layout::horizontal([Constraint::Ratio(1, 7); 7]).split(rows[week + 1]);
        for day in 0..7 {
            let date = start + ((week * 7 + day) as i64).days();
            draw_month_day(frame, app, date, cells[day]);
        }
    }
}

fn draw_month_day(frame: &mut Frame, app: &App, date: Date, area: Rect) {
    let today = date == app.now.date();
    let selected = date == app.selected;
    let marker = match (today, selected) {
        (true, true) => "●▶",
        (true, false) => "● ",
        (false, true) => " ▶",
        (false, false) => "  ",
    };
    let mut block_style = Style::default();
    if today {
        block_style = block_style.bg(Color::Rgb(30, 40, 48)).fg(Color::White);
    }
    let border_style = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if today {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title = format!("{marker} {}", date.strftime("%-d %b"));
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(block_style)
        .border_style(border_style);
    let inner_height = area.height.saturating_sub(2) as usize;
    let events = app.events_on(date);
    let mut lines = events
        .iter()
        .take(inner_height)
        .enumerate()
        .map(|(index, event)| month_event_line(app, event, date, index))
        .collect::<Vec<_>>();
    if events.len() > inner_height && inner_height > 0 {
        lines.truncate(inner_height.saturating_sub(1));
        lines.push(Line::styled(
            format!("  +{} more", events.len() - lines.len()),
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn month_event_line<'a>(
    app: &App,
    event: &'a EventOccurrence<'a>,
    date: Date,
    palette_index: usize,
) -> Line<'a> {
    let start = event.start.timestamp().to_zoned(app.timezone().clone());
    let label = if event.all_day {
        format!("█ {}", event.summary)
    } else if start.date() == date {
        format!("▌{} {}", start.strftime("%H:%M"), event.summary)
    } else {
        format!("▌↳ {}", event.summary)
    };
    Line::styled(
        label,
        Style::default().fg(color_for_event(app, event.event, palette_index)),
    )
}

fn draw_week(frame: &mut Frame, app: &App, area: Rect) {
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(6),
    ])
    .split(area);
    let start = app.view_start();
    let header_columns = week_columns(sections[0]);
    frame.render_widget(
        Paragraph::new("TIME").style(Style::default().fg(Color::DarkGray)),
        header_columns[0],
    );
    for day in 0..7 {
        let date = start + day.days();
        let today = date == app.now.date();
        let selected = date == app.selected;
        let marker = match (today, selected) {
            (true, true) => "●▶",
            (true, false) => "● ",
            (false, true) => " ▶",
            _ => "  ",
        };
        let style = if today {
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        frame.render_widget(
            Paragraph::new(format!("{marker} {}", date.strftime("%a %-d")))
                .alignment(Alignment::Center)
                .style(style),
            header_columns[day as usize + 1],
        );
    }

    let all_day_columns = week_columns(sections[1]);
    frame.render_widget(
        Paragraph::new("ALL DAY").style(Style::default().fg(Color::DarkGray)),
        all_day_columns[0],
    );
    for day in 0..7 {
        let date = start + day.days();
        let events = app
            .events_on(date)
            .into_iter()
            .filter(|event| event.all_day)
            .collect::<Vec<_>>();
        let line = if let Some(event) = events.first() {
            Line::styled(
                format!("█ {}", event.summary),
                Style::default().fg(color_for_event(app, event.event, 0)),
            )
        } else {
            Line::raw("")
        };
        frame.render_widget(
            Paragraph::new(line).block(Block::default().borders(Borders::LEFT)),
            all_day_columns[day as usize + 1],
        );
    }

    draw_week_timeline(frame, app, start, sections[2]);
}

fn draw_week_timeline(frame: &mut Frame, app: &App, start: Date, area: Rect) {
    let columns = week_columns(area);
    let height = area.height.max(1) as i64;
    let hours_per_row = ((24 + height - 1) / height).max(1);
    let rows = ((24 + hours_per_row - 1) / hours_per_row).min(height);
    let labels = (0..rows)
        .map(|row| Line::from(format!("{:02}:00", row * hours_per_row)))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(labels).style(Style::default().fg(Color::DarkGray)),
        columns[0],
    );

    for day in 0..7 {
        let date = start + day.days();
        let events = app.events_on(date);
        let mut lines = Vec::with_capacity(rows as usize);
        for row in 0..rows {
            let hour = row * hours_per_row;
            let slot_start = date
                .at(hour as i8, 0, 0, 0)
                .to_zoned(app.timezone().clone())
                .expect("timeline time must be valid");
            let slot_end = if hour + hours_per_row >= 24 {
                (date + 1.day())
                    .at(0, 0, 0, 0)
                    .to_zoned(app.timezone().clone())
                    .expect("timeline day end must be valid")
            } else {
                date.at((hour + hours_per_row) as i8, 0, 0, 0)
                    .to_zoned(app.timezone().clone())
                    .expect("timeline time must be valid")
            };
            let overlapping = events
                .iter()
                .filter(|event| {
                    !event.all_day
                        && event.start.timestamp() < slot_end.timestamp()
                        && event.end.timestamp() > slot_start.timestamp()
                })
                .collect::<Vec<_>>();
            if let Some(event) = overlapping.first() {
                let event_start = event.start.timestamp().to_zoned(app.timezone().clone());
                let begins_here = event_start.timestamp() >= slot_start.timestamp()
                    && event_start.timestamp() < slot_end.timestamp();
                let label = if begins_here {
                    format!("▌{} {}", event_start.strftime("%H:%M"), event.summary)
                } else {
                    "████████".to_string()
                };
                lines.push(Line::styled(
                    label,
                    Style::default().fg(color_for_event(app, event.event, row as usize)),
                ));
            } else {
                lines.push(Line::styled(
                    "·",
                    Style::default().fg(Color::Rgb(45, 45, 45)),
                ));
            }
        }
        let style = if date == app.now.date() {
            Style::default().bg(Color::Rgb(25, 35, 42))
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(lines).style(style).block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            columns[day as usize + 1],
        );
    }
}

fn week_columns(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(LayoutDirection::Horizontal)
        .constraints(
            std::iter::once(Constraint::Length(7))
                .chain((0..7).map(|_| Constraint::Ratio(1, 7)))
                .collect::<Vec<_>>(),
        )
        .split(area)
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let key = |action| {
        app.config
            .hotkeys
            .normal
            .key_for(&action)
            .unwrap_or_else(|| "—".to_string())
    };
    let footer = Line::from(vec![
        Span::styled(format!(" {} ", key(NormalAction::AddEvent)), key_style()),
        Span::raw(" add  "),
        Span::styled(format!(" {} ", key(NormalAction::OpenAgenda)), key_style()),
        Span::raw(" agenda  "),
        Span::styled(format!(" {} ", key(NormalAction::Search)), key_style()),
        Span::raw(" search  "),
        Span::styled(
            format!(
                " {}/{} ",
                key(NormalAction::MonthView),
                key(NormalAction::WeekView)
            ),
            key_style(),
        ),
        Span::raw(" views  "),
        Span::styled(format!(" {} ", key(NormalAction::Help)), key_style()),
        Span::raw(" keys  "),
        Span::styled(format!(" {} ", key(NormalAction::DefaultView)), key_style()),
        Span::raw(format!(
            " back/quit    {} · {} event(s)",
            app.selected,
            app.events_on(app.selected).len()
        )),
    ]);
    frame.render_widget(Paragraph::new(footer), Rect { height: 1, ..area });
    if let Some(status) = &app.status
        && area.height > 1
    {
        frame.render_widget(
            Paragraph::new(status.as_str()).style(Style::default().fg(Color::Yellow)),
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
        );
    }
}

fn draw_editor(frame: &mut Frame, app: &App, editor: &EditState) {
    let (height, mode_name) = match editor.edit_mode {
        EditMode::Duration => (18, "start + duration"),
        EditMode::ExactRange => (15, "exact start + end"),
    };
    let area = centered(frame.area(), 76, height);
    frame.render_widget(Clear, area);
    let operation = if editor.is_editing() { "Edit" } else { "Add" };
    let recurrence = if editor.is_recurring() {
        " · this occurrence"
    } else {
        ""
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " {operation} appointment{recurrence} · {mode_name} "
            ))
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
        area,
    );
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let rows = Layout::vertical(match editor.edit_mode {
        EditMode::Duration => vec![
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
        EditMode::ExactRange => vec![
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    })
    .split(inner);
    draw_input(
        frame,
        "Description",
        &editor.description,
        editor.field == EditField::Description,
        rows[0],
    );
    let (error_row, footer_row, active_row) = match editor.edit_mode {
        EditMode::Duration => {
            draw_input(
                frame,
                "Start date",
                &editor.start_date,
                editor.field == EditField::StartDate,
                rows[1],
            );
            draw_input(
                frame,
                "Start time · 13:00 or 5pm · blank with duration = all day",
                &editor.start_time,
                editor.field == EditField::StartTime,
                rows[2],
            );
            draw_input(
                frame,
                "Duration · e.g. 45m, 1h30m, or 1:30",
                &editor.duration,
                editor.field == EditField::Duration,
                rows[3],
            );
            let active = match editor.field {
                EditField::Description => rows[0],
                EditField::StartDate => rows[1],
                EditField::StartTime => rows[2],
                EditField::Duration => rows[3],
                _ => rows[1],
            };
            (rows[4], rows[5], active)
        }
        EditMode::ExactRange => {
            draw_input(
                frame,
                "Start · date + time, e.g. jun 7 13:00",
                &editor.exact_start,
                editor.field == EditField::ExactStart,
                rows[1],
            );
            draw_input(
                frame,
                "End · date + time, e.g. jun 7 14:30",
                &editor.exact_end,
                editor.field == EditField::ExactEnd,
                rows[2],
            );
            let active = match editor.field {
                EditField::Description => rows[0],
                EditField::ExactStart => rows[1],
                EditField::ExactEnd => rows[2],
                _ => rows[1],
            };
            (rows[3], rows[4], active)
        }
    };
    if let Some(error) = &editor.error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::default().fg(Color::LightRed)),
            error_row,
        );
    }
    let bindings = &app.config.hotkeys.dialog;
    let binding = |action| bindings.key_for(&action).unwrap_or_else(|| "—".to_string());
    frame.render_widget(
        Paragraph::new(format!(
            "{} mode · {}/{} fields · {} clear · {} save · {} back",
            binding(crate::hotkey::DialogAction::ToggleMode),
            binding(crate::hotkey::DialogAction::NextField),
            binding(crate::hotkey::DialogAction::PreviousField),
            binding(crate::hotkey::DialogAction::ClearField),
            binding(crate::hotkey::DialogAction::Save),
            binding(crate::hotkey::DialogAction::Cancel),
        ))
        .style(Style::default().fg(Color::DarkGray)),
        footer_row,
    );
    let scroll = editor
        .active_input()
        .visual_scroll(active_row.width.saturating_sub(2) as usize);
    let cursor = editor.active_input().visual_cursor().saturating_sub(scroll);
    frame.set_cursor_position((active_row.x + 1 + cursor as u16, active_row.y + 1));
}

fn draw_input(frame: &mut Frame, title: &str, input: &tui_input::Input, active: bool, area: Rect) {
    let style = if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let scroll = input.visual_scroll(area.width.saturating_sub(2) as usize);
    frame.render_widget(
        Paragraph::new(input.value())
            .scroll((0, scroll as u16))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(style),
            ),
        area,
    );
}

fn draw_help(frame: &mut Frame, app: &App) {
    let normal = app.config.hotkeys.normal.rows();
    let agenda = app.config.hotkeys.agenda.rows();
    let search = app.config.hotkeys.search.rows();
    let confirm = app.config.hotkeys.confirm.rows();
    let dialog = app.config.hotkeys.dialog.rows();
    let left_height = normal.len() + 1;
    let middle_height = agenda.len() + 1;
    let right_height = search.len() + confirm.len() + dialog.len() + 8;
    let height = (left_height.max(middle_height).max(right_height) + 4)
        .min(frame.area().height.saturating_sub(2) as usize) as u16;
    let area = centered(frame.area(), 110, height.max(8));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" All hotkeys ")
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black)),
        area,
    );
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let sections = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let columns = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(sections[0]);

    let mut left = vec![Line::styled(
        "NORMAL",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    left.extend(normal.into_iter().map(binding_line));

    let mut middle = vec![Line::styled(
        "AGENDA",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    middle.extend(agenda.into_iter().map(binding_line));

    let mut right = vec![Line::styled(
        "SEARCH",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    right.extend(search.into_iter().map(binding_line));
    right.push(Line::raw(""));
    right.push(Line::styled(
        "DELETE CONFIRMATION",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    right.extend(confirm.into_iter().map(binding_line));
    right.push(Line::raw(""));
    right.push(Line::styled(
        "ADD / EDIT APPOINTMENT",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    right.extend(dialog.into_iter().map(binding_line));
    frame.render_widget(Paragraph::new(left).wrap(Wrap { trim: false }), columns[0]);
    frame.render_widget(
        Paragraph::new(middle).wrap(Wrap { trim: false }),
        columns[1],
    );
    frame.render_widget(Paragraph::new(right).wrap(Wrap { trim: false }), columns[2]);
    frame.render_widget(
        Paragraph::new("Press any key to close")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        sections[1],
    );
}

fn binding_line((key, label): (String, String)) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {key:>10} "), key_style()),
        Span::raw(format!("  {label}")),
    ])
}

fn draw_pending(frame: &mut Frame, app: &App) {
    let continuations = app.pending_hotkeys();
    if continuations.is_empty() {
        return;
    }
    let width = 38.min(frame.area().width.saturating_sub(2));
    let height = (continuations.len() as u16 + 2).min(frame.area().height.saturating_sub(2));
    if width < 12 || height < 3 {
        return;
    }
    let area = Rect {
        x: frame.area().right().saturating_sub(width + 1),
        y: frame.area().bottom().saturating_sub(height + 2),
        width,
        height,
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(
            continuations
                .into_iter()
                .map(binding_line)
                .collect::<Vec<_>>(),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} … ", app.pending_sequence())),
        )
        .style(Style::default().bg(Color::Black)),
        area,
    );
}

fn centered(area: Rect, preferred_width: u16, preferred_height: u16) -> Rect {
    let width = preferred_width.min(area.width.saturating_sub(2)).max(1);
    let height = preferred_height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn format_date(date: Date) -> String {
    date.strftime("%-d %b %Y").to_string()
}

fn key_style() -> Style {
    Style::default().bg(Color::Rgb(45, 45, 45)).fg(Color::White)
}

fn color_for_event(app: &App, event: &CalendarEvent, fallback: usize) -> Color {
    let mut calendars = app
        .events
        .iter()
        .map(|event| &event.calendar)
        .collect::<Vec<_>>();
    calendars.sort();
    calendars.dedup();
    let index = calendars
        .iter()
        .position(|calendar| *calendar == &event.calendar)
        .unwrap_or(fallback);
    calendar_color(app, &event.calendar, index)
}

fn calendar_color(app: &App, calendar: &str, fallback: usize) -> Color {
    app.config
        .calendar_colors
        .get(calendar)
        .and_then(|color| parse_color(color))
        .unwrap_or(PALETTE[fallback % PALETTE.len()])
}

fn parse_color(input: &str) -> Option<Color> {
    match input.to_ascii_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "white" => Some(Color::White),
        value if value.starts_with('#') && value.len() == 7 => Some(Color::Rgb(
            u8::from_str_radix(&value[1..3], 16).ok()?,
            u8::from_str_radix(&value[3..5], 16).ok()?,
            u8::from_str_radix(&value[5..7], 16).ok()?,
        )),
        _ => None,
    }
}
