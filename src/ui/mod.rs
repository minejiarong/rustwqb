use crate::app_state::{App, FocusArea, InputMode, ViewMode};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use serde_json::Value;

pub fn draw(f: &mut Frame, app: &mut App) {
    // 创建布局
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 顶部标题栏
            Constraint::Min(0),    // 中间内容区域
            Constraint::Min(8),    // 底部命令/日志区域（增加高度以显示更多日志）
        ])
        .split(f.size());

    // 顶部标题栏
    render_top_bar(f, chunks[0]);

    // 中间内容区域（左侧菜单 + 主视图）
    let middle_chunks = Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(0)])
        .split(chunks[1]);

    // 左侧菜单
    render_left_menu(f, middle_chunks[0], app);

    // 主视图
    render_main_view(f, middle_chunks[1], app);

    // 底部命令/日志区域
    render_bottom_bar(f, chunks[2], app);
}

fn render_top_bar(f: &mut Frame, area: Rect) {
    let title = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));

    let title_text = Line::from(vec![
        Span::styled(
            " Alpha 管理工具 ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" - Terminal TUI"),
    ]);

    let paragraph = Paragraph::new(title_text)
        .block(title)
        .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(paragraph, area);
}

fn render_left_menu(f: &mut Frame, area: Rect, app: &App) {
    let menu_items: Vec<ListItem> = vec!["Alpha 列表", "回测任务", "详细信息", "字段统计"]
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let is_selected = i == app.menu_selected_index;
            let is_active = match (i, &app.view_mode) {
                (0, ViewMode::AlphaList) => true,
                (1, ViewMode::BacktestQueue) => true,
                (2, ViewMode::Detail) => true,
                (3, ViewMode::FieldStats) => true,
                _ => false,
            };

            let style = if is_selected {
                // 选中的菜单项
                if app.focus_area == FocusArea::Menu {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Magenta)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD)
                }
            } else if is_active {
                // 当前激活的视图
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_active { "● " } else { "○ " };
            ListItem::new(format!("{}{}", prefix, text)).style(style)
        })
        .collect();

    let title = if app.focus_area == FocusArea::Menu {
        "菜单 (Enter/c 确认)"
    } else {
        "菜单 (← 切换)"
    };

    let menu =
        List::new(menu_items).block(Block::default().borders(Borders::ALL).title(title).style(
            if app.focus_area == FocusArea::Menu {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            },
        ));

    f.render_widget(menu, area);
}

fn render_main_view(f: &mut Frame, area: Rect, app: &mut App) {
    match app.view_mode {
        ViewMode::AlphaList => {
            let items: Vec<ListItem> = app
                .alpha_list
                .iter()
                .enumerate()
                .map(|(i, alpha)| {
                    // 根据状态选择颜色和符号
                    let (status_symbol, status_color) = match alpha.status.as_str() {
                        "DONE" => ("✓", Color::Green),
                        "ERROR" => ("✗", Color::Red),
                        "SIMULATING" => ("▶", Color::Cyan),
                        "PENDING" => ("○", Color::Yellow),
                        _ => ("?", Color::Gray),
                    };

                    let is_selected = i == app.selected_index;
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let content = Line::from(vec![
                        Span::styled(
                            format!("{} ", status_symbol),
                            Style::default().fg(status_color),
                        ),
                        Span::styled(
                            format!("{:<12}", alpha.status),
                            Style::default().fg(status_color),
                        ),
                        Span::raw(&alpha.expression),
                    ]);

                    ListItem::new(content).style(style)
                })
                .collect();

            let status_filter = app.filter_status.as_deref().unwrap_or("ALL");
            let query_info = if app.filter_query.is_empty() {
                String::new()
            } else {
                format!(" 搜索: \"{}\"", app.filter_query)
            };
            let title = if app.focus_area == FocusArea::MainView {
                format!(
                    "Alpha 列表 [Filter: {}]{} (f 切换, / 搜索, Enter/c 详情, ← 菜单)",
                    status_filter, query_info
                )
            } else {
                format!("Alpha 列表 [Filter: {}]{}", status_filter, query_info)
            };

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title).style(
                    if app.focus_area == FocusArea::MainView {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ))
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(">> ");
            app.alpha_list_state.select(Some(app.selected_index));
            f.render_stateful_widget(list, area, &mut app.alpha_list_state);
        }
        ViewMode::BacktestQueue => {
            let stats = &app.backtest_stats;
            let content = vec![
                Line::from(vec![Span::styled(
                    "--- 任务队列概览 ---",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![Span::raw(format!("  总计任务: {:>4}", stats.total))]),
                Line::from(vec![Span::styled(
                    format!("  待处理  : {:>4}", stats.pending),
                    Style::default().fg(Color::Yellow),
                )]),
                Line::from(vec![Span::styled(
                    format!("  模拟中  : {:>4}", stats.running),
                    Style::default().fg(Color::Cyan),
                )]),
                Line::from(vec![Span::styled(
                    format!("  已完成  : {:>4}", stats.completed),
                    Style::default().fg(Color::Green),
                )]),
                Line::from(vec![Span::styled(
                    format!("  可重试错误: {:>4}", stats.error_retryable),
                    Style::default().fg(Color::Red),
                )]),
                Line::from(vec![Span::styled(
                    format!("  严重错误  : {:>4}", stats.error_fatal),
                    Style::default().fg(Color::LightRed),
                )]),
                Line::from(vec![Span::styled(
                    format!("  次数超限  : {:>4}", stats.error_exceeded),
                    Style::default().fg(Color::Gray),
                )]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "提示: 后台 Service 每 5 秒自动扫描并执行 PENDING 任务",
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::ITALIC),
                )]),
            ];

            let title = if app.focus_area == FocusArea::MainView {
                "回测任务情况 (← 切换菜单)"
            } else {
                "回测任务情况"
            };

            let paragraph = Paragraph::new(content).block(
                Block::default().borders(Borders::ALL).title(title).style(
                    if app.focus_area == FocusArea::MainView {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            );
            f.render_widget(paragraph, area);
        }
        ViewMode::Detail => {
            let content = if let Some(ref detail) = app.selected_detail {
                let mut lines = vec![
                    Line::from(vec![
                        Span::styled("表达式: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(&detail.expression, Style::default().fg(Color::Cyan)),
                    ]),
                    Line::from(vec![
                        Span::styled("状态: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(&detail.status),
                    ]),
                    Line::from(vec![
                        Span::styled("区域: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(&detail.region),
                        Span::raw("  "),
                        Span::styled("Universe: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(&detail.universe),
                    ]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "--- 核心指标 (IS) ---",
                        Style::default().fg(Color::Yellow),
                    )]),
                ];

                // 渲染数值指标 (从 core_metrics 字段读取)
                let sharpe = detail
                    .core_metrics
                    .is_sharpe
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "N/A".to_string());
                let fitness = detail
                    .core_metrics
                    .is_fitness
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "N/A".to_string());
                let turnover = detail
                    .core_metrics
                    .is_turnover
                    .map(|v| format!("{:.2}%", v * 100.0))
                    .unwrap_or_else(|| "N/A".to_string());
                let returns = detail
                    .core_metrics
                    .is_returns
                    .map(|v| format!("{:.2}%", v * 100.0))
                    .unwrap_or_else(|| "N/A".to_string());

                lines.push(Line::from(format!(
                    "Sharpe:  {:<10} Fitness: {:<10}",
                    sharpe, fitness
                )));
                lines.push(Line::from(format!(
                    "Returns: {:<10} Turnover: {:<10}",
                    returns, turnover
                )));

                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "--- 检查详情 (Checks) ---",
                    Style::default().fg(Color::Yellow),
                )]));

                // 解析并展示全部 Checks
                if let Value::Array(ref checks) = detail.checks_json {
                    for check in checks.iter() {
                        let name = check["name"].as_str().unwrap_or("?");
                        let result = check["result"].as_str().unwrap_or("?");
                        let color = if result == "PASS" {
                            Color::Green
                        } else if result == "FAIL" {
                            Color::Red
                        } else {
                            Color::Yellow
                        };
                        lines.push(Line::from(vec![
                            Span::raw(format!("  • {:<25}: ", name)),
                            Span::styled(result, Style::default().fg(color)),
                        ]));
                    }
                }

                lines
            } else {
                vec![Line::from("正在加载详情...")]
            };

            let title = if app.focus_area == FocusArea::MainView {
                "详细信息 (↑↓ 滚动, ← 切换菜单)"
            } else {
                "详细信息"
            };

            let paragraph = Paragraph::new(content)
                .block(Block::default().borders(Borders::ALL).title(title).style(
                    if app.focus_area == FocusArea::MainView {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ))
                .scroll((app.detail_scroll, 0)); // 应用滚动偏移
            f.render_widget(paragraph, area);
        }
        ViewMode::FieldStats => {
            let mut lines = vec![
                Line::from(vec![Span::styled(
                    "--- 字段统计 (Region / Universe / Delay) ---",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
            ];
            for row in &app.field_stats {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{:<6}", row.region),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(" | "),
                    Span::styled(
                        format!("{:<12}", row.universe),
                        Style::default().fg(Color::White),
                    ),
                    Span::raw(" | "),
                    Span::styled(
                        format!("延迟 {:>2}", row.delay),
                        Style::default().fg(Color::Magenta),
                    ),
                    Span::raw(" | "),
                    Span::styled(
                        format!("数量 {:>6}", row.count),
                        Style::default().fg(Color::Green),
                    ),
                ]));
            }
            if app.field_stats.is_empty() {
                lines.push(Line::from("暂无数据，按菜单确认或输入 `fields stats` 加载"));
            }
            let title = if app.focus_area == FocusArea::MainView {
                "字段统计 (← 切换菜单)"
            } else {
                "字段统计"
            };
            let paragraph = Paragraph::new(lines).block(
                Block::default().borders(Borders::ALL).title(title).style(
                    if app.focus_area == FocusArea::MainView {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            );
            f.render_widget(paragraph, area);
        }
    }
}

fn render_bottom_bar(f: &mut Frame, area: Rect, app: &App) {
    let bottom_chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // 命令输入区域
    let command_prompt = if app.input_mode == InputMode::Command {
        let mut spans = vec![Span::styled(
            "命令: ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )];
        let cur = app.command_cursor.min(app.command_input.len());
        let (left, right) = app.command_input.split_at(cur);
        spans.push(Span::raw(left));
        spans.push(Span::styled("_", Style::default().fg(Color::Yellow)));
        spans.push(Span::raw(right));

        // 如果有建议，添加浅灰色幽灵文本
        if let Some(hint) = app.get_completion_hint() {
            spans.push(Span::styled(hint, Style::default().fg(Color::DarkGray)));
        }

        vec![
            Line::from(spans),
            Line::from("Enter执行 Esc取消 Tab补全 ←→光标 Home/End ↑历史 ↓下一条 q退出"),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("命令: ", Style::default().fg(Color::Yellow)),
                Span::raw("(按 / 进入命令模式)"),
            ]),
            Line::from("/命令 f筛选 /搜索 ←→切换 ↑↓导航 Enter/c确认 x返回 q退出"),
        ]
    };
    let command_paragraph = Paragraph::new(command_prompt).block(
        Block::default()
            .borders(Borders::ALL)
            .title(if app.input_mode == InputMode::Command {
                "命令输入模式"
            } else {
                "命令输入"
            })
            .style(if app.input_mode == InputMode::Command {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            }),
    );
    f.render_widget(command_paragraph, bottom_chunks[0]);

    // 日志区域 - 显示最近的日志消息（最多显示最后20条）
    let log_items: Vec<ListItem> = app
        .log_messages
        .iter()
        .rev() // 反转，显示最新的在顶部
        .take(20) // 最多显示20条
        .map(|msg| {
            // 根据消息类型设置不同的样式
            let style = if msg.starts_with("✓") {
                Style::default().fg(Color::Green)
            } else if msg.starts_with("✗") {
                Style::default().fg(Color::Red)
            } else if msg.starts_with("⚠") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(msg.as_str()).style(style)
        })
        .collect();

    let log = List::new(log_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("日志 (共 {} 条)", app.log_messages.len()))
            .style(Style::default().fg(Color::White)),
    );
    f.render_widget(log, bottom_chunks[1]);
}
