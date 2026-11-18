//! エラー応答（404 / 400 / 500）の統合テスト。
//!
//! どの失敗も `AppError` を通って、同じ見た目の HTML エラーページになることを確かめる。

mod common;

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use rust_axum_image_bbs::{build_app, config::AppConfig, state::AppState};
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn unknown_path_returns_404_html_page() {
    let app = common::setup().await;

    let res = common::send(
        &app.router,
        Request::get("/no-such-page").body(Body::empty()).unwrap(),
    )
    .await;

    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert!(res.content_type.unwrap().starts_with("text/html"));
    assert!(res.body.contains("ページが見つかりません"));
    assert!(res.body.contains(r#"href="/""#), "トップへ戻るリンクがある");
}

#[tokio::test]
async fn missing_upload_returns_404() {
    let app = common::setup().await;

    let res = common::send(
        &app.router,
        Request::get("/uploads/999.png")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn non_multipart_post_returns_400_error_page() {
    let app = common::setup().await;
    let request = Request::post("/posts")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"hello"}"#))
        .unwrap();

    let res = common::send(&app.router, request).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.content_type.unwrap().starts_with("text/html"));
    assert!(res
        .body
        .contains("フォームの送信内容を読み取れませんでした"));
}

#[tokio::test]
async fn db_failure_returns_500_page_without_internal_details() {
    let app = common::setup().await;
    // プールを閉じると以降の問い合わせはすべて失敗する（DB 障害の再現）
    app.db.close().await;

    let res = common::send(&app.router, Request::get("/").body(Body::empty()).unwrap()).await;

    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(res.content_type.unwrap().starts_with("text/html"));
    assert!(res.body.contains("サーバでエラーが発生しました"));
    assert!(!res.body.to_lowercase().contains("pool"));
}

#[tokio::test]
async fn unmigrated_db_returns_500_and_logs_the_cause_as_error() {
    // マイグレーションを当てていない空の DB（post テーブルが無い）でアプリを組み立てる
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let upload_dir = tempfile::TempDir::new().unwrap();
    let router = build_app(AppState {
        db,
        upload_dir: upload_dir.path().to_path_buf(),
        config: AppConfig::from_env_or_default(),
    });
    let (logs, _guard) = common::capture_logs();

    let (status, html) = common::get(&router, "/").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(html.contains("サーバでエラーが発生しました"));
    assert!(!html.contains("no such table"), "原因は利用者には見せない");
    // 運用者が原因を追えるよう、ERROR のログに DB のエラー内容が残る（EDGE-004）
    let logs = logs.text();
    assert!(
        logs.lines()
            .any(|line| line.contains("ERROR") && line.contains("no such table: post")),
        "ログ:\n{logs}"
    );
}
