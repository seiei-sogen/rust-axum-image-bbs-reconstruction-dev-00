//! axum の State Extractor で共有する状態。

use std::path::PathBuf;

use sqlx::SqlitePool;

use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub upload_dir: PathBuf,
    pub config: AppConfig,
}
