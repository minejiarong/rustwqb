use crate::storage::entity::alpha::{
    self, ActiveModel as AlphaActiveModel, Entity as Alpha, Model as AlphaModel,
};
use crate::storage::entity::alpha_field_relation::Entity as AlphaFieldRelation;
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AlphaDefinition {
    pub expression: String,
    pub region: String,
    pub universe: String,
    pub language: String,
    pub delay: i32,
    pub decay: i32,
    pub neutralization: String,
    pub operator_count: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CoreMetrics {
    pub is_sharpe: Option<f64>,
    pub is_fitness: Option<f64>,
    pub is_turnover: Option<f64>,
    pub is_returns: Option<f64>,
    pub is_drawdown: Option<f64>,
    pub is_pnl: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AlphaDto {
    pub expression: String,
    pub region: String,
    pub universe: String,
    pub language: String,
    pub delay: i32,
    pub decay: i32,
    pub neutralization: String,
    pub operator_count: i32,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub core_metrics: CoreMetrics,
    pub metrics_json: Value,
    pub checks_json: Value,
}

impl From<AlphaModel> for AlphaDto {
    fn from(model: AlphaModel) -> Self {
        Self {
            expression: model.expression,
            region: model.region,
            universe: model.universe,
            language: model.language,
            delay: model.delay,
            decay: model.decay,
            neutralization: model.neutralization,
            operator_count: model.operator_count,
            status: model.status,
            created_at: model.created_at,
            updated_at: model.updated_at,
            core_metrics: CoreMetrics {
                is_sharpe: model.is_sharpe,
                is_fitness: model.is_fitness,
                is_turnover: model.is_turnover,
                is_returns: model.is_returns,
                is_drawdown: model.is_drawdown,
                is_pnl: model.is_pnl,
            },
            metrics_json: serde_json::from_str(&model.metrics_json)
                .unwrap_or(Value::Object(Default::default())),
            checks_json: serde_json::from_str(&model.checks_json)
                .unwrap_or(Value::Array(Default::default())),
        }
    }
}

pub struct AlphaRepository;

impl AlphaRepository {
    pub async fn insert_or_ignore_alpha(
        db: &DatabaseConnection,
        def: AlphaDefinition,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        let active_model = AlphaActiveModel {
            expression: Set(def.expression),
            region: Set(def.region),
            universe: Set(def.universe),
            language: Set(def.language),
            delay: Set(def.delay),
            decay: Set(def.decay),
            neutralization: Set(def.neutralization),
            operator_count: Set(def.operator_count),
            status: Set("PENDING".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            metrics_json: Set("{}".to_string()),
            checks_json: Set("[]".to_string()),
            ..Default::default()
        };

        // SQLite "INSERT OR IGNORE" isn't directly exposed as a single method in SeaORM for all backends easily,
        // but we can use on_conflict in some versions or just try and ignore error.
        // For SeaORM 1.0, we can use on_conflict.
        Alpha::insert(active_model)
            .on_conflict(
                sea_orm::sea_query::OnConflict::column(alpha::Column::Expression)
                    .do_nothing()
                    .to_owned(),
            )
            .exec(db)
            .await?;

        Ok(())
    }

    pub async fn delete_all(db: &DatabaseConnection) -> Result<u64, sea_orm::DbErr> {
        let res = Alpha::delete_many().exec(db).await?;
        Ok(res.rows_affected)
    }

    pub async fn delete_all_relations(db: &DatabaseConnection) -> Result<u64, sea_orm::DbErr> {
        let res = AlphaFieldRelation::delete_many().exec(db).await?;
        Ok(res.rows_affected)
    }

    pub async fn wipe_all(db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
        let _ = Self::delete_all_relations(db).await?;
        let _ = Self::delete_all(db).await?;
        Ok(())
    }

    pub async fn insert_batch(
        db: &DatabaseConnection,
        defs: Vec<AlphaDefinition>,
    ) -> Result<(), sea_orm::DbErr> {
        if defs.is_empty() {
            return Ok(());
        }
        let now = Utc::now().timestamp();
        let models: Vec<AlphaActiveModel> = defs
            .into_iter()
            .map(|def| AlphaActiveModel {
                expression: Set(def.expression),
                region: Set(def.region),
                universe: Set(def.universe),
                language: Set(def.language),
                delay: Set(def.delay),
                decay: Set(def.decay),
                neutralization: Set(def.neutralization),
                operator_count: Set(def.operator_count),
                status: Set("PENDING".to_string()),
                created_at: Set(now),
                updated_at: Set(now),
                metrics_json: Set("{}".to_string()),
                checks_json: Set("[]".to_string()),
                ..Default::default()
            })
            .collect();

        Alpha::insert_many(models)
            .on_conflict(
                sea_orm::sea_query::OnConflict::column(alpha::Column::Expression)
                    .do_nothing()
                    .to_owned(),
            )
            .exec(db)
            .await?;

        Ok(())
    }

    pub async fn load_by_status(
        db: &DatabaseConnection,
        status: &str,
        limit: u64,
    ) -> Result<Vec<AlphaDto>, sea_orm::DbErr> {
        let mut query = Alpha::find();

        // 如果不是 "ALL"，则按状态过滤
        if status != "ALL" {
            query = query.filter(alpha::Column::Status.eq(status));
            let models = query
                .order_by_desc(alpha::Column::UpdatedAt)
                .limit(limit)
                .all(db)
                .await?;
            return Ok(models.into_iter().map(AlphaDto::from).collect());
        }

        // "ALL" 视图优先展示已完成/错误，再按更新时间降序
        let models = query
            .order_by_asc(alpha::Column::Status)
            .order_by_desc(alpha::Column::UpdatedAt)
            .limit(limit)
            .all(db)
            .await?;

        Ok(models.into_iter().map(AlphaDto::from).collect())
    }

    pub async fn load_all_by_status(
        db: &DatabaseConnection,
        status: &str,
    ) -> Result<Vec<AlphaDto>, sea_orm::DbErr> {
        let mut query = Alpha::find();

        if status != "ALL" {
            query = query.filter(alpha::Column::Status.eq(status));
            let models = query
                .order_by_desc(alpha::Column::UpdatedAt)
                .all(db)
                .await?;
            return Ok(models.into_iter().map(AlphaDto::from).collect());
        }

        let models = query
            .order_by_asc(alpha::Column::Status)
            .order_by_desc(alpha::Column::UpdatedAt)
            .all(db)
            .await?;

        Ok(models.into_iter().map(AlphaDto::from).collect())
    }

    pub async fn mark_simulating(
        db: &DatabaseConnection,
        expression: &str,
        _worker_id: &str, // 可以在 status 中体现，或者以后加字段，目前按要求仅更新 status
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        Alpha::update_many()
            .col_expr(alpha::Column::Status, Expr::value("SIMULATING"))
            .col_expr(alpha::Column::UpdatedAt, Expr::value(now))
            .filter(alpha::Column::Expression.eq(expression))
            .exec(db)
            .await?;
        Ok(())
    }

    pub async fn mark_done(
        db: &DatabaseConnection,
        expression: &str,
        core_metrics: Option<CoreMetrics>,
        metrics_json: Option<Value>,
        checks_json: Option<Value>,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();

        // 需加载旧数据以进行 JSON merge
        let model = Alpha::find_by_id(expression).one(db).await?;
        if let Some(model) = model {
            let mut active_model: AlphaActiveModel = model.clone().into();
            active_model.status = Set("DONE".to_string());
            active_model.updated_at = Set(now);

            if let Some(core) = core_metrics {
                if let Some(v) = core.is_sharpe {
                    active_model.is_sharpe = Set(Some(v));
                }
                if let Some(v) = core.is_fitness {
                    active_model.is_fitness = Set(Some(v));
                }
                if let Some(v) = core.is_turnover {
                    active_model.is_turnover = Set(Some(v));
                }
                if let Some(v) = core.is_returns {
                    active_model.is_returns = Set(Some(v));
                }
                if let Some(v) = core.is_drawdown {
                    active_model.is_drawdown = Set(Some(v));
                }
                if let Some(v) = core.is_pnl {
                    active_model.is_pnl = Set(Some(v));
                }
            }

            if let Some(new_metrics) = metrics_json {
                let mut old_metrics: Value = serde_json::from_str(&model.metrics_json)
                    .unwrap_or(Value::Object(Default::default()));
                merge_json(&mut old_metrics, &new_metrics);
                active_model.metrics_json = Set(old_metrics.to_string());
            }

            if let Some(new_checks) = checks_json {
                // 对于 checks_json，通常是覆盖或者合并数组，按要求“更新时必须做 JSON merge”
                // 但 checks 是数组，merge 规则可能不同。用户示例是数组。
                // 如果是对象则 merge，如果是数组则可能需要特殊处理。
                // 简单起见，如果都是对象则递归 merge，否则覆盖。
                let mut old_checks: Value = serde_json::from_str(&model.checks_json)
                    .unwrap_or(Value::Array(Default::default()));
                merge_json(&mut old_checks, &new_checks);
                active_model.checks_json = Set(old_checks.to_string());
            }

            active_model.update(db).await?;
        }

        Ok(())
    }

    pub async fn mark_error(
        db: &DatabaseConnection,
        expression: &str,
        _error_message: &str,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        // 可以把 error_message 存入某个字段，目前表结构没给 error 字段，暂存 status 或 log 吧
        // 不过用户没给 error 字段，我们只更新状态。
        Alpha::update_many()
            .col_expr(alpha::Column::Status, Expr::value("ERROR"))
            .col_expr(alpha::Column::UpdatedAt, Expr::value(now))
            .filter(alpha::Column::Expression.eq(expression))
            .exec(db)
            .await?;
        Ok(())
    }

    pub async fn reset_stale_simulating(
        db: &DatabaseConnection,
        timeout_secs: i64,
    ) -> Result<u64, sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        let threshold = now - timeout_secs;

        let result = Alpha::update_many()
            .col_expr(alpha::Column::Status, Expr::value("PENDING"))
            .col_expr(alpha::Column::UpdatedAt, Expr::value(now))
            .filter(alpha::Column::Status.eq("SIMULATING"))
            .filter(alpha::Column::UpdatedAt.lt(threshold))
            .exec(db)
            .await?;

        Ok(result.rows_affected)
    }

    pub async fn status_counts(
        db: &DatabaseConnection,
    ) -> Result<HashMap<String, u64>, sea_orm::DbErr> {
        use sea_orm::{query::*, sea_query::*};

        let res = Alpha::find()
            .select_only()
            .column(alpha::Column::Status)
            .column_as(alpha::Column::Status.count(), "count")
            .group_by(alpha::Column::Status)
            .into_tuple::<(String, i64)>()
            .all(db)
            .await?;

        Ok(res.into_iter().map(|(s, c)| (s, c as u64)).collect())
    }
}

fn merge_json(a: &mut Value, b: &Value) {
    // 迭代式合并：使用路径队列，避免深递归导致的栈溢出
    let mut queue: Vec<(Vec<String>, Value)> = Vec::new();
    queue.push((Vec::new(), b.clone()));

    while let Some((path, v)) = queue.pop() {
        // 定位/创建目标节点
        let mut tgt: &mut Value = a;
        for key in &path {
            if !tgt.is_object() {
                *tgt = Value::Object(serde_json::Map::new());
            }
            let obj = tgt.as_object_mut().expect("object after init");
            tgt = obj.entry(key.clone()).or_insert(Value::Null);
        }

        if tgt.is_object() && v.is_object() {
            // 对象-对象：展开子键入队处理
            let bobj = v.as_object().unwrap();
            for (k, bv) in bobj.iter() {
                let mut sub = path.clone();
                sub.push(k.clone());
                queue.push((sub, bv.clone()));
            }
        } else {
            // 其它类型：直接覆盖
            *tgt = v;
        }
    }
}
