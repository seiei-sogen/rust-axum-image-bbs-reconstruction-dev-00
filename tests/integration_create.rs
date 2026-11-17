//! `POST /posts`（投稿作成）の統合テスト。
//!
//! ブラウザと同じ multipart/form-data のリクエストを本物のルーターに投げ、
//! ステータス・リダイレクト先・再表示された HTML・DB の行を検査する。

mod common;

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use common::Part;

/// PNG のマジックナンバー。中身の形式は先頭バイトで判定されるので、後ろは何でもよい。
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const FIVE_MB: usize = 5 * 1024 * 1024;

fn png_of_len(len: usize) -> Vec<u8> {
    let mut bytes = PNG_MAGIC.to_vec();
    bytes.resize(len, 0);
    bytes
}

#[tokio::test]
async fn create_inserts_post_and_redirects_with_303() {
    let app = common::setup().await;

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[Part::Text("title", "hello"), Part::Text("text", "world")],
    )
    .await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(res.location.as_deref(), Some("/?notice=created"));
    let (title, text, image): (String, String, Option<String>) =
        sqlx::query_as("SELECT title, text, image FROM post")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(
        (title.as_str(), text.as_str(), image),
        ("hello", "world", None)
    );
}

#[tokio::test]
async fn success_message_is_shown_only_after_create() {
    let app = common::setup().await;
    let res = common::post_multipart(&app.router, "/posts", &[Part::Text("title", "hello")]).await;

    let (status, html) = common::get(&app.router, res.location.as_deref().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("投稿しました"));
    assert!(html.contains("hello"));

    let (_, plain) = common::get(&app.router, "/").await;
    assert!(!plain.contains("投稿しました"));
    let (_, unknown) = common::get(&app.router, "/?notice=hacked").await;
    assert!(!unknown.contains("投稿しました") && !unknown.contains("hacked"));
}

#[tokio::test]
async fn japanese_title_of_50_chars_is_accepted() {
    let app = common::setup().await;
    let title = "あ".repeat(50);

    let res = common::post_multipart(&app.router, "/posts", &[Part::Text("title", &title)]).await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(common::post_count(&app.db).await, 1);
}

#[tokio::test]
async fn empty_title_rerenders_form_with_input_kept() {
    let app = common::setup().await;
    common::insert_post(&app.db, "既存の投稿", "既存の本文", None).await;

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[Part::Text("title", ""), Part::Text("text", "入力した本文")],
    )
    .await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.body.contains("alert-danger"));
    assert!(res.body.contains("タイトルは必須です"));
    assert!(res.body.contains(">入力した本文</textarea>"));
    // フォームの下には一覧もそのまま表示される
    assert!(res.body.contains("既存の投稿"));
    assert_eq!(common::post_count(&app.db).await, 1);
}

#[tokio::test]
async fn text_of_501_chars_is_rejected_and_title_is_kept() {
    let app = common::setup().await;
    let text = "あ".repeat(501);

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "残るタイトル"),
            Part::Text("text", &text),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.body.contains("本文は500文字以内で入力してください"));
    assert!(res.body.contains(r#"value="残るタイトル""#));
    assert_eq!(common::post_count(&app.db).await, 0);
}

#[tokio::test]
async fn rerendered_input_is_html_escaped() {
    let app = common::setup().await;
    let title = r#""><script>alert(1)</script>"#;

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", title),
            Part::Text("text", &"x".repeat(501)),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(!res.body.contains("<script>alert(1)</script>"));
    assert!(res.body.contains("&lt;script&gt;"));
}

#[tokio::test]
async fn empty_file_part_from_browser_means_no_image() {
    let app = common::setup().await;

    // ファイル未選択のブラウザは filename="" で空のパートを送る
    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "画像なし"),
            Part::File("image", "", b""),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(common::post_count(&app.db).await, 1);
}

#[tokio::test]
async fn zero_byte_image_file_is_rejected() {
    let app = common::setup().await;

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "空の画像"),
            Part::File("image", "empty.jpg", b""),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.body.contains("画像ファイルが空です"));
    assert_eq!(common::post_count(&app.db).await, 0);
}

#[tokio::test]
async fn image_just_over_5mb_is_a_validation_error() {
    let app = common::setup().await;
    let image = png_of_len(FIVE_MB + 1);

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "大きい"),
            Part::File("image", "a.png", &image),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.body.contains("画像サイズは5MB以内にしてください"));
    assert!(res.body.contains(r#"value="大きい""#));
    assert_eq!(common::post_count(&app.db).await, 0);
}

#[tokio::test]
async fn body_far_over_the_limit_returns_413() {
    let app = common::setup().await;
    let image = png_of_len(6 * 1024 * 1024);

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "大きすぎる"),
            Part::File("image", "a.png", &image),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(common::post_count(&app.db).await, 0);
}

#[tokio::test]
async fn broken_multipart_returns_400() {
    let app = common::setup().await;
    let request = Request::post("/posts")
        .header(CONTENT_TYPE, "multipart/form-data; boundary=XYZ")
        .body(Body::from(
            "--XYZ\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nno end",
        ))
        .unwrap();

    let res = common::send(&app.router, request).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert_eq!(common::post_count(&app.db).await, 0);
}

#[tokio::test]
async fn non_multipart_request_returns_400() {
    let app = common::setup().await;
    let request = Request::post("/posts")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("title=hello"))
        .unwrap();

    let res = common::send(&app.router, request).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert_eq!(common::post_count(&app.db).await, 0);
}
