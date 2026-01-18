use crate::generate::context::FieldEntry;
use crate::storage::entity::data_field::{
    ActiveModel as DataFieldActiveModel, Column as DataFieldColumn, Entity as DataField,
    Model as DataFieldModel,
};
use crate::storage::entity::data_field_scope::{
    ActiveModel as DataFieldScopeActiveModel, Column as DataFieldScopeColumn,
    Entity as DataFieldScope, Model as DataFieldScopeModel,
};
use chrono::Utc;
use rand::Rng;
use sea_orm::sea_query::Expr;
use sea_orm::NotSet;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter,
    QuerySelect, Set,
};
use std::collections::{HashMap, HashSet};

pub struct DataFieldRepository;

#[derive(Debug, Clone, FromQueryResult)]
pub struct FieldStatsRow {
    pub region: String,
    pub universe: String,
    pub delay: i32,
    pub count: i64,
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct FieldFreqRow {
    pub field_id: String,
    pub freq: i64,
}

impl DataFieldRepository {
    pub async fn upsert_batch(
        db: &DatabaseConnection,
        entries: Vec<FieldEntry>,
    ) -> Result<(usize, usize), sea_orm::DbErr> {
        if entries.is_empty() {
            return Ok((0, 0));
        }

        let ids: Vec<String> = entries.iter().map(|e| e.field_id.clone()).collect();

        let existing: Vec<DataFieldModel> = DataField::find()
            .filter(DataFieldColumn::FieldId.is_in(ids.clone()))
            .all(db)
            .await?;
        let existing_set: HashSet<String> = existing.into_iter().map(|m| m.field_id).collect();

        let now = Utc::now().timestamp();

        let mut to_insert = Vec::new();
        let mut to_update = Vec::new();

        for e in entries {
            if existing_set.contains(&e.field_id) {
                to_update.push(e);
            } else {
                let m = DataFieldActiveModel {
                    field_id: Set(e.field_id),
                    description: Set(e.description),
                    dataset_id: Set(e.dataset_id),
                    dataset_name: Set(e.dataset_name),
                    category_id: Set(e.category_id),
                    category_name: Set(e.category_name),
                    subcategory_id: Set(e.subcategory_id),
                    subcategory_name: Set(e.subcategory_name),
                    region: Set(e.region),
                    delay: Set(e.delay),
                    universe: Set(e.universe),
                    field_type: Set(e.field_type),
                    date_coverage: Set(0.0),
                    coverage: Set(0.0),
                    user_count: Set(0),
                    alpha_count: Set(0),
                    pyramid_multiplier: Set(0.0),
                    themes: Set("[]".to_string()),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                to_insert.push(m);
            }
        }

        let insert_count = to_insert.len();
        if !to_insert.is_empty() {
            DataField::insert_many(to_insert).exec(db).await?;
        }

        let mut updated = 0usize;
        for e in to_update {
            if let Some(model) = DataField::find_by_id(e.field_id.clone()).one(db).await? {
                let mut am: DataFieldActiveModel = model.into();
                am.description = Set(e.description);
                am.dataset_id = Set(e.dataset_id);
                am.dataset_name = Set(e.dataset_name);
                am.category_id = Set(e.category_id);
                am.category_name = Set(e.category_name);
                am.subcategory_id = Set(e.subcategory_id);
                am.subcategory_name = Set(e.subcategory_name);
                am.region = Set(e.region);
                am.delay = Set(e.delay);
                am.universe = Set(e.universe);
                am.field_type = Set(e.field_type);
                am.updated_at = Set(now);
                am.update(db).await?;
                updated += 1;
            }
        }

        Ok((insert_count, updated))
    }

    pub async fn stats_by_region_universe_delay(
        db: &DatabaseConnection,
    ) -> Result<Vec<FieldStatsRow>, sea_orm::DbErr> {
        DataFieldScope::find()
            .select_only()
            .column(DataFieldScopeColumn::Region)
            .column(DataFieldScopeColumn::Universe)
            .column(DataFieldScopeColumn::Delay)
            .column_as(Expr::cust("COUNT(DISTINCT field_id)"), "count")
            .group_by(DataFieldScopeColumn::Region)
            .group_by(DataFieldScopeColumn::Universe)
            .group_by(DataFieldScopeColumn::Delay)
            .into_model::<FieldStatsRow>()
            .all(db)
            .await
    }

    pub async fn upsert_scopes(
        db: &DatabaseConnection,
        entries: &[FieldEntry],
    ) -> Result<usize, sea_orm::DbErr> {
        let mut inserted = 0usize;
        let now = Utc::now().timestamp();
        let mut seen: HashSet<(String, String, String, i32)> = HashSet::new();
        for e in entries {
            let key = (
                e.field_id.clone(),
                e.region.clone(),
                e.universe.clone(),
                e.delay,
            );
            if !seen.insert(key.clone()) {
                continue;
            }
            let exists = DataFieldScope::find()
                .filter(DataFieldScopeColumn::FieldId.eq(e.field_id.clone()))
                .filter(DataFieldScopeColumn::Region.eq(e.region.clone()))
                .filter(DataFieldScopeColumn::Universe.eq(e.universe.clone()))
                .filter(DataFieldScopeColumn::Delay.eq(e.delay))
                .one(db)
                .await?;
            if exists.is_none() {
                let am = DataFieldScopeActiveModel {
                    id: NotSet,
                    field_id: Set(e.field_id.clone()),
                    region: Set(e.region.clone()),
                    universe: Set(e.universe.clone()),
                    delay: Set(e.delay),
                    created_at: Set(now),
                    updated_at: Set(now),
                };
                let _ = am.insert(db).await?;
                inserted += 1;
            }
        }
        Ok(inserted)
    }

    pub async fn sample_weighted_fields(
        db: &DatabaseConnection,
        region: Option<String>,
        universe: Option<String>,
        delay: Option<i32>,
        n: usize,
    ) -> Result<Vec<String>, sea_orm::DbErr> {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

        let mut query = DataFieldScope::find()
            .select_only()
            .column(DataFieldScopeColumn::FieldId)
            .column_as(Expr::cust("COUNT(*)"), "freq")
            .group_by(DataFieldScopeColumn::FieldId);

        if let Some(r) = region.as_ref() {
            query = query.filter(DataFieldScopeColumn::Region.eq(r.clone()));
        }
        if let Some(u) = universe.as_ref() {
            query = query.filter(DataFieldScopeColumn::Universe.eq(u.clone()));
        }
        if let Some(d) = delay {
            query = query.filter(DataFieldScopeColumn::Delay.eq(d));
        }

        let rows = query.into_model::<FieldFreqRow>().all(db).await?;

        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let mut rng = rand::thread_rng();
        let mut keys: Vec<(f64, String)> = rows
            .into_iter()
            .map(|row| {
                let w = 1.0f64 / (row.freq as f64);
                let u: f64 = rng.gen::<f64>();
                let k = u.powf(1.0 / w);
                (k, row.field_id)
            })
            .collect();

        keys.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let take = n.min(keys.len());
        Ok(keys.into_iter().take(take).map(|(_, id)| id).collect())
    }

    pub async fn sample_weighted_fields_grouped(
        db: &DatabaseConnection,
        region: Option<String>,
        universe: Option<String>,
        delay: Option<i32>,
        n: usize,
    ) -> Result<(Vec<String>, Vec<String>), sea_orm::DbErr> {
        let ids = Self::sample_weighted_fields(db, region, universe, delay, n).await?;
        if ids.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let models: Vec<DataFieldModel> = DataField::find()
            .filter(DataFieldColumn::FieldId.is_in(ids.clone()))
            .all(db)
            .await?;

        let mut by_id: HashMap<String, DataFieldModel> = HashMap::with_capacity(models.len());
        for m in models {
            by_id.insert(m.field_id.clone(), m);
        }

        let mut normal = Vec::new();
        let mut event = Vec::new();

        for id in ids {
            let is_event = by_id.get(&id).map(is_event_field).unwrap_or(false);
            if is_event {
                event.push(id);
            } else {
                normal.push(id);
            }
        }

        Ok((normal, event))
    }
}

fn is_event_field(m: &DataFieldModel) -> bool {
    let mut s = String::new();
    s.push_str(&m.dataset_name);
    s.push(' ');
    s.push_str(&m.category_name);
    s.push(' ');
    s.push_str(&m.subcategory_name);
    let hay = s.to_ascii_lowercase();
    hay.contains("news") || hay.contains("event")
}
