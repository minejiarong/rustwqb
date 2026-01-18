use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "data_fields")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub field_id: String,
    pub description: String,
    pub dataset_id: String,
    pub dataset_name: String,
    pub category_id: String,
    pub category_name: String,
    pub subcategory_id: String,
    pub subcategory_name: String,
    pub region: String,
    pub delay: i32,
    pub universe: String,
    pub field_type: String, // VECTOR or MATRIX
    pub date_coverage: f64,
    pub coverage: f64,
    pub user_count: i32,
    pub alpha_count: i32,
    pub pyramid_multiplier: f64,
    pub themes: String, // JSON array
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    // #[sea_orm(has_many = "super::alpha_field_relation::Entity")]
    // AlphaFieldRelation,
}

impl ActiveModelBehavior for ActiveModel {}
