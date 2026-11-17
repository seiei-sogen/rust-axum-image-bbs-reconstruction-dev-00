//! 投稿の編集（`GET /posts/{id}/edit` / `POST /posts/{id}/edit`）の統合テスト。
//!
//! 本物のルーターに HTTP リクエストを投げ、ステータス・HTML・DB の行・アップロード先のファイルを検査する。

mod common;

use axum::http::StatusCode;

#[tokio::test]
async fn edit_form_shows_current_values() {
    let app = common::setup().await;
    let id = common::insert_post(&app.db, "今のタイトル", "今の本文", Some("1.png")).await;

    let (status, html) = common::get(&app.router, &format!("/posts/{id}/edit")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"value="今のタイトル""#));
    assert!(html.contains(">今の本文</textarea>"));
    // 送信先は同じ URL への multipart POST
    assert!(html.contains(&format!(r#"action="/posts/{id}/edit""#)));
    assert!(html.contains("multipart/form-data"));
    // 今の画像のプレビューと、画像だけを消すチェックボックス（REQ-103）
    assert!(html.contains(r#"src="/uploads/1.png""#));
    assert!(html.contains(r#"name="delete_image""#));
    // 登録前の確認ダイアログと、一覧へ戻るリンク
    assert!(html.contains("登録します。よろしいですか？"));
    assert!(html.contains(r#"href="/""#));
}

#[tokio::test]
async fn edit_form_without_image_has_no_preview() {
    let app = common::setup().await;
    let id = common::insert_post(&app.db, "画像なし", "", None).await;

    let (status, html) = common::get(&app.router, &format!("/posts/{id}/edit")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("/uploads/"));
    assert!(html.contains(r#"name="image""#), "新しい画像は選べる");
}

#[tokio::test]
async fn edit_form_for_missing_id_returns_404_page() {
    let app = common::setup().await;

    let (status, html) = common::get(&app.router, "/posts/999/edit").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("ページが見つかりません"));
}

#[tokio::test]
async fn list_links_each_post_to_its_edit_form() {
    let app = common::setup().await;
    let first = common::insert_post(&app.db, "1件目", "", None).await;
    let second = common::insert_post(&app.db, "2件目", "", None).await;

    let (_, html) = common::get(&app.router, "/").await;

    for id in [first, second] {
        assert!(html.contains(&format!(r#"href="/posts/{id}/edit""#)));
    }
}
