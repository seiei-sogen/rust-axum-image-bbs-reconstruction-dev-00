//! ログ（tracing）の統合テスト。
//!
//! 運用者が追える情報（投稿 id・リクエストのステータス・失敗の原因）がログに出て、
//! 利用者が入力したタイトルや本文はログに出ないことを確かめる。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Part;

#[tokio::test]
async fn create_edit_and_delete_log_the_post_id_but_not_the_content() {
    let app = common::setup().await;
    let (logs, _guard) = common::capture_logs();

    let created = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "ひみつのタイトル"),
            Part::Text("text", "ひみつの本文"),
        ],
    )
    .await;
    assert_eq!(created.status, StatusCode::SEE_OTHER);
    let id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM post")
        .fetch_one(&app.db)
        .await
        .unwrap();
    let edited = common::post_multipart(
        &app.router,
        &format!("/posts/{id}/edit"),
        &[
            Part::Text("title", "べつのタイトル"),
            Part::Text("text", "べつの本文"),
        ],
    )
    .await;
    assert_eq!(edited.status, StatusCode::SEE_OTHER);
    let delete = Request::post(format!("/posts/{id}/delete"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        common::send(&app.router, delete).await.status,
        StatusCode::SEE_OTHER
    );

    let logs = logs.text();
    let post_id = format!("post_id={id}");
    for event in ["post created", "post updated", "post deleted"] {
        assert!(
            logs.lines()
                .any(|line| line.contains(event) && line.contains(&post_id)),
            "「{event}」の行に {post_id} が付く。ログ:\n{logs}"
        );
    }
    // リクエストごとの行（TraceLayer）に、メソッド・URL・ステータスが出る
    assert!(
        logs.lines().any(|line| line.contains("method=POST")
            && line.contains("uri=/posts")
            && line.contains("status=303")),
        "ログ:\n{logs}"
    );
    for content in [
        "ひみつのタイトル",
        "ひみつの本文",
        "べつのタイトル",
        "べつの本文",
    ] {
        assert!(!logs.contains(content), "{content} はログに出ない");
    }
}

#[tokio::test]
async fn rollback_logs_the_post_id_and_the_reason() {
    let app = common::setup().await;
    // 最初の投稿は id=1。保存先 `1.png` にディレクトリを置いて、画像の保存を失敗させる
    std::fs::create_dir(app.upload_dir.path().join("1.png")).unwrap();
    let (logs, _guard) = common::capture_logs();

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "保存失敗"),
            Part::File("image", "a.png", b"\x89PNG\r\n\x1a\nbody"),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    let logs = logs.text();
    assert!(
        logs.lines().any(|line| line.contains("WARN")
            && line.contains("rolling back")
            && line.contains("post_id=1")
            && line.contains("reason=io error")),
        "取り消しの行に投稿 id と原因が出る。ログ:\n{logs}"
    );
}
