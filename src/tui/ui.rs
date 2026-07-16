use super::{App, InputAction, InputState};
use crate::config::{BlockedResponse, Upstream};
use chrono::DateTime;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use tracing::Level;

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Bottom panel grows with the number of configured sources
    let bottom_height =
        (5 + app.sources.len() + app.config.blocklist.remote_lists.len()).min(14) as u16;

    let [main, bottom] =
        Layout::vertical([Constraint::Min(10), Constraint::Length(bottom_height)]).areas(area);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(main);

    draw_left(frame, app, left);
    draw_right(frame, app, right);
    draw_blocklists(frame, app, bottom);

    if let Some(input) = &app.input {
        draw_input_popup(frame, input, area);
    }
}

/// Left column: ASCII art banner + live activity log
fn draw_left(frame: &mut Frame, app: &App, area: Rect) {
    let [banner_area, log_area] =
        Layout::vertical([Constraint::Length(8), Constraint::Min(3)]).areas(area);

    let mut banner_lines: Vec<Line> = crate::cli::BANNER
        .trim_matches('\n')
        .lines()
        .map(|l| Line::from(l.cyan()))
        .collect();
    banner_lines.push(Line::from(vec![
        Span::styled(
            "   Skypier Blackhole ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("v{} · DNS sinkhole", env!("CARGO_PKG_VERSION")),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(banner_lines).block(Block::default().borders(Borders::ALL)),
        banner_area,
    );

    // Bottom-anchored log tail
    let visible = log_area.height.saturating_sub(2) as usize;
    let logs = app.logs.lock().unwrap();
    let lines: Vec<Line> = logs
        .iter()
        .skip(logs.len().saturating_sub(visible))
        .map(|log| {
            let level = match log.level {
                Level::TRACE => "TRACE".magenta().bold(),
                Level::DEBUG => "DEBUG".blue().bold(),
                Level::INFO => "INFO ".green().bold(),
                Level::WARN => "WARN ".yellow().bold(),
                Level::ERROR => "ERROR".red().bold(),
            };
            let body = if log.blocked_domain.is_some() {
                log.body.clone().red()
            } else {
                log.body.clone().into()
            };
            let mut spans = vec![
                log.time.clone().dark_gray(),
                " ".into(),
                level,
                " ".into(),
                body,
            ];
            if log.repeat > 1 {
                spans.push(format!(" (x{})", log.repeat).dark_gray());
            }
            Line::from(spans)
        })
        .collect();
    drop(logs);

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Activity log "),
        ),
        log_area,
    );
}

/// Right column: upstreams, session stats, top blocked domains, config
fn draw_right(frame: &mut Frame, app: &App, area: Rect) {
    let upstream_count = app.config.server.upstream_dns.len().max(1) as u16;
    let [upstream_area, stats_area, top_area, config_area] = Layout::vertical([
        Constraint::Length(upstream_count + 2),
        Constraint::Length(8),
        Constraint::Min(4),
        Constraint::Length(8),
    ])
    .areas(area);

    draw_upstreams(frame, app, upstream_area);
    draw_stats(frame, app, stats_area);
    draw_top_blocked(frame, app, top_area);
    draw_config(frame, app, config_area);
}

fn draw_upstreams(frame: &mut Frame, app: &App, area: Rect) {
    let multiple = app.config.server.upstream_dns.len() > 1;
    let lines: Vec<Line> = app
        .config
        .server
        .upstream_dns
        .iter()
        .map(|upstream| {
            let suffix = if multiple {
                " (random per query)".dark_gray()
            } else {
                "".into()
            };
            let kind = match upstream {
                Upstream::Udp(_) => "UDP".cyan().bold(),
                Upstream::DoH { .. } => "DoH".magenta().bold(),
            };
            Line::from(vec![
                "● ".green(),
                kind,
                "  ".into(),
                upstream.to_string().into(),
                suffix,
            ])
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Upstream DNS "),
        ),
        area,
    );
}

fn draw_stats(frame: &mut Frame, app: &App, area: Rect) {
    let metrics = &app.metrics;
    let total = metrics.total_queries();
    let blocked = metrics.blocked_queries();
    let block_rate = if total > 0 {
        blocked as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    let lines = vec![
        stat_line("Uptime", format_duration(metrics.uptime()), Color::White),
        stat_line("Total queries", total.to_string(), Color::White),
        stat_line(
            "Allowed",
            metrics.allowed_queries().to_string(),
            Color::Green,
        ),
        stat_line("Blocked", blocked.to_string(), Color::Red),
        stat_line("Block rate", format!("{:.1}%", block_rate), Color::Yellow),
        stat_line(
            "Distinct domains",
            metrics.distinct_blocked().to_string(),
            Color::Red,
        ),
    ];

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Session stats "),
        ),
        area,
    );
}

fn draw_top_blocked(frame: &mut Frame, app: &App, area: Rect) {
    let visible = area.height.saturating_sub(2) as usize;
    let top = app.metrics.top_blocked(visible.max(1));

    let lines: Vec<Line> = if top.is_empty() {
        vec![Line::from("No blocked queries yet".dark_gray())]
    } else {
        top.iter()
            .map(|(domain, count)| {
                Line::from(vec![
                    Span::styled(
                        format!("{:>6}  ", count),
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    domain.clone().into(),
                ])
            })
            .collect()
    };

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Top blocked (since start) "),
        ),
        area,
    );
}

fn draw_config(frame: &mut Frame, app: &App, area: Rect) {
    let server = &app.config.server;
    let response = match &server.blocked_response {
        BlockedResponse::Refused => "REFUSED".to_string(),
        BlockedResponse::NxDomain => "NXDOMAIN".to_string(),
        BlockedResponse::Ip(ip) => format!("IP {}", ip),
    };
    let updater = if app.config.updater.enabled {
        format!(
            "{} ({})",
            app.config.updater.schedule, app.config.updater.timezone
        )
    } else {
        "disabled".to_string()
    };

    let lines = vec![
        stat_line(
            "Listen",
            format!("{}:{}", server.listen_addr, server.listen_port),
            Color::Cyan,
        ),
        stat_line("Blocked response", response, Color::White),
        stat_line(
            "Wildcards",
            if app.config.blocklist.enable_wildcards {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
            Color::White,
        ),
        stat_line(
            "Log level",
            app.config.logging.log_level.clone(),
            Color::White,
        ),
        stat_line("Auto-update", updater, Color::White),
        stat_line("Config file", app.config_path.clone(), Color::DarkGray),
    ];

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Configuration "),
        ),
        area,
    );
}

/// Bottom panel: blocklist sources, totals, update info, and key bindings
fn draw_blocklists(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![Line::from(vec![
        "Total blocked domains: ".into(),
        Span::styled(
            app.total_domains.to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    for source in &app.sources {
        let count = match source.domains {
            Some(n) => Span::styled(
                format!("{:>8} domains", n),
                Style::default().fg(Color::Green),
            ),
            None => Span::styled("     missing", Style::default().fg(Color::DarkGray)),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<13}", source.kind.label()),
                Style::default().fg(Color::Cyan),
            ),
            count,
            "  ".into(),
            source.path.display().to_string().dark_gray(),
        ]));
    }
    for url in &app.config.blocklist.remote_lists {
        lines.push(Line::from(vec![
            Span::styled("  remote url   ", Style::default().fg(Color::Cyan)),
            url.clone().blue(),
        ]));
    }

    let last_update = app
        .last_update
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "never".to_string());
    let next_update = app
        .next_run
        .map(|t| {
            DateTime::<chrono::Local>::from(t)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string());
    lines.push(Line::from(vec![
        "Last remote update: ".into(),
        last_update.white(),
        "   Next scheduled: ".into(),
        next_update.white(),
    ]));

    lines.push(Line::from(vec![
        key_hint("a", "add"),
        key_hint("d", "remove"),
        key_hint("u", "update now"),
        key_hint("r", "reload"),
        key_hint("q", "quit"),
    ]));

    let title = if app.updating.load(std::sync::atomic::Ordering::SeqCst) {
        " Blocklists (updating…) "
    } else {
        " Blocklists "
    };

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn draw_input_popup(frame: &mut Frame, input: &InputState, area: Rect) {
    let title = match input.action {
        InputAction::Add => " Add domain (Enter to confirm, Esc to cancel) ",
        InputAction::Remove => " Remove domain (Enter to confirm, Esc to cancel) ",
    };
    let width = 60.min(area.width.saturating_sub(4));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + area.height.saturating_sub(3) / 2,
        width,
        height: 3,
    }
    // Rendering outside the buffer panics; clamp for tiny terminals
    .intersection(area);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            "> ".dark_gray(),
            input.buffer.clone().into(),
            "█".into(),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        popup,
    );
}

fn stat_line(label: &str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<18}", label),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(value, Style::default().fg(color)),
    ])
}

fn key_hint(key: &str, action: &str) -> Span<'static> {
    Span::styled(
        format!(" [{}] {} ", key, action),
        Style::default().fg(Color::Black).bg(Color::Cyan),
    )
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let (days, hours, mins) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if days > 0 {
        format!("{}d {:02}h {:02}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {:02}m {:02}s", hours, mins, secs % 60)
    } else {
        format!("{}m {:02}s", mins, secs % 60)
    }
}
