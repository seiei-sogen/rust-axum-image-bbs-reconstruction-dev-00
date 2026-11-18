//! 投稿の削除（`POST /posts/{id}/delete`）の統合テスト。
//!
//! 本物のルーターに HTTP リクエストを投げ、ステータス・DB の行・アップロード先のファイルを検査する。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{Part, TestApp, TestResponse};

/// ブラウザの削除フォームと同じく、ボディなしで `POST /posts/{id}/delete` する。
async fn post_delete(app: &TestApp, id: i64) -> TestResponse {
    let request = Request::post(format!("/posts/{id}/delete"))
        .body(Body::empty())
        .unwrap();
    common::send(&app.router, request).await
}

/// 投稿 id の行があるか。
async fn exists(app: &TestApp, id: i64) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM post WHERE id = ?")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
        == 1
}

/// `POST /posts` で PNG 画像つきの投稿を作り、その id を返す。
async fn create_with_png(app: &TestApp, title: &str) -> i64 {
    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", title),
            Part::File("image", "a.png", b"\x89PNG\r\n\x1a\nbody"),
        ],
    )
    .await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    sqlx::query_scalar("SELECT MAX(id) FROM post")
        .fetch_one(&app.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn delete_removes_the_post_and_its_image() {
    let app = common::setup().await;
    let keep = create_with_png(&app, "残す投稿").await;
    let gone = create_with_png(&app, "消す投稿").await;

    let res = post_delete(&app, gone).await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert!(!exists(&app, gone).await);
    assert!(exists(&app, keep).await);
    assert_eq!(common::upload_files(&app), [format!("{keep}.png")]);

    // リダイレクト先の一覧には成功メッセージが出て、消した投稿は出ない
    let (status, html) = common::get(&app.router, res.location.as_deref().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("削除しました"));
    assert!(html.contains("残す投稿"));
    assert!(!html.contains("消す投稿"));
}

#[tokio::test]
async fn delete_of_post_without_image_succeeds() {
    let app = common::setup().await;
    let id = common::insert_post(&app.db, "画像なし", "", None).await;

    let res = post_delete(&app, id).await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(common::post_count(&app.db).await, 0);
}

#[tokio::test]
async fn delete_of_missing_post_returns_404_page() {
    let app = common::setup().await;
    common::insert_post(&app.db, "無関係", "", None).await;

    let res = post_delete(&app, 999).await;

    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert!(res.body.contains("ページが見つかりません"));
    assert_eq!(common::post_count(&app.db).await, 1);
}

#[tokio::test]
async fn get_delete_returns_405_and_deletes_nothing() {
    let app = common::setup().await;
    let id = create_with_png(&app, "GET では消えない").await;

    let (status, _) = common::get(&app.router, &format!("/posts/{id}/delete")).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(exists(&app, id).await);
    assert_eq!(common::upload_files(&app), [format!("{id}.png")]);
}

#[tokio::test]
async fn delete_removes_the_row_even_if_the_image_cannot_be_removed() {
    let app = common::setup().await;
    let id = common::insert_post(&app.db, "画像を消せない", "", None).await;
    let png = format!("{id}.png");
    sqlx::query("UPDATE post SET image = ? WHERE id = ?")
        .bind(&png)
        .bind(id)
        .execute(&app.db)
        .await
        .unwrap();
    // 画像の場所にディレクトリを置くと、ファイルとしては削除できない（失敗の再現）
    std::fs::create_dir(app.upload_dir.path().join(&png)).unwrap();

    let res = post_delete(&app, id).await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert!(!exists(&app, id).await);
}

#[tokio::test]
async fn list_has_a_confirmed_delete_form_for_each_post() {
    let app = common::setup().await;
    let first = common::insert_post(&app.db, "1件目", "", None).await;
    let second = common::insert_post(&app.db, "2件目", "", None).await;

    let (_, html) = common::get(&app.router, "/").await;

    for id in [first, second] {
        assert!(html.contains(&format!(
            r#"<form method="post" action="/posts/{id}/delete""#
        )));
        assert!(html.contains(&format!(
            "confirm('IDが「{id}」のデータを削除します。よろしいですか？')"
        )));
    }
}
