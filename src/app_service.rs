use crate::app_state::{AlphaSummary, AppEvent};
use crate::storage::repository::{AlphaRepository, BacktestRepository};
use sea_orm::DatabaseConnection;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

fn checks_has_fail(v: &Value) -> bool {
    match v {
        Value::Array(checks) => checks.iter().any(|c| {
            c.get("result")
                .and_then(|x| x.as_str())
                .map(|s| s.eq_ignore_ascii_case("FAIL"))
                .unwrap_or(false)
        }),
        _ => false,
    }
}

pub async fn refresh_ui(db: &Arc<DatabaseConnection>, tx: &mpsc::UnboundedSender<AppEvent>) {
    // 1. 加载全部 Alpha 记录
    if let Ok(alphas) = AlphaRepository::load_all_by_status(db, "ALL").await {
        let list = alphas
            .into_iter()
            .map(|a| AlphaSummary {
                expression: a.expression,
                status: a.status,
                has_fail: checks_has_fail(&a.checks_json),
                is_sharpe: a.core_metrics.is_sharpe,
            })
            .collect();
        let _ = tx.send(AppEvent::Alphas(list));
    }

    // 2. 加载回测统计数据
    if let Ok(stats) = BacktestRepository::get_stats(db).await {
        let _ = tx.send(AppEvent::Stats(stats));
    }
}

pub async fn refresh_stats(db: &Arc<DatabaseConnection>, tx: &mpsc::UnboundedSender<AppEvent>) {
    if let Ok(stats) = BacktestRepository::get_stats(db).await {
        let _ = tx.send(AppEvent::Stats(stats));
    }
}
