//! `GET /`（投稿一覧）の統合テスト。
//!
//! `build_app` で組み立てた本物のルーターに HTTP リクエストを投げ、
//! ステータスコードと返ってきた HTML の中身を検査する。

mod common;

use axum::http::StatusCode;

/// HTML 内に `needle` が何回出てくるかを数える。
fn count(html: &str, needle: &str) -> usize {
    html.matches(needle).count()
}

#[tokio::test]
async fn list_shows_empty_message_when_no_posts() {
    let app = common::setup().await;

    let (status, html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("投稿がまだありません"));
    assert_eq!(count(&html, "<article"), 0);
}

#[tokio::test]
async fn list_shows_posts_newest_first() {
    let app = common::setup().await;
    let oldest = common::insert_post(&app.db, "最初の投稿", "一番古い本文", None).await;
    common::insert_post(&app.db, "二番目の投稿", "真ん中の本文", None).await;
    common::insert_post(&app.db, "最新の投稿", "一番新しい本文", None).await;
    let oldest_date: String = sqlx::query_scalar("SELECT regist_date FROM post WHERE id = ?")
        .bind(oldest)
        .fetch_one(&app.db)
        .await
        .unwrap();

    let (status, html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("投稿がまだありません"));
    assert_eq!(count(&html, "<article"), 3);
    for text in ["一番古い本文", "真ん中の本文", "一番新しい本文"] {
        assert!(html.contains(text), "本文 {text} が表示されること");
    }
    // 投稿日時（regist_date）も表示される
    assert!(html.contains(&oldest_date));
    // 新しい投稿ほど上（HTML の前の方）に出る
    let newest_at = html.find("最新の投稿").unwrap();
    let middle_at = html.find("二番目の投稿").unwrap();
    let oldest_at = html.find("最初の投稿").unwrap();
    assert!(newest_at < middle_at && middle_at < oldest_at);
}

#[tokio::test]
async fn list_shows_image_only_for_posts_with_image() {
    let app = common::setup().await;
    common::insert_post(&app.db, "画像あり", "本文", Some("1.jpg")).await;
    common::insert_post(&app.db, "画像なし", "本文", None).await;

    let (status, html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&html, "<img"), 1);
    assert!(html.contains(r#"src="/uploads/1.jpg""#));
}

#[tokio::test]
async fn list_escapes_html_in_posts() {
    let app = common::setup().await;
    common::insert_post(&app.db, "<script>alert(1)</script>", "<b>太字</b>", None).await;

    let (status, html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("<script>alert(1)</script>"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<b>太字</b>"));
}

#[tokio::test]
async fn list_returns_500_when_db_is_unavailable() {
    let app = common::setup().await;
    // プールを閉じると以降の問い合わせはすべて失敗する（DB 障害の再現）
    app.db.close().await;

    let (status, _html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
