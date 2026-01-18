use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "backtest_jobs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub alpha_id: Option<String>,
    pub expression: String,
    pub simulation_id: Option<String>,
    pub status: String, // QUEUED/CLAIMED/SUBMITTING/RUNNING/FETCHING/DONE/RETRY_WAIT/FAILED_PERMANENT
    pub priority: i32,
    pub retry_count: i32,
    pub max_retries: i32,
    pub next_run_at: i64,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<i64>,
    pub metrics_json: Option<String>,
    pub checks_json: Option<String>,
    pub last_error_kind: Option<String>, // RETRYABLE / PERMANENT / RETRY_EXCEEDED
    pub last_error_code: Option<String>, // HTTP_429 / TIMEOUT / INVALID_EXPRESSION ...
    pub last_error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub region: String,   // 新增：回测区域
    pub universe: String, // 新增：回测universe
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
