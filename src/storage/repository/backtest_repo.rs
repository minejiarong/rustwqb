use crate::storage::entity::backtest_job::{
    self, ActiveModel as BacktestJobActiveModel, Entity as BacktestJob,
};
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use sea_orm::{ConnectionTrait, Statement};
use serde_json::Value;

pub struct BacktestRepository;

impl BacktestRepository {
    pub async fn create_job(
        db: &DatabaseConnection,
        expression: String,
        region: String,
        universe: String,
    ) -> Result<Option<i32>, sea_orm::DbErr> {
        let exists = BacktestJob::find()
            .filter(backtest_job::Column::Expression.eq(expression.clone()))
            .filter(backtest_job::Column::Status.is_in([
                "QUEUED",
                "RETRY_WAIT",
                "CLAIMED",
                "SUBMITTING",
                "RUNNING",
                "FETCHING",
            ]))
            .one(db)
            .await?;
        if exists.is_some() {
            return Ok(None);
        }

        let now = Utc::now().timestamp();
        let next_run_at = now;
        let active_model = BacktestJobActiveModel {
            alpha_id: Set(None),
            expression: Set(expression),
            status: Set("QUEUED".to_string()),
            priority: Set(0),
            retry_count: Set(0),
            max_retries: Set(5),
            next_run_at: Set(next_run_at),
            created_at: Set(now),
            updated_at: Set(now),
            region: Set(region),
            universe: Set(universe),
            ..Default::default()
        };

        let result = active_model.insert(db).await?;
        Ok(Some(result.id))
    }

    pub async fn update_status(
        db: &DatabaseConnection,
        id: i32,
        status: String,
        sim_id: Option<String>,
        alpha_id: Option<String>,
        error_message: Option<String>,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        let mut update = BacktestJobActiveModel {
            id: Set(id),
            status: Set(status),
            updated_at: Set(now),
            ..Default::default()
        };

        if let Some(s) = sim_id {
            update.simulation_id = Set(Some(s));
        }
        if let Some(a) = alpha_id {
            update.alpha_id = Set(Some(a));
        }
        if let Some(m) = error_message {
            update.last_error_message = Set(Some(m));
        }

        update.update(db).await?;
        Ok(())
    }

    pub async fn get_pending_jobs(
        db: &DatabaseConnection,
        limit: u64,
    ) -> Result<Vec<backtest_job::Model>, sea_orm::DbErr> {
        BacktestJob::find()
            .filter(backtest_job::Column::Status.eq("QUEUED"))
            .limit(limit)
            .all(db)
            .await
    }

    /// 原子性 claim 下一条可执行任务（SQLite: BEGIN IMMEDIATE）
    /// 规则：
    /// - status in (QUEUED, RETRY_WAIT)
    /// - next_run_at <= now
    /// - priority DESC, created_at ASC
    pub async fn claim_next(
        db: &DatabaseConnection,
        worker_id: &str,
        now: i64,
    ) -> Result<Option<backtest_job::Model>, sea_orm::DbErr> {
        // 关键修复：
        // 不要在连接池上手写 BEGIN IMMEDIATE/COMMIT（并发时容易“transaction within a transaction”）。
        // 使用 SeaORM 事务，确保 select+update 在同一连接上完成。
        let txn = db.begin().await?;

        let picked = BacktestJob::find()
            .filter(
                backtest_job::Column::Status
                    .eq("QUEUED")
                    .or(backtest_job::Column::Status.eq("RETRY_WAIT")),
            )
            .filter(backtest_job::Column::NextRunAt.lte(now))
            .order_by_desc(backtest_job::Column::Priority)
            .order_by_asc(backtest_job::Column::CreatedAt)
            .one(&txn)
            .await?;

        if let Some(job) = picked {
            let job_id = job.id;
            let now2 = Utc::now().timestamp();
            BacktestJob::update_many()
                .col_expr(backtest_job::Column::Status, Expr::value("CLAIMED"))
                .col_expr(
                    backtest_job::Column::ClaimedBy,
                    Expr::value(worker_id.to_string()),
                )
                .col_expr(backtest_job::Column::ClaimedAt, Expr::value(now2))
                .col_expr(backtest_job::Column::UpdatedAt, Expr::value(now2))
                .filter(backtest_job::Column::Id.eq(job_id))
                .exec(&txn)
                .await?;

            txn.commit().await?;
            return Ok(BacktestJob::find_by_id(job_id).one(db).await?);
        }

        txn.commit().await?;
        Ok(None)
    }

    pub async fn mark_status(
        db: &DatabaseConnection,
        id: i32,
        status: &str,
        simulation_id: Option<String>,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        let mut update = BacktestJobActiveModel {
            id: Set(id),
            status: Set(status.to_string()),
            updated_at: Set(now),
            ..Default::default()
        };
        if let Some(s) = simulation_id {
            update.simulation_id = Set(Some(s));
        }
        update.update(db).await?;
        Ok(())
    }

    pub async fn mark_done(
        db: &DatabaseConnection,
        id: i32,
        simulation_id: Option<String>,
        alpha_id: Option<String>,
        metrics_json: Option<Value>,
        checks_json: Option<Value>,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        let mut update = BacktestJobActiveModel {
            id: Set(id),
            status: Set("DONE".to_string()),
            updated_at: Set(now),
            ..Default::default()
        };
        if let Some(s) = simulation_id {
            update.simulation_id = Set(Some(s));
        }
        if let Some(a) = alpha_id {
            update.alpha_id = Set(Some(a));
        }
        if let Some(m) = metrics_json {
            update.metrics_json = Set(Some(m.to_string()));
        }
        if let Some(c) = checks_json {
            update.checks_json = Set(Some(c.to_string()));
        }
        update.update(db).await?;
        Ok(())
    }

    pub async fn mark_failed_retryable(
        db: &DatabaseConnection,
        id: i32,
        kind: &str,
        code: Option<String>,
        message: Option<String>,
        next_run_at: i64,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        BacktestJob::update_many()
            .col_expr(backtest_job::Column::Status, Expr::value("RETRY_WAIT"))
            .col_expr(
                backtest_job::Column::RetryCount,
                Expr::col(backtest_job::Column::RetryCount).add(1),
            )
            .col_expr(backtest_job::Column::NextRunAt, Expr::value(next_run_at))
            .col_expr(
                backtest_job::Column::LastErrorKind,
                Expr::value(kind.to_string()),
            )
            .col_expr(
                backtest_job::Column::LastErrorCode,
                Expr::value(code.unwrap_or_default()),
            )
            .col_expr(
                backtest_job::Column::LastErrorMessage,
                Expr::value(message.unwrap_or_default()),
            )
            .col_expr(backtest_job::Column::UpdatedAt, Expr::value(now))
            .filter(backtest_job::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }

    pub async fn mark_failed_permanent(
        db: &DatabaseConnection,
        id: i32,
        kind: &str,
        code: Option<String>,
        message: Option<String>,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        BacktestJob::update_many()
            .col_expr(
                backtest_job::Column::Status,
                Expr::value("FAILED_PERMANENT"),
            )
            .col_expr(
                backtest_job::Column::LastErrorKind,
                Expr::value(kind.to_string()),
            )
            .col_expr(
                backtest_job::Column::LastErrorCode,
                Expr::value(code.unwrap_or_default()),
            )
            .col_expr(
                backtest_job::Column::LastErrorMessage,
                Expr::value(message.unwrap_or_default()),
            )
            .col_expr(backtest_job::Column::UpdatedAt, Expr::value(now))
            .filter(backtest_job::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }

    pub async fn get_running_jobs(
        db: &DatabaseConnection,
    ) -> Result<Vec<backtest_job::Model>, sea_orm::DbErr> {
        BacktestJob::find()
            .filter(backtest_job::Column::Status.eq("POLLING"))
            .all(db)
            .await
    }

    /// 将所有中间状态的任务重置为 PENDING
    pub async fn reset_stale_jobs(db: &DatabaseConnection) -> Result<u64, sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        let res = BacktestJob::update_many()
            .col_expr(
                backtest_job::Column::Status,
                sea_orm::sea_query::Expr::value("QUEUED"),
            )
            .col_expr(
                backtest_job::Column::NextRunAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .col_expr(
                backtest_job::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(
                backtest_job::Column::Status
                    .eq("RUNNING")
                    .or(backtest_job::Column::Status.eq("FETCHING"))
                    .or(backtest_job::Column::Status.eq("SUBMITTING"))
                    .or(backtest_job::Column::Status.eq("CLAIMED")),
            )
            .exec(db)
            .await?;
        Ok(res.rows_affected)
    }

    /// 增加重试计数并重置为 PENDING
    pub async fn increment_retry(db: &DatabaseConnection, id: i32) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        // 这里的逻辑稍微复杂，我们需要先读再写，或者使用表达式更新
        // SeaORM 允许使用表达式更新
        BacktestJob::update_many()
            .col_expr(
                backtest_job::Column::RetryCount,
                sea_orm::sea_query::Expr::col(backtest_job::Column::RetryCount).add(1),
            )
            .col_expr(
                backtest_job::Column::Status,
                sea_orm::sea_query::Expr::value("QUEUED"),
            )
            .col_expr(
                backtest_job::Column::NextRunAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .col_expr(
                backtest_job::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(backtest_job::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }

    pub async fn get_stats(
        db: &DatabaseConnection,
    ) -> Result<crate::backtest::model::BacktestStats, sea_orm::DbErr> {
        use sea_orm::PaginatorTrait;

        let total = BacktestJob::find().count(db).await? as usize;
        let pending = BacktestJob::find()
            .filter(backtest_job::Column::Status.eq("QUEUED"))
            .count(db)
            .await? as usize;
        let running = BacktestJob::find()
            .filter(
                backtest_job::Column::Status
                    .eq("RUNNING")
                    .or(backtest_job::Column::Status.eq("SUBMITTING"))
                    .or(backtest_job::Column::Status.eq("FETCHING"))
                    .or(backtest_job::Column::Status.eq("CLAIMED")),
            )
            .count(db)
            .await? as usize;
        let completed = BacktestJob::find()
            .filter(backtest_job::Column::Status.eq("DONE"))
            .count(db)
            .await? as usize;
        let error_retryable = BacktestJob::find()
            .filter(backtest_job::Column::Status.eq("RETRY_WAIT"))
            .count(db)
            .await? as usize;
        let error_fatal = BacktestJob::find()
            .filter(backtest_job::Column::Status.eq("FAILED_PERMANENT"))
            .count(db)
            .await? as usize;
        let error_exceeded = BacktestJob::find()
            .filter(backtest_job::Column::LastErrorKind.eq("RETRY_EXCEEDED"))
            .count(db)
            .await? as usize;

        Ok(crate::backtest::model::BacktestStats {
            total,
            pending,
            running,
            completed,
            error_retryable,
            error_fatal,
            error_exceeded,
        })
    }
}
