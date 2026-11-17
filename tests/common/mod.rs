//! 統合テスト（tests/*.rs）で共有するセットアップヘルパ。
//!
//! 学習メモ:
//! - `tests/` 直下の `.rs` ファイルは、それぞれが独立したテスト用クレートとしてビルドされる。
//!   `tests/common/mod.rs` はサブディレクトリにあるのでテストクレート扱いされず、
//!   各テストファイルから `mod common;` で取り込んで共有できる。
//! - 取り込んだテストファイルによっては使わない関数があり「未使用」警告が出るため、
//!   このファイルに限り `#[allow(dead_code)]` を付けてよいことにしている。

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, LOCATION};
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
#[allow(dead_code)]
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

/// `send` / `post_multipart` の結果。
#[allow(dead_code)]
pub struct TestResponse {
    pub status: StatusCode,
    /// `Location` ヘッダ（リダイレクト先）。無ければ `None`。
    pub location: Option<String>,
    /// 本文。HTML を検査しやすいよう文字列にしておく（UTF-8 でない部分は置き換え文字になる）。
    pub body: String,
    /// 本文の生のバイト列（画像の中身を比べるとき用）。
    pub bytes: Vec<u8>,
    /// `Content-Type` ヘッダ。
    pub content_type: Option<String>,
}

/// 任意のリクエストをルーターへ投げ、ステータス・Location・本文を返す。
#[allow(dead_code)]
pub async fn send(router: &Router, request: Request<Body>) -> TestResponse {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|value: &axum::http::HeaderValue| value.to_str().unwrap().to_string())
    };
    let location = header(LOCATION);
    let content_type = header(CONTENT_TYPE);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    TestResponse {
        status,
        location,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        bytes: bytes.to_vec(),
        content_type,
    }
}

/// UPLOAD_DIR にあるファイル名の一覧（名前順）。
#[allow(dead_code)]
pub fn upload_files(app: &TestApp) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(app.upload_dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// multipart/form-data の1欄。
#[allow(dead_code)]
pub enum Part<'a> {
    /// テキスト欄: (name, 値)
    Text(&'a str, &'a str),
    /// ファイル欄: (name, ファイル名, 中身)
    File(&'a str, &'a str, &'a [u8]),
}

/// ブラウザのフォーム送信と同じ形の multipart/form-data で `POST uri` する。
#[allow(dead_code)]
pub async fn post_multipart(router: &Router, uri: &str, parts: &[Part<'_>]) -> TestResponse {
    const BOUNDARY: &str = "----test-boundary-7MA4YWxkTrZu0gW";
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match part {
            Part::Text(name, value) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}")
                    .as_bytes(),
            ),
            Part::File(name, filename, bytes) => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                         Content-Type: application/octet-stream\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(bytes);
            }
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    let request = Request::post(uri)
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    send(router, request).await
}

/// `post` テーブルの行数。
#[allow(dead_code)]
pub async fn post_count(db: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM post")
        .fetch_one(db)
        .await
        .unwrap()
}

/// 投稿を1件 INSERT し、採番された id を返す。
#[allow(dead_code)]
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
