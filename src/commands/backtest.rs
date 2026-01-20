use crate::storage::repository::{AlphaDefinition, AlphaRepository, BacktestRepository};
use crate::AppEvent;
use sea_orm::DatabaseConnection;
use tokio::sync::mpsc;

pub async fn run(
    expression: &str,
    db: &DatabaseConnection,
    evt_tx: mpsc::UnboundedSender<AppEvent>,
) {
    let sanitized = crate::generate::parser::sanitize_expression(expression);
    if let Err(reason) = crate::generate::parser::validate_prequeue(&sanitized) {
        let msg = match reason.as_str() {
            "unexpected_right_paren" => "预提交校验失败：存在意外右括号（形如 ...)(...）",
            "trailing_comma" => "预提交校验失败：存在拖尾逗号（形如 ...,)）",
            "winsorize_arity" => "预提交校验失败：winsorize 仅接受 1 个输入参数",
            _ => "预提交校验失败：表达式不符合入队规则",
        };
        let _ = evt_tx.send(AppEvent::Error(msg.to_string()));
        return;
    }
    if let Err(crate::storage::repository::data_field_repo::EventOpValidationErr::Incompatible) =
        crate::storage::repository::DataFieldRepository::validate_event_operator_compatibility(
            db,
            &sanitized,
            Some("CHN"),
            Some("TOP2000U"),
            Some(1),
        )
        .await
    {
        let _ = evt_tx.send(AppEvent::Error(
            "预提交校验失败：事件字段与不兼容运算符组合".to_string(),
        ));
        return;
    }
    let _ = evt_tx.send(AppEvent::Log(format!("正在提交回测任务: {}", sanitized)));

    // 1. 先在 alphas 主表中占位（使用默认回测设置）
    let def = AlphaDefinition {
        expression: sanitized.to_string(),
        region: "CHN".to_string(),
        universe: "TOP2000U".to_string(),
        language: "FASTEXPR".to_string(),
        delay: 1,
        decay: 10,
        neutralization: "INDUSTRY".to_string(),
        operator_count: 0,
    };

    if let Err(e) = AlphaRepository::insert_or_ignore_alpha(db, def).await {
        let _ = evt_tx.send(AppEvent::Log(format!("⚠ 无法创建 Alpha 记录: {}", e)));
    }

    // 2. 提交到后台任务队列
    match BacktestRepository::create_job(
        db,
        sanitized.to_string(),
        "CHN".to_string(),
        "TOP2000U".to_string(),
    )
    .await
    {
        Ok(Some(id)) => {
            let _ = evt_tx.send(AppEvent::Log(format!(
                "✓ 任务已入库 [ID: {}], 等待后台调度",
                id
            )));
        }
        Ok(None) => {
            let _ = evt_tx.send(AppEvent::Log(format!(
                "✓ 任务已存在（跳过入队）: {}",
                expression
            )));
        }
        Err(e) => {
            let _ = evt_tx.send(AppEvent::Log(format!("✗ 提交任务失败: {}", e)));
        }
    }
}
