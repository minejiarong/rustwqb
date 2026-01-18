use crate::backtest::model::BacktestStats;
use crate::commands::AppCommand;
use crate::storage::repository::{AlphaDto, FieldStatsRow};
use crossterm::event::KeyCode;
use ratatui::widgets::ListState;
use std::str::FromStr;
use tokio::sync::mpsc;

#[derive(PartialEq, Debug, Clone)]
pub enum ViewMode {
    AlphaList,
    BacktestQueue,
    Detail,
    FieldStats,
}

#[derive(PartialEq, Debug, Clone)]
pub enum InputMode {
    Normal,
    Command,
}

#[derive(PartialEq, Debug, Clone)]
pub enum FocusArea {
    Menu,     // 焦点在左侧菜单
    MainView, // 焦点在主视图
}

#[derive(Debug, Clone)]
pub struct AlphaSummary {
    pub expression: String,
    pub status: String,
    pub has_fail: bool,
    pub is_sharpe: Option<f64>,
}

#[derive(Debug)]
pub enum AppEvent {
    Log(String),
    Message(String),
    Error(String),
    Alphas(Vec<AlphaSummary>),
    Detail(AlphaDto),
    Stats(BacktestStats),
    FieldStatsRows(Vec<FieldStatsRow>),
}

pub struct App {
    pub view_mode: ViewMode,
    pub input_mode: InputMode,
    pub focus_area: FocusArea,
    pub menu_selected_index: usize,
    pub alphas_all: Vec<AlphaSummary>,
    pub alpha_list: Vec<AlphaSummary>,
    pub selected_index: usize,
    pub alpha_list_state: ListState,
    pub selected_detail: Option<AlphaDto>,
    pub backtest_stats: BacktestStats,
    pub field_stats: Vec<FieldStatsRow>,
    pub detail_scroll: u16,
    pub command_input: String,
    pub command_cursor: usize,
    pub command_history: Vec<String>,
    pub command_history_index: Option<usize>,
    pub filter_status: Option<String>,
    pub filter_query: String,
    pub filter_no_fail: bool,
    pub log_messages: Vec<String>,
    pub cmd_tx: mpsc::UnboundedSender<AppCommand>,
    pub evt_rx: Option<mpsc::UnboundedReceiver<AppEvent>>, // Changed to Option to allow taking it out
}

impl App {
    pub fn new(
        session_info: Vec<String>,
        cmd_tx: mpsc::UnboundedSender<AppCommand>,
        evt_rx: mpsc::UnboundedReceiver<AppEvent>,
    ) -> App {
        let mut log_messages = vec!["应用已启动".to_string()];
        log_messages.extend(session_info);

        App {
            view_mode: ViewMode::AlphaList,
            input_mode: InputMode::Normal,
            focus_area: FocusArea::Menu,
            menu_selected_index: 0,
            alphas_all: Vec::new(),
            alpha_list: Vec::new(),
            selected_index: 0,
            alpha_list_state: {
                let mut s = ListState::default();
                s.select(Some(0));
                s
            },
            selected_detail: None,
            backtest_stats: BacktestStats::default(),
            field_stats: Vec::new(),
            detail_scroll: 0,
            command_input: String::new(),
            command_cursor: 0,
            command_history: Vec::new(),
            command_history_index: None,
            filter_status: None,
            filter_query: String::new(),
            filter_no_fail: false,
            log_messages,
            cmd_tx,
            evt_rx: Some(evt_rx),
        }
    }

    pub fn add_log(&mut self, msg: String) {
        self.log_messages.push(msg);
    }

    /// 获取当前的预测建议
    pub fn get_completion_hint(&self) -> Option<String> {
        let commands = vec![
            "catch", "backtest", "help", "generate", "verify", "delete", "quit", "fields",
        ];
        let input = self.command_input.trim();

        if input.is_empty() {
            return None;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.len() == 1 {
            if parts[0] == "fields" {
                return Some(" sync".to_string());
            }
            if parts[0] == "generate" {
                return Some(" loop".to_string());
            }
            for cmd in commands {
                if cmd.starts_with(parts[0]) && cmd != parts[0] {
                    return Some(cmd[parts[0].len()..].to_string());
                }
            }
            return None;
        } else {
            match parts[0] {
                "fields" => {
                    let subs = ["sync", "stats", "sample"];
                    let cur = parts.get(1).copied().unwrap_or("");
                    for s in subs {
                        if s.starts_with(cur) && s != cur {
                            return Some(s[cur.len()..].to_string());
                        }
                    }
                    return None;
                }
                "generate" => {
                    let subs = ["once", "loop", "stop"];
                    let cur = parts.get(1).copied().unwrap_or("");
                    for s in subs {
                        if s.starts_with(cur) && s != cur {
                            return Some(s[cur.len()..].to_string());
                        }
                    }
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    pub fn clamp_selection(&mut self) {
        if self.selected_index >= self.alpha_list.len() {
            self.selected_index = self.alpha_list.len().saturating_sub(1);
        }
        self.alpha_list_state.select(Some(self.selected_index));
    }

    pub fn apply_filters(&mut self) {
        let mut filtered: Vec<AlphaSummary> = self
            .alphas_all
            .iter()
            .filter(|a| {
                if let Some(status) = &self.filter_status {
                    if &a.status != status {
                        return false;
                    }
                }
                if self.filter_no_fail && a.has_fail {
                    return false;
                }
                if !self.filter_query.is_empty() {
                    if !a.expression.contains(&self.filter_query) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        if filtered.is_empty() && !self.alphas_all.is_empty() && self.filter_status.is_some() {
            filtered = self.alphas_all.clone();
            self.filter_status = None;
        }

        filtered.sort_by(|a, b| {
            let a_sharpe = a.is_sharpe.filter(|x| x.is_finite());
            let b_sharpe = b.is_sharpe.filter(|x| x.is_finite());
            match (a_sharpe, b_sharpe) {
                (Some(sa), Some(sb)) => sb.total_cmp(&sa),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        self.alpha_list = filtered;
        if self.selected_index >= self.alpha_list.len() {
            self.selected_index = 0;
        }
        self.alpha_list_state.select(Some(self.selected_index));
    }

    /// 请求详情数据
    pub fn request_detail(&mut self) {
        if let Some(alpha) = self.alpha_list.get(self.selected_index) {
            self.detail_scroll = 0; // 切换 Alpha 时重置滚动
                                    // Send AppCommand::GetDetail
            let _ = self.cmd_tx.send(AppCommand::GetDetail {
                expr: alpha.expression.clone(),
            });
        }
    }

    pub fn request_field_stats(&mut self) {
        let _ = self.cmd_tx.send(AppCommand::FieldStats);
    }

    pub fn handle_key_event(&mut self, key: KeyCode) -> bool {
        if self.input_mode == InputMode::Command {
            match key {
                KeyCode::Enter => {
                    let cmd_owned = self.command_input.trim().to_string();
                    if cmd_owned.is_empty() {
                        self.command_input.clear();
                        self.command_cursor = 0;
                        self.input_mode = InputMode::Normal;
                        return false;
                    }

                    if cmd_owned == "q" {
                        self.command_input.clear();
                        self.command_cursor = 0;
                        self.input_mode = InputMode::Normal;
                        return false;
                    } else {
                        if let Some(rest) = cmd_owned.strip_prefix("filter") {
                            let args = rest.trim();
                            if args.is_empty() {
                                self.filter_query.clear();
                                self.filter_no_fail = false;
                            } else if args == "clear" || args == "--clear" {
                                self.filter_query.clear();
                                self.filter_no_fail = false;
                            } else {
                                let mut nofail = self.filter_no_fail;
                                let mut query_parts: Vec<&str> = Vec::new();
                                for tok in args.split_whitespace() {
                                    let t = tok.to_ascii_lowercase();
                                    if t == "nofail" || t == "--nofail" || t == "--no-fail" {
                                        nofail = true;
                                        continue;
                                    }
                                    if t == "nofail=0"
                                        || t == "nofail=false"
                                        || t == "nofail=off"
                                        || t == "--nofail=0"
                                        || t == "--nofail=false"
                                        || t == "--nofail=off"
                                        || t == "--no-fail=0"
                                        || t == "--no-fail=false"
                                        || t == "--no-fail=off"
                                    {
                                        nofail = false;
                                        continue;
                                    }
                                    query_parts.push(tok);
                                }
                                self.filter_no_fail = nofail;
                                self.filter_query = query_parts.join(" ");
                            }
                            self.apply_filters();
                            self.command_history.push(cmd_owned.clone());
                            self.command_history_index = None;
                            self.command_input.clear();
                            self.command_cursor = 0;
                            self.input_mode = InputMode::Normal;
                            return false;
                        }
                        // Parse command
                        if let Ok(app_cmd) = AppCommand::from_str(&cmd_owned) {
                            let _ = self.cmd_tx.send(app_cmd);
                        } else {
                            // Should technically not happen with my parser implementation
                            // but good to be safe
                            let _ = self.cmd_tx.send(AppCommand::Unknown(cmd_owned.clone()));
                        }

                        self.command_history.push(cmd_owned);
                        self.command_history_index = None;
                        self.command_input.clear();
                        self.command_cursor = 0;
                        self.input_mode = InputMode::Normal;
                        return false;
                    }
                }
                KeyCode::Esc => {
                    self.command_input.clear();
                    self.command_cursor = 0;
                    self.input_mode = InputMode::Normal;
                    return false;
                }
                KeyCode::Tab => {
                    if let Some(hint) = self.get_completion_hint() {
                        let insert = format!("{} ", hint);
                        self.command_input.insert_str(self.command_cursor, &insert);
                        self.command_cursor += insert.len();
                    }
                    return false;
                }
                KeyCode::Up => {
                    if self.command_history.is_empty() {
                        return false;
                    }
                    let next = match self.command_history_index {
                        None => self.command_history.len().saturating_sub(1),
                        Some(i) => i.saturating_sub(1),
                    };
                    self.command_history_index = Some(next);
                    if let Some(cmd) = self.command_history.get(next) {
                        self.command_input = cmd.clone();
                        self.command_cursor = self.command_input.len();
                    }
                    return false;
                }
                KeyCode::Down => {
                    if self.command_history.is_empty() {
                        return false;
                    }
                    let next = match self.command_history_index {
                        None => return false,
                        Some(i) => {
                            let n = i + 1;
                            if n >= self.command_history.len() {
                                self.command_history_index = None;
                                self.command_input.clear();
                                self.command_cursor = 0;
                                return false;
                            }
                            n
                        }
                    };
                    self.command_history_index = Some(next);
                    if let Some(cmd) = self.command_history.get(next) {
                        self.command_input = cmd.clone();
                        self.command_cursor = self.command_input.len();
                    }
                    return false;
                }
                KeyCode::Backspace => {
                    if self.command_cursor > 0 && !self.command_input.is_empty() {
                        let idx = self.command_cursor - 1;
                        self.command_input.remove(idx);
                        self.command_cursor = self.command_cursor.saturating_sub(1);
                    }
                    return false;
                }
                KeyCode::Delete => {
                    if self.command_cursor < self.command_input.len() {
                        self.command_input.remove(self.command_cursor);
                    }
                    return false;
                }
                KeyCode::Left => {
                    if self.command_cursor > 0 {
                        self.command_cursor -= 1;
                    }
                    return false;
                }
                KeyCode::Right => {
                    if self.command_cursor < self.command_input.len() {
                        self.command_cursor += 1;
                    }
                    return false;
                }
                KeyCode::Home => {
                    self.command_cursor = 0;
                    return false;
                }
                KeyCode::End => {
                    self.command_cursor = self.command_input.len();
                    return false;
                }
                KeyCode::Char(c) => {
                    self.command_input.insert(self.command_cursor, c);
                    self.command_cursor += 1;
                    return false;
                }
                _ => return false,
            }
        }

        // 正常模式下的按键处理
        match key {
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Command;
                self.command_input.clear();
                self.command_cursor = 0;
                false
            }
            KeyCode::Char('q') => {
                true // 退出应用
            }
            KeyCode::Left => {
                // 左箭头：切换到菜单焦点
                self.focus_area = FocusArea::Menu;
                false
            }
            KeyCode::Right => {
                // 右箭头：切换到主视图焦点
                self.focus_area = FocusArea::MainView;
                false
            }
            KeyCode::Up => {
                if self.focus_area == FocusArea::Menu {
                    // 在菜单中向上导航
                    if self.menu_selected_index > 0 {
                        self.menu_selected_index -= 1;
                    }
                } else {
                    // 在主视图中
                    if self.view_mode == ViewMode::Detail {
                        // 详情页向上滚动
                        self.detail_scroll = self.detail_scroll.saturating_sub(1);
                    } else if self.selected_index > 0 {
                        // 在 Alpha 列表中向上导航
                        self.selected_index -= 1;
                    }
                }
                false
            }
            KeyCode::Down => {
                if self.focus_area == FocusArea::Menu {
                    // 在菜单中向下导航
                    let menu_items_count = 4;
                    if self.menu_selected_index < menu_items_count - 1 {
                        self.menu_selected_index += 1;
                    }
                } else {
                    // 在主视图中
                    if self.view_mode == ViewMode::Detail {
                        // 详情页向下滚动
                        self.detail_scroll = self.detail_scroll.saturating_add(1);
                    } else if self.selected_index < self.alpha_list.len().saturating_sub(1) {
                        // 在 Alpha 列表中向下导航
                        self.selected_index += 1;
                    }
                }
                false
            }
            KeyCode::Enter | KeyCode::Char('c') => {
                // Enter 或 c 键：确认选择
                if self.focus_area == FocusArea::Menu {
                    // 根据菜单选择切换视图
                    match self.menu_selected_index {
                        0 => {
                            self.view_mode = ViewMode::AlphaList;
                        }
                        1 => {
                            self.view_mode = ViewMode::BacktestQueue;
                        }
                        2 => {
                            self.view_mode = ViewMode::Detail;
                            self.request_detail(); // 切换到详情时请求数据
                        }
                        3 => {
                            self.view_mode = ViewMode::FieldStats;
                            self.request_field_stats();
                        }
                        _ => {}
                    }
                    // 确认后自动切换焦点到主视图
                    self.focus_area = FocusArea::MainView;
                } else if self.focus_area == FocusArea::MainView {
                    // 如果在主视图列表按 Enter/c，直接进入详情页
                    if self.view_mode == ViewMode::AlphaList && !self.alpha_list.is_empty() {
                        self.view_mode = ViewMode::Detail;
                        self.menu_selected_index = 2; // 同时同步左侧菜单的状态
                        self.request_detail(); // 切换到详情时请求数据
                    }
                }
                false
            }
            KeyCode::Char('x') => {
                if self.focus_area == FocusArea::MainView && self.view_mode == ViewMode::Detail {
                    self.view_mode = ViewMode::AlphaList;
                    self.menu_selected_index = 0;
                }
                false
            }
            KeyCode::Char('f') => {
                if self.focus_area == FocusArea::MainView && self.view_mode == ViewMode::AlphaList {
                    self.filter_status = match self.filter_status.as_deref() {
                        None => Some("DONE".to_string()),
                        Some("DONE") => Some("ERROR".to_string()),
                        Some("ERROR") => Some("PENDING".to_string()),
                        Some("PENDING") => Some("SIMULATING".to_string()),
                        Some("SIMULATING") => None,
                        _ => None,
                    };
                    self.apply_filters();
                }
                false
            }
            _ => false,
        }
    }
}
