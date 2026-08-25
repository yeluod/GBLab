use std::path::Path;

use sqlx::{
    SqlitePool,
    migrate::MigrateError,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use thiserror::Error;

const SQLITE_MAX_CONNECTIONS: u32 = 4;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// `GBLab` 的 SQLite 连接池。
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// 打开 SQLite 文件并执行所有内嵌迁移。
    ///
    /// # Errors
    ///
    /// 连接 SQLite 或执行 schema migration 失败时返回 [`DatabaseError`]。
    pub async fn open(path: &Path) -> Result<Self, DatabaseError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(SQLITE_MAX_CONNECTIONS)
            .connect_with(options)
            .await?;

        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    /// 返回连接池是否至少建立了一个连接。
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.pool.size() > 0
    }
}

/// SQLite 初始化或迁移错误。
#[derive(Debug, Error)]
pub enum DatabaseError {
    /// SQLite 连接或查询失败。
    #[error("SQLite 操作失败: {0}")]
    Sqlx(#[from] sqlx::Error),
    /// 数据库 schema migration 失败。
    #[error("SQLite 迁移失败: {0}")]
    Migration(#[from] MigrateError),
}
