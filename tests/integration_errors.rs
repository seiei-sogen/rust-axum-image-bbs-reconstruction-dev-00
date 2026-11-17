//! エラー応答（404 / 400 / 500）の統合テスト。
//!
//! どの失敗も `AppError` を通って、同じ見た目の HTML エラーページになることを確かめる。

mod common;

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};

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
