//! テスト基盤（`tests/common/mod.rs`）そのものの確認。
//!
//! - `setup()` で作ったアプリが `GET /` に 200 を返す（いちばん小さい「動いている」確認）
//! - `setup()` を2回呼ぶと、DB もアップロード先も別々になる（テスト同士が干渉しない）
//!
//! 学習メモ:
//! - `cargo test` は既定でテストを並列に実行する。テストごとに `setup()` で使い捨ての
//!   in-memory DB と一時ディレクトリを作るので、並列に動いても互いのデータが見えない。
//! - `oneshot` はポートを listen しないので、並列に動いてもポート番号がぶつからない。

mod common;

use axum::http::StatusCode;

#[tokio::test]
async fn smoke_get_root_returns_200() {
    let app = common::setup().await;

    let (status, html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::OK, "GET / は 200 を返す");
    assert!(html.contains("<form"), "トップページには投稿フォームがある");
}

#[tokio::test]
async fn two_setups_do_not_share_db_or_upload_dir() {
    let first = common::setup().await;
    let second = common::setup().await;

    // 片方の DB に入れた投稿は、もう片方からは見えない
    common::insert_post(&first.db, "first だけの投稿", "", None).await;
    assert_eq!(common::post_count(&first.db).await, 1);
    assert_eq!(common::post_count(&second.db).await, 0);
    let (_, html) = common::get(&second.router, "/").await;
    assert!(!html.contains("first だけの投稿"));

    // アップロード先も別のディレクトリ
    assert_ne!(first.upload_dir.path(), second.upload_dir.path());

    // TestApp を drop すると、一時ディレクトリも消える（次のテストにファイルを持ち越さない）
    // `to_path_buf()` でパスを複製しておくのは、drop の後で `first` を参照できなくなるから
    let first_dir = first.upload_dir.path().to_path_buf();
    assert!(first_dir.exists());
    drop(first);
    assert!(!first_dir.exists(), "drop で一時ディレクトリが削除される");
    assert!(second.upload_dir.path().exists(), "もう片方は残っている");
}
