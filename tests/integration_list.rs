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

/// `post-01`（最古）〜 `post-{n}`（最新）のタイトルで n 件投入する。
async fn insert_numbered_posts(db: &sqlx::SqlitePool, n: u32) {
    for i in 1..=n {
        common::insert_post(db, &format!("post-{i:02}"), "本文", None).await;
    }
}

/// カードのタイトル要素。`post-1` が `post-10` に部分一致しないよう、タグごと比較する。
fn title(n: u32) -> String {
    format!(">post-{n:02}</h3>")
}

#[tokio::test]
async fn pagination_shows_20_posts_then_the_remaining_5() {
    let app = common::setup().await;
    insert_numbered_posts(&app.db, 25).await;

    // 1ページ目: 新しい順に post-25 〜 post-06 の20件
    let (status, page1) = common::get(&app.router, "/?page=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&page1, "<article"), 20);
    assert!(page1.contains(&title(25)) && page1.contains(&title(6)));
    assert!(!page1.contains(&title(5)));
    assert!(page1.contains("1 / 2"));
    assert!(page1.contains(r#"href="?page=2""#) && page1.contains("次へ"));
    assert!(!page1.contains("前へ"));

    // 2ページ目: 残りの post-05 〜 post-01 の5件
    let (status, page2) = common::get(&app.router, "/?page=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&page2, "<article"), 5);
    assert!(page2.contains(&title(5)) && page2.contains(&title(1)));
    assert!(!page2.contains(&title(6)));
    assert!(page2.contains("2 / 2"));
    assert!(page2.contains(r#"href="?page=1""#) && page2.contains("前へ"));
    assert!(!page2.contains("次へ"));
}

#[tokio::test]
async fn invalid_page_falls_back_to_first_page() {
    let app = common::setup().await;
    insert_numbered_posts(&app.db, 25).await;
    let (_, first_page) = common::get(&app.router, "/").await;
    assert!(first_page.contains(&title(25)) && first_page.contains("1 / 2"));

    for uri in ["/?page=abc", "/?page=-1", "/?page=0", "/?page="] {
        let (status, html) = common::get(&app.router, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} は 400 ではなく 200");
        assert_eq!(html, first_page, "{uri} は1ページ目と同じ内容");
    }
}

#[tokio::test]
async fn page_beyond_last_shows_last_page() {
    let app = common::setup().await;
    insert_numbered_posts(&app.db, 25).await;

    let (status, html) = common::get(&app.router, "/?page=99").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(count(&html, "<article"), 5);
    assert!(html.contains(&title(1)) && html.contains("2 / 2"));
}

#[tokio::test]
async fn pager_is_hidden_when_everything_fits_on_one_page() {
    let app = common::setup().await;
    let (_, empty) = common::get(&app.router, "/").await;

    insert_numbered_posts(&app.db, 20).await;
    let (_, full_page) = common::get(&app.router, "/").await;

    for html in [empty, full_page] {
        assert!(!html.contains("pagination"));
        assert!(!html.contains("次へ") && !html.contains("前へ"));
    }
}

#[tokio::test]
async fn list_returns_500_when_db_is_unavailable() {
    let app = common::setup().await;
    // プールを閉じると以降の問い合わせはすべて失敗する（DB 障害の再現）
    app.db.close().await;

    let (status, _html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
