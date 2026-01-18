use crate::session::WQBSession;
use crate::storage::repository::{AlphaDefinition, AlphaRepository, CoreMetrics};
use crate::AppEvent;
use log::error;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::sync::Arc;

use tokio::sync::mpsc;

pub async fn run(
    alpha_id: &str,
    session: &WQBSession,
    db: &Arc<DatabaseConnection>,
    evt_tx: mpsc::UnboundedSender<AppEvent>,
) {
    let _ = evt_tx.send(AppEvent::Log(format!(
        "正在获取 Alpha 信息: {}...",
        alpha_id
    )));

    match session.locate_alpha(alpha_id).await {
        Ok(resp) => {
            if !resp.status().is_success() {
                let err_msg = format!("✗ 获取失败: HTTP {}", resp.status());
                let _ = evt_tx.send(AppEvent::Log(err_msg));
                return;
            }

            match resp.json::<Value>().await {
                Ok(json) => {
                    if let Err(e) = save_to_db(db, &json).await {
                        let err_msg = format!("✗ 数据库保存失败: {}", e);
                        let _ = evt_tx.send(AppEvent::Log(err_msg));
                        error!("{}", e);
                    } else {
                        let _ = evt_tx.send(AppEvent::Log(format!(
                            "✓ Alpha {} 已成功存入数据库",
                            alpha_id
                        )));
                        // 注意：这里不再发送具体的 Refresh 事件，后台主循环会自动刷新
                    }
                }
                Err(e) => {
                    let _ = evt_tx.send(AppEvent::Log(format!("✗ JSON 解析失败: {}", e)));
                }
            }
        }
        Err(e) => {
            let _ = evt_tx.send(AppEvent::Log(format!("✗ 网络请求失败: {}", e)));
        }
    }
}

async fn save_to_db(
    db: &DatabaseConnection,
    json: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. 提取定义字段
    let expression = json["regular"]["code"]
        .as_str()
        .ok_or("Missing regular.code")?
        .to_string();

    let region = json["settings"]["region"]
        .as_str()
        .unwrap_or("USA")
        .to_string();
    let universe = json["settings"]["universe"]
        .as_str()
        .unwrap_or("TOP3000")
        .to_string();
    let language = json["settings"]["language"]
        .as_str()
        .unwrap_or("FASTEXPR")
        .to_string();
    let delay = json["settings"]["delay"].as_i64().unwrap_or(1) as i32;
    let decay = json["settings"]["decay"].as_i64().unwrap_or(0) as i32;
    let neutralization = json["settings"]["neutralization"]
        .as_str()
        .unwrap_or("NONE")
        .to_string();
    let operator_count = json["regular"]["operatorCount"].as_i64().unwrap_or(0) as i32;
    let _status = json["status"].as_str().unwrap_or("UNKNOWN").to_string();

    let def = AlphaDefinition {
        expression: expression.clone(),
        region,
        universe,
        language,
        delay,
        decay,
        neutralization,
        operator_count,
    };

    // 2. 插入或忽略定义
    AlphaRepository::insert_or_ignore_alpha(db, def).await?;

    // 3. 提取核心指标 (IS 阶段)
    let is = &json["is"];
    let core_metrics = CoreMetrics {
        is_sharpe: is["sharpe"].as_f64(),
        is_fitness: is["fitness"].as_f64(),
        is_turnover: is["turnover"].as_f64(),
        is_returns: is["returns"].as_f64(),
        is_drawdown: is["drawdown"].as_f64(),
        is_pnl: is["pnl"].as_f64(),
    };

    // 4. 构建 metrics_json (按要求支持多阶段多视角)
    // 提取 raw 指标 (IS)
    let mut raw_metrics = serde_json::Map::new();
    for field in &[
        "pnl",
        "bookSize",
        "longCount",
        "shortCount",
        "turnover",
        "returns",
        "drawdown",
        "margin",
        "sharpe",
        "fitness",
    ] {
        if let Some(val) = is[*field].as_f64() {
            raw_metrics.insert(field.to_string(), json!(val));
        } else if let Some(val) = is[*field].as_i64() {
            raw_metrics.insert(field.to_string(), json!(val));
        }
    }

    let metrics_json = json!({
        "IS": {
            "raw": raw_metrics,
            "riskNeutralized": is["riskNeutralized"],
            "investabilityConstrained": is["investabilityConstrained"],
        }
    });

    // 5. 提取 checks_json
    let checks_json = json!(is["checks"]);

    // 6. 更新状态和指标
    AlphaRepository::mark_done(
        db,
        &expression,
        Some(core_metrics),
        Some(metrics_json),
        Some(checks_json),
    )
    .await?;

    Ok(())
}
