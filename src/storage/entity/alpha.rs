use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "alphas")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
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

    // 核心 IS 指标
    #[sea_orm(nullable)]
    pub is_sharpe: Option<f64>,
    #[sea_orm(nullable)]
    pub is_fitness: Option<f64>,
    #[sea_orm(nullable)]
    pub is_turnover: Option<f64>,
    #[sea_orm(nullable)]
    pub is_returns: Option<f64>,
    #[sea_orm(nullable)]
    pub is_drawdown: Option<f64>,
    #[sea_orm(nullable)]
    pub is_pnl: Option<f64>,

    // JSON 字段
    pub metrics_json: String,
    pub checks_json: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
