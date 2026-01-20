use crate::storage::entity::operator_event_compat::{
    self, ActiveModel as OperatorCompatActiveModel, Column as OperatorCompatColumn,
    Entity as OperatorCompat, Model as OperatorCompatModel,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::collections::HashSet;

pub struct OperatorCompatRepository;

impl OperatorCompatRepository {
    pub async fn list_incompatible_ops(
        db: &DatabaseConnection,
    ) -> Result<HashSet<String>, sea_orm::DbErr> {
        let rows: Vec<OperatorCompatModel> = OperatorCompat::find()
            .filter(OperatorCompatColumn::SupportsEvent.eq(false))
            .all(db)
            .await?;
        Ok(rows.into_iter().map(|m| m.operator_name).collect())
    }

    pub async fn mark_incompatible(
        db: &DatabaseConnection,
        operator_name: &str,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        if let Some(existing) = OperatorCompat::find()
            .filter(OperatorCompatColumn::OperatorName.eq(operator_name.to_string()))
            .one(db)
            .await?
        {
            let mut am: OperatorCompatActiveModel = existing.into();
            am.supports_event = Set(false);
            am.updated_at = Set(now);
            am.update(db).await?;
        } else {
            let am = OperatorCompatActiveModel {
                id: Set(0),
                operator_name: Set(operator_name.to_string()),
                supports_event: Set(false),
                created_at: Set(now),
                updated_at: Set(now),
            };
            let _ = am.insert(db).await?;
        }
        Ok(())
    }

    pub async fn mark_supported(
        db: &DatabaseConnection,
        operator_name: &str,
    ) -> Result<(), sea_orm::DbErr> {
        let now = Utc::now().timestamp();
        if let Some(existing) = OperatorCompat::find()
            .filter(OperatorCompatColumn::OperatorName.eq(operator_name.to_string()))
            .one(db)
            .await?
        {
            let mut am: OperatorCompatActiveModel = existing.into();
            am.supports_event = Set(true);
            am.updated_at = Set(now);
            am.update(db).await?;
        } else {
            let am = OperatorCompatActiveModel {
                id: Set(0),
                operator_name: Set(operator_name.to_string()),
                supports_event: Set(true),
                created_at: Set(now),
                updated_at: Set(now),
            };
            let _ = am.insert(db).await?;
        }
        Ok(())
    }
}
