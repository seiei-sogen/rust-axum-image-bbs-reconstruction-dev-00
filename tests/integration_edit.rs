//! 投稿の編集（`GET /posts/{id}/edit` / `POST /posts/{id}/edit`）の統合テスト。
//!
//! 本物のルーターに HTTP リクエストを投げ、ステータス・HTML・DB の行・アップロード先のファイルを検査する。

mod common;

use axum::http::StatusCode;
use common::{Part, TestApp};

/// 形式は先頭バイトで判定されるので、後ろは中身を見分けるための適当な文字列。
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnew png";
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0old jpeg";

/// `POST /posts` で「古いタイトル / 古い本文 / 画像 `file`」の投稿を作り、その id を返す。
async fn create_with_image(app: &TestApp, file: &[u8]) -> i64 {
    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "古いタイトル"),
            Part::Text("text", "古い本文"),
            Part::File("image", "old", file),
        ],
    )
    .await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    sqlx::query_scalar("SELECT MAX(id) FROM post")
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// 投稿 id の (title, text, image) を DB から読む。
async fn row(app: &TestApp, id: i64) -> (String, String, Option<String>) {
    sqlx::query_as("SELECT title, text, image FROM post WHERE id = ?")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// 作成直後の (title, text, image)。画像は `{id}.{ext}` で保存されている。
fn untouched_row(id: i64, ext: &str) -> (String, String, Option<String>) {
    (
        "古いタイトル".into(),
        "古い本文".into(),
        Some(format!("{id}.{ext}")),
    )
}

/// アップロード先のファイル `name` の中身。
fn upload(app: &TestApp, name: &str) -> Vec<u8> {
    std::fs::read(app.upload_dir.path().join(name)).unwrap()
}

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

#[tokio::test]
async fn edit_updates_title_and_text_and_keeps_the_image() {
    let app = common::setup().await;
    let id = create_with_image(&app, JPEG).await;

    // ファイル未選択のブラウザは filename="" の空パートを送る。それは「画像はそのまま」
    let res = common::post_multipart(
        &app.router,
        &format!("/posts/{id}/edit"),
        &[
            Part::Text("title", "新しいタイトル"),
            Part::Text("text", "新しい本文"),
            Part::File("image", "", b""),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let jpg = format!("{id}.jpg");
    assert_eq!(
        row(&app, id).await,
        (
            "新しいタイトル".into(),
            "新しい本文".into(),
            Some(jpg.clone())
        )
    );
    assert_eq!(common::upload_files(&app), [jpg.as_str()]);
    assert_eq!(upload(&app, &jpg), JPEG);

    // リダイレクト先の一覧に、更新後の内容と成功メッセージが出る
    let (status, html) = common::get(&app.router, res.location.as_deref().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("更新しました"));
    assert!(html.contains("新しいタイトル"));
}

#[tokio::test]
async fn edit_with_new_image_replaces_it_and_removes_the_old_extension() {
    let app = common::setup().await;
    let id = create_with_image(&app, JPEG).await;

    let res = common::post_multipart(
        &app.router,
        &format!("/posts/{id}/edit"),
        &[
            Part::Text("title", "古いタイトル"),
            Part::File("image", "new.png", PNG),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let png = format!("{id}.png");
    assert_eq!(row(&app, id).await.2, Some(png.clone()));
    assert_eq!(common::upload_files(&app), [png.as_str()]);
    assert_eq!(upload(&app, &png), PNG);
}

#[tokio::test]
async fn edit_with_new_image_of_the_same_extension_overwrites_it() {
    let app = common::setup().await;
    let id = create_with_image(&app, b"\x89PNG\r\n\x1a\nold png").await;

    let res = common::post_multipart(
        &app.router,
        &format!("/posts/{id}/edit"),
        &[
            Part::Text("title", "古いタイトル"),
            Part::File("image", "new.png", PNG),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let png = format!("{id}.png");
    assert_eq!(row(&app, id).await.2, Some(png.clone()));
    assert_eq!(common::upload_files(&app), [png.as_str()]);
    assert_eq!(upload(&app, &png), PNG);
}

#[tokio::test]
async fn invalid_edit_rerenders_the_form_with_400_and_changes_nothing() {
    let app = common::setup().await;
    let id = create_with_image(&app, JPEG).await;

    let res = common::post_multipart(
        &app.router,
        &format!("/posts/{id}/edit"),
        &[
            Part::Text("title", ""),
            Part::Text("text", "入力した本文"),
            Part::File("image", "fake.png", b"this is not an image"),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.body.contains("alert-danger"));
    assert!(res.body.contains("タイトルは必須です"));
    assert!(res
        .body
        .contains("対応していない画像形式です (jpg/gif/png)"));
    // 入力値は残り、送信先と今の画像のプレビューは元の投稿のまま
    assert!(res.body.contains(">入力した本文</textarea>"));
    assert!(res.body.contains(&format!(r#"action="/posts/{id}/edit""#)));
    assert!(res.body.contains(&format!(r#"src="/uploads/{id}.jpg""#)));
    assert_eq!(row(&app, id).await, untouched_row(id, "jpg"));
    assert_eq!(common::upload_files(&app), [format!("{id}.jpg")]);
}

#[tokio::test]
async fn edit_of_missing_post_returns_404_page() {
    let app = common::setup().await;

    let res =
        common::post_multipart(&app.router, "/posts/999/edit", &[Part::Text("title", "x")]).await;

    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert!(res.body.contains("ページが見つかりません"));
    assert_eq!(common::post_count(&app.db).await, 0);
}

#[tokio::test]
async fn failed_image_save_keeps_the_post_and_its_old_file() {
    let app = common::setup().await;
    let id = create_with_image(&app, JPEG).await;
    // 新しい画像の保存先 `{id}.png` にディレクトリを置いて、保存を失敗させる
    std::fs::create_dir(app.upload_dir.path().join(format!("{id}.png"))).unwrap();

    let res = common::post_multipart(
        &app.router,
        &format!("/posts/{id}/edit"),
        &[
            Part::Text("title", "新しいタイトル"),
            Part::Text("text", "新しい本文"),
            Part::File("image", "new.png", PNG),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(row(&app, id).await, untouched_row(id, "jpg"));
    assert_eq!(upload(&app, &format!("{id}.jpg")), JPEG);
    // 書きかけのファイルも残らない（あるのは古い画像と、塞ぐために置いたディレクトリだけ）
    assert_eq!(
        common::upload_files(&app),
        [format!("{id}.jpg"), format!("{id}.png")]
    );
}
