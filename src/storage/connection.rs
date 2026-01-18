use crate::storage::entity::alpha;
use log::info;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbErr, Schema};
use std::time::Duration;

pub async fn establish_connection(db_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(db_url.to_owned());
    opt.max_connections(10)
        .min_connections(2)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .max_lifetime(Duration::from_secs(8))
        .sqlx_logging(true)
        .sqlx_logging_level(log::LevelFilter::Info);

    let db = Database::connect(opt).await?;

    // 启用 WAL 模式
    let _ = sea_orm::ConnectionTrait::execute(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "PRAGMA journal_mode=WAL;".to_string(),
        ),
    )
    .await?;

    // 创建表（如果不存在）
    let builder = db.get_database_backend();
    let schema = Schema::new(builder);

    // Alphas table
    let stmt = builder.build(
        schema
            .create_table_from_entity(alpha::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    // Backtest Jobs table
    let stmt = builder.build(
        schema
            .create_table_from_entity(crate::storage::entity::backtest_job::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;
    ensure_backtest_jobs_columns(&db).await?;

    // Data Fields table
    let stmt = builder.build(
        schema
            .create_table_from_entity(crate::storage::entity::data_field::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    // Alpha-Field Relations table
    let stmt = builder.build(
        schema
            .create_table_from_entity(crate::storage::entity::alpha_field_relation::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    let stmt = builder.build(
        schema
            .create_table_from_entity(crate::storage::entity::data_field_scope::Entity)
            .if_not_exists(),
    );
    db.execute(stmt).await?;

    // 唯一索引：避免重复作用域映射
    let _ = sea_orm::ConnectionTrait::execute(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_data_field_scopes_unique ON data_field_scopes(field_id, region, universe, delay);".to_string(),
        ),
    )
    .await?;

    info!("Database connection established with WAL mode and table initialized.");

    Ok(db)
}

async fn ensure_backtest_jobs_columns(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    if backend != sea_orm::DatabaseBackend::Sqlite {
        return Ok(());
    }

    let rows = db
        .query_all(sea_orm::Statement::from_string(
            backend,
            "PRAGMA table_info(backtest_jobs);".to_string(),
        ))
        .await?;

    let mut cols = std::collections::HashSet::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "name") {
            cols.insert(name);
        }
    }

    if !cols.contains("region") {
        db.execute(sea_orm::Statement::from_string(
            backend,
            "ALTER TABLE backtest_jobs ADD COLUMN region TEXT NOT NULL DEFAULT 'CHN';".to_string(),
        ))
        .await?;
    }
    if !cols.contains("universe") {
        db.execute(sea_orm::Statement::from_string(
            backend,
            "ALTER TABLE backtest_jobs ADD COLUMN universe TEXT NOT NULL DEFAULT 'TOP2000U';"
                .to_string(),
        ))
        .await?;
    }

    Ok(())
}
