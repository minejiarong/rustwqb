mod ai;
mod app_service;
mod app_state;
mod backtest;
mod commands;
mod generate;
mod session;
mod storage;
mod ui;

use chrono::Local;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use sea_orm::EntityTrait;
use session::WQBSession;
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::app_service::refresh_ui;
use crate::app_state::{App, AppEvent};
use crate::commands::AppCommand;
use crate::storage::entity::Alpha;
use crate::storage::repository::{AlphaDto, DataFieldRepository};
use crate::ui::draw;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> io::Result<()> {
    let ts = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let log_dir = std::path::PathBuf::from("logs");
    std::fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join(format!("app-{}.log", ts));
    let log_file = std::fs::File::create(log_path)?;
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(Box::new(log_file))) // 核心：重定向输出到文件
        .filter_level(log::LevelFilter::Warn)
        .filter_module("rustwqb", log::LevelFilter::Info)
        .filter_module("sqlx", log::LevelFilter::Error)
        .filter_module("sea_orm", log::LevelFilter::Error)
        .init();

    // 加载环境变量
    let mut session_info = Vec::new();

    // 获取当前工作目录
    let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    session_info.push(format!("当前工作目录: {}", current_dir.display()));

    // 检查 .env 文件是否存在
    let env_path = current_dir.join(".env");
    let env_exists = env_path.exists();
    if env_exists {
        session_info.push(format!("✓ 找到 .env 文件: {}", env_path.display()));
    } else {
        session_info.push(format!("⚠ 未找到 .env 文件: {}", env_path.display()));
    }

    // 尝试加载 .env 文件（直接手动解析，避免递归栈问题）
    let env_loaded = if env_exists {
        if let Ok(content) = std::fs::read_to_string(&env_path) {
            session_info.push(format!("✓ 读取 .env 文件: {}", env_path.display()));
            let mut loaded = false;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some(equal_pos) = line.find('=') {
                    let key = line[..equal_pos].trim();
                    let value = line[equal_pos + 1..].trim();
                    let value = value.trim_matches(|c| c == '"' || c == '\'');
                    std::env::set_var(key, value);
                    loaded = true;
                }
            }
            loaded
        } else {
            session_info.push("⚠ 无法读取 .env 文件".to_string());
            false
        }
    } else {
        session_info.push("⚠ 未找到 .env 文件".to_string());
        false
    };

    if !env_loaded {
        session_info.push("⚠ 尝试从系统环境变量读取".to_string());
    }

    // 初始化数据库
    session_info.push("正在初始化数据库...".to_string());
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://alphas.db?mode=rwc".to_string());
    let db = match storage::establish_connection(&db_url).await {
        Ok(connection) => {
            session_info.push("✓ 数据库连接成功".to_string());
            Arc::new(connection)
        }
        Err(e) => {
            eprintln!("无法连接数据库: {}", e);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("数据库连接失败: {}", e),
            ));
        }
    };

    // 读取账号信息并创建 session
    let (session, info) = match (std::env::var("WQB_EMAIL"), std::env::var("WQB_PASSWORD")) {
        (Ok(email), Ok(password)) => {
            session_info.push(format!("✓ 已读取账号信息: {}", email));
            session_info.push("正在创建 WQB Session...".to_string());

            match create_session(email.clone(), password, &mut session_info).await {
                Ok(sess) => {
                    session_info.push("✓ Session 创建成功！".to_string());
                    (Some(sess), session_info.clone())
                }
                Err(e) => {
                    session_info.push(format!("✗ 创建 Session 失败: {}", e));
                    (None, session_info.clone())
                }
            }
        }
        (Err(e1), Err(e2)) => {
            session_info.push("✗ 未找到 WQB_EMAIL 和 WQB_PASSWORD 环境变量".to_string());
            session_info.push(format!("  WQB_EMAIL 错误: {}", e1));
            session_info.push(format!("  WQB_PASSWORD 错误: {}", e2));
            session_info.push("请创建 .env 文件并设置以下变量:".to_string());
            session_info.push("  WQB_EMAIL=your_email@example.com".to_string());
            session_info.push("  WQB_PASSWORD=your_password".to_string());
            (None, session_info.clone())
        }
        (Ok(email), Err(e)) => {
            session_info.push(format!("✓ 已读取 WQB_EMAIL: {}", email));
            session_info.push(format!("✗ 未找到 WQB_PASSWORD: {}", e));
            session_info.push("请在 .env 文件中设置 WQB_PASSWORD".to_string());
            (None, session_info.clone())
        }
        (Err(e), Ok(_)) => {
            session_info.push(format!("✗ 未找到 WQB_EMAIL: {}", e));
            session_info.push("✓ 已读取 WQB_PASSWORD".to_string());
            session_info.push("请在 .env 文件中设置 WQB_EMAIL".to_string());
            (None, session_info.clone())
        }
    };

    // 创建核心 Channel (使用 AppCommand)
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<AppCommand>();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel::<AppEvent>();

    // 启动单后台任务模型 (Actor)
    let session_bg = session.map(Arc::new);
    let db_bg = Arc::clone(&db);
    let evt_tx_bg = evt_tx.clone();

    tokio::spawn(async move {
        use crate::ai::AnyProvider;
        use crate::backtest::BacktestService;
        use crate::generate::context::{ApiContextProvider, GenerateContextProvider};
        use crate::generate::field_sync::FieldSyncService;
        use crate::generate::{GenerateConfig, GeneratorService};

        // 1. 初始化 BacktestService
        let backtest_service = if let Some(ref sess) = session_bg {
            Some(BacktestService::new(
                db_bg.clone(),
                sess.clone(),
                evt_tx_bg.clone(),
            ))
        } else {
            None
        };

        // generate loop 控制
        let mut gen_loop: Option<tokio::task::JoinHandle<()>> = None;

        // 2. 执行恢复逻辑 + 启动常驻 workers
        if let Some(ref service) = backtest_service {
            service.recover().await;
            service.start_workers();
        }

        // 2.1 周期性刷新 UI（Alpha 列表 + 回测统计）
        {
            let dbc = db_bg.clone();
            let txc = evt_tx_bg.clone();
            tokio::spawn(async move {
                loop {
                    refresh_ui(&dbc, &txc).await;
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            });
        }

        // 3. 初始化 FieldSyncService（仅命令触发，不在启动时自动同步）
        let field_sync_service = if let Some(ref sess) = session_bg {
            Some(Arc::new(FieldSyncService::new(
                sess.clone(),
                db_bg.clone(),
                evt_tx_bg.clone(),
            )))
        } else {
            None
        };
        let ctx_provider: Option<Arc<dyn GenerateContextProvider>> = session_bg
            .as_ref()
            .map(|sess| Arc::new(ApiContextProvider::new(sess.clone())) as _);

        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                AppCommand::Backtest { expr } => {
                    if let Some(ref service) = backtest_service {
                        let _ =
                            evt_tx_bg.send(AppEvent::Message(format!("收到回测请求: {}", expr)));
                        match service.add_job(&expr).await {
                            Ok(Some(id)) => {
                                let _ = evt_tx_bg.send(AppEvent::Message(format!(
                                    "已添加回测任务 [ID: {}]: {}",
                                    id, expr
                                )));
                            }
                            Ok(None) => {
                                let _ = evt_tx_bg.send(AppEvent::Message(format!(
                                    "回测任务已存在（跳过入队）: {}",
                                    expr
                                )));
                            }
                            Err(e) => {
                                let _ = evt_tx_bg
                                    .send(AppEvent::Error(format!("添加回测任务失败: {}", e)));
                            }
                        }
                    } else {
                        let _ = evt_tx_bg.send(AppEvent::Error(
                            "无法回测：未登录或 Session 无效".to_string(),
                        ));
                    }
                }
                AppCommand::GenerateStart {
                    model,
                    batch,
                    interval_sec,
                    region,
                    universe,
                    delay,
                    sample_size,
                    auto_backtest,
                } => {
                    // 如果已有生成任务在运行，先停止
                    if let Some(handle) = gen_loop.take() {
                        handle.abort();
                        let _ = evt_tx_bg.send(AppEvent::Message("停止之前的生成任务".to_string()));
                    }

                    if let (Some(sess), Some(ctx_provider)) =
                        (session_bg.as_ref(), ctx_provider.as_ref())
                    {
                        let provider = match AnyProvider::from_env() {
                            Ok(p) => p,
                            Err(_) => {
                                let _ = evt_tx_bg.send(AppEvent::Error(
                                    "无法生成：缺少 AI 供应商配置".to_string(),
                                ));
                                continue;
                            }
                        };

                        let generator = GeneratorService::new(
                            provider,
                            db_bg.clone(),
                            sess.clone(),
                            evt_tx_bg.clone(),
                            ctx_provider.clone(),
                        );

                        let config_clone = GenerateConfig {
                            batch_size: batch,
                            max_insert: batch,
                            model,
                            interval_sec,
                            region,
                            universe,
                            delay,
                            field_sample_size: sample_size,
                            auto_backtest,
                        };
                        let handle = tokio::spawn(async move {
                            generator.run_loop(config_clone).await;
                        });
                        gen_loop = Some(handle);
                        let _ = evt_tx_bg.send(AppEvent::Message("开始生成任务...".to_string()));
                    } else {
                        let _ = evt_tx_bg.send(AppEvent::Error("无法生成：未登录".to_string()));
                    }
                }
                AppCommand::GenerateOnce {
                    model,
                    batch,
                    region,
                    universe,
                    delay,
                    sample_size,
                    auto_backtest,
                } => {
                    if let (Some(sess), Some(ctx_provider)) =
                        (session_bg.as_ref(), ctx_provider.as_ref())
                    {
                        let provider = match AnyProvider::from_env() {
                            Ok(p) => p,
                            Err(_) => {
                                let _ = evt_tx_bg.send(AppEvent::Error(
                                    "无法生成：缺少 AI 供应商配置".to_string(),
                                ));
                                continue;
                            }
                        };

                        let generator = GeneratorService::new(
                            provider,
                            db_bg.clone(),
                            sess.clone(),
                            evt_tx_bg.clone(),
                            ctx_provider.clone(),
                        );

                        let config = GenerateConfig {
                            batch_size: batch,
                            max_insert: batch,
                            model,
                            interval_sec: 0,
                            region,
                            universe,
                            delay,
                            field_sample_size: sample_size,
                            auto_backtest,
                        };

                        tokio::spawn({
                            let tx = evt_tx_bg.clone();
                            async move {
                                let _ =
                                    tx.send(AppEvent::Message("开始单次生成任务...".to_string()));
                                match generator.generate_once(&config).await {
                                    Ok(res) => {
                                        let _ = tx.send(AppEvent::Log(format!(
                                            "单次生成完成: 候选 {}, 入库 {}, 拒绝 {}",
                                            res.candidates,
                                            res.inserted,
                                            res.rejected_examples.len()
                                        )));
                                    }
                                    Err(e) => {
                                        let _ = tx
                                            .send(AppEvent::Error(format!("单次生成出错: {}", e)));
                                    }
                                }
                            }
                        });
                    } else {
                        let _ = evt_tx_bg.send(AppEvent::Error("无法生成：未登录".to_string()));
                    }
                }
                AppCommand::GenerateStop => {
                    if let Some(handle) = gen_loop.take() {
                        handle.abort();
                        let _ = evt_tx_bg.send(AppEvent::Message("生成任务已停止".to_string()));
                    }
                }
                AppCommand::FieldsSync => {
                    if let Some(ref service) = field_sync_service {
                        if service.is_running() {
                            let _ = evt_tx_bg
                                .send(AppEvent::Message("已有字段同步任务进行中".to_string()));
                        } else {
                            let svc = service.clone();
                            tokio::spawn(async move {
                                let delays = vec![1, 3, 5, 10];
                                let _ = svc.sync_all_discovered(&delays).await;
                            });
                            let _ =
                                evt_tx_bg.send(AppEvent::Message("已触发字段同步任务".to_string()));
                        }
                    } else {
                        let _ = evt_tx_bg.send(AppEvent::Error("无法同步：未登录".to_string()));
                    }
                }
                AppCommand::FieldStats => {
                    match DataFieldRepository::stats_by_region_universe_delay(db_bg.as_ref()).await
                    {
                        Ok(rows) => {
                            let _ = evt_tx_bg.send(AppEvent::FieldStatsRows(rows));
                        }
                        Err(e) => {
                            let _ = evt_tx_bg.send(AppEvent::Error(format!("统计查询失败: {}", e)));
                        }
                    }
                }
                AppCommand::FieldSample {
                    region,
                    universe,
                    delay,
                    n,
                } => {
                    match DataFieldRepository::sample_weighted_fields(
                        db_bg.as_ref(),
                        region.clone(),
                        universe.clone(),
                        delay,
                        n,
                    )
                    .await
                    {
                        Ok(ids) => {
                            let count = ids.len();
                            let preview: Vec<String> = ids.iter().take(30).cloned().collect();
                            let mut line = String::new();
                            line.push_str("已抽样字段数量: ");
                            line.push_str(&count.to_string());
                            if !preview.is_empty() {
                                line.push_str("，前30个: ");
                                line.push_str(&preview.join(", "));
                            }
                            let _ = evt_tx_bg.send(AppEvent::Message(line));
                        }
                        Err(e) => {
                            let _ = evt_tx_bg.send(AppEvent::Error(format!("抽样失败: {}", e)));
                        }
                    }
                }
                AppCommand::GetDetail { expr } => {
                    match Alpha::find_by_id(expr.clone()).one(db_bg.as_ref()).await {
                        Ok(Some(model)) => {
                            let dto = AlphaDto::from(model);
                            let _ = evt_tx_bg.send(AppEvent::Detail(dto));
                        }
                        Ok(None) => {
                            let _ = evt_tx_bg.send(AppEvent::Error("未找到对应记录".to_string()));
                        }
                        Err(e) => {
                            let _ = evt_tx_bg.send(AppEvent::Error(format!("查询失败: {}", e)));
                        }
                    }
                }
                AppCommand::Catch { alpha_id } => {
                    if let Some(ref sess) = session_bg {
                        let dbc = db_bg.clone();
                        let txc = evt_tx_bg.clone();
                        let alpha = alpha_id.clone();
                        let sessc = sess.clone();
                        tokio::spawn(async move {
                            crate::commands::catch::run(&alpha, &sessc, &dbc, txc).await;
                        });
                    } else {
                        let _ = evt_tx_bg.send(AppEvent::Error("无法获取：未登录".to_string()));
                    }
                }
                AppCommand::Help => {
                    let _ = evt_tx_bg.send(AppEvent::Message("可用命令: backtest <expr> | fields sync | fields stats | fields sample [region] [universe] [delay] [n] | generate once <n> [model] [region] [universe] [delay] [sample_size] [auto_backtest] | generate loop <n> <sec> [model] [region] [universe] [delay] [sample_size] [auto_backtest] | generate stop | __INTERNAL_GET_DETAIL__ <expr>".to_string()));
                }
                AppCommand::Quit => {
                    let _ = evt_tx_bg.send(AppEvent::Message("收到退出命令".to_string()));
                }
                AppCommand::Unknown(msg) => {
                    let _ = evt_tx_bg.send(AppEvent::Error(format!("未知命令: {}", msg)));
                }
            }
        }
    });

    // TUI 初始化
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 创建 App 状态
    let mut app = App::new(info, cmd_tx, evt_rx);

    // 主循环
    let rx = app.evt_rx.take().unwrap();
    let res = run_app_loop(&mut terminal, &mut app, rx).await;

    // 恢复终端
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

/// 创建 WQB Session
async fn create_session(
    email: String,
    password: String,
    log_messages: &mut Vec<String>,
) -> Result<WQBSession, Box<dyn std::error::Error>> {
    log_messages.push("  正在初始化 Session...".to_string());
    let session = WQBSession::new(email.clone(), password);
    log_messages.push("  ✓ Session 对象已创建".to_string());

    // 测试连接
    log_messages.push("  正在测试认证连接...".to_string());
    match session.auth_request().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                log_messages.push(format!("  ✓ 认证成功！状态码: {}", status));

                // 尝试解析响应获取用户信息
                match resp.text().await {
                    Ok(text) => {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(user) = json.get("user") {
                                if let Some(user_id) = user.get("id") {
                                    log_messages.push(format!("  ✓ 用户 ID: {}", user_id));
                                }
                                if let Some(user_email) = user.get("email") {
                                    log_messages.push(format!("  ✓ 用户邮箱: {}", user_email));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log_messages.push(format!("  ⚠ 无法解析响应: {}", e));
                    }
                }
            } else {
                log_messages.push(format!("  ⚠ 认证响应状态码: {} (可能有问题)", status));
            }
        }
        Err(e) => {
            log_messages.push(format!("  ✗ 认证请求失败: {}", e));
            return Err(Box::new(e));
        }
    }

    log_messages.push("  ✓ Session 已就绪，可以使用".to_string());
    Ok(session)
}

async fn run_app_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    mut evt_rx: mpsc::UnboundedReceiver<AppEvent>,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;

        while let Ok(event) = evt_rx.try_recv() {
            match event {
                AppEvent::Log(msg) => app.log_messages.push(msg),
                AppEvent::Message(msg) => app.log_messages.push(msg),
                AppEvent::Error(msg) => app.log_messages.push(msg),
                AppEvent::Alphas(list) => {
                    app.alphas_all = list;
                    app.apply_filters();
                    app.clamp_selection();
                }
                AppEvent::Detail(dto) => {
                    app.selected_detail = Some(dto);
                }
                AppEvent::Stats(stats) => {
                    app.backtest_stats = stats;
                }
                AppEvent::FieldStatsRows(rows) => {
                    app.field_stats = rows;
                }
            }
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.handle_key_event(key.code) {
                        return Ok(());
                    }
                }
            }
        }
    }
}
