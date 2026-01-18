use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "alpha_field_relations")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub alpha_expression: String, // 指向alpha表的expression字段
    pub field_id: String,         // 指向data_fields表的field_id字段
    pub region: String,           // 字段适用的区域
    pub universe: String,         // 字段适用的universe
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::alpha::Entity",
        from = "Column::AlphaExpression",
        to = "super::alpha::Column::Expression"
    )]
    Alpha,
    #[sea_orm(
        belongs_to = "super::data_field::Entity",
        from = "Column::FieldId",
        to = "super::data_field::Column::FieldId"
    )]
    DataField,
}

impl ActiveModelBehavior for ActiveModel {}

// impl Related<super::data_field::Entity> for Entity {
//     fn to() -> RelationDef {
//         super::data_field::Relation::AlphaFieldRelation.def()
//     }
// }
