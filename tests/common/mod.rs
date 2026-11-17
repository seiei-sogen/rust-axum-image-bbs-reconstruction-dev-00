//! 統合テスト（tests/*.rs）で共有するセットアップヘルパ。
//!
//! 学習メモ:
//! - `tests/` 直下の `.rs` ファイルは、それぞれが独立したテスト用クレートとしてビルドされる。
//!   `tests/common/mod.rs` はサブディレクトリにあるのでテストクレート扱いされず、
//!   各テストファイルから `mod common;` で取り込んで共有できる。
//! - 取り込んだテストファイルによっては使わない関数があり「未使用」警告が出るため、
//!   このファイルに限り `#[allow(dead_code)]` を付けてよいことにしている。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use rust_axum_image_bbs::{build_app, config::AppConfig, state::AppState};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::ServiceExt; // `oneshot` メソッドを使えるようにするトレイト

/// テスト1件ぶんのアプリ一式。
pub struct TestApp {
    /// `build_app` で組み立てた本物のルーター。
    pub router: Router,
    /// ルーターと同じ in-memory DB を指すプール。テストデータの投入や検証に使う。
    pub db: SqlitePool,
    /// UPLOAD_DIR として渡した一時ディレクトリ。
    /// `TempDir` は drop されるとディレクトリごと消えるので、テスト中は所有して生かしておく。
    #[allow(dead_code)]
    pub upload_dir: TempDir,
}

/// まっさらな in-memory DB と一時ディレクトリでアプリを組み立てる。
///
/// 学習メモ:
/// - in-memory SQLite は接続が全部閉じると中身が消える。`max_connections(1)` で接続を1本に固定し、
///   マイグレーション・テストデータ投入・ハンドラが必ず同じ DB を見るようにする。
/// - `db.clone()` はプールの複製だが、中身は参照カウント付きの共有なので同じ接続を指す。
pub async fn setup() -> TestApp {
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite に接続できること");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("マイグレーションが適用できること");

    let upload_dir = TempDir::new().expect("一時ディレクトリを作れること");
    let state = AppState {
        db: db.clone(),
        upload_dir: upload_dir.path().to_path_buf(),
        config: AppConfig::from_env_or_default(),
    };

    TestApp {
        router: build_app(state),
        db,
        upload_dir,
    }
}

/// `GET uri` をルーターへ投げ、ステータスコードと本文（UTF-8 文字列）を返す。
///
/// `oneshot` はルーターの所有権を消費するので、`clone()` した複製に投げる。
/// こうすると同じ `TestApp` に何度でもリクエストできる。
pub async fn get(router: &Router, uri: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// 投稿を1件 INSERT し、採番された id を返す。
pub async fn insert_post(db: &SqlitePool, title: &str, text: &str, image: Option<&str>) -> i64 {
    sqlx::query("INSERT INTO post (title, text, image) VALUES (?, ?, ?)")
        .bind(title)
        .bind(text)
        .bind(image)
        .execute(db)
        .await
        .expect("INSERT できること")
        .last_insert_rowid()
}
