//! rust-axum-image-bbs のライブラリクレート。
//!
//! モジュール宣言と、ルーター組み立て関数 `build_app` をここに置く。
//! `src/main.rs` は起動処理（ログ初期化・DB 接続・マイグレーション・listen）だけを担当する。
//!
//! 学習メモ:
//! - Cargo は `src/lib.rs` があると「ライブラリクレート」、`src/main.rs` があると
//!   「バイナリクレート」として扱う。両方置くと、同じパッケージに2つのクレートが同居する。
//! - モジュールをライブラリ側で `pub mod` として公開しておくと、
//!   バイナリ（main.rs）や `tests/` 配下の統合テストから `rust_axum_image_bbs::...` で参照できる。
//!   統合テストは `build_app` で作ったルーターを、サーバを listen せずに直接呼び出せる。
//! - バイナリ側だけで `mod xxx;` と宣言した場合、まだ main から呼ばれていない関数は
//!   「未使用（dead_code）」警告になる。ライブラリで `pub` にした関数は外部から使われうるので警告にならない。
//!
//! TASK-0002 追加メモ（TraceLayer）:
//! - `TraceLayer::new_for_http()` を Router にかぶせると、リクエストの開始・終了・
//!   ステータスコードが自動でログ出力される。ハンドラ側でロギングを書く必要がない。

pub mod config;
pub mod errors;
pub mod handlers;
pub mod models;
pub mod repository;
pub mod state;
pub mod templates;

use axum::{extract::DefaultBodyLimit, routing::get, Router};
use tower_http::trace::TraceLayer;

use crate::handlers::posts;
use crate::state::AppState;

/// アプリのルーター（URL とハンドラの対応表 + ミドルウェア）を組み立てる。
///
/// 学習メモ:
/// - 引数 `state` は値渡し（所有権ごと受け取る）。最後の `.with_state(state)` で
///   ルーターの中へムーブされ、以降は各ハンドラが `State<AppState>` で複製（Clone）を受け取る。
/// - 戻り値の `Router` は `main.rs` では `axum::serve` に渡し、統合テストでは `oneshot` で直接呼び出す。
pub fn build_app(state: AppState) -> Router {
    // `state` をルーターへムーブする前に、必要な値（usize はコピーされる）を取り出しておく
    let max_body_bytes = state.config.max_image_bytes;

    // ---------- ルーター組み立て ----------
    //
    // TraceLayer::new_for_http() は Tower のミドルウェア（Layer）として動作する。
    // Router に `.layer(...)` で重ねると、すべてのリクエスト/レスポンスが
    // このレイヤーを通過するたびに tracing スパンが生成されてログが出る。
    //
    // 学習メモ:
    // - axum の `.layer()` は「ミドルウェアを外側から被せる」イメージ。
    //   複数呼べば複数のレイヤーが積み重なる。
    // - TraceLayer はリクエスト開始時に INFO ログ、終了時に INFO + ステータスを出す。
    //   内部の詳細は DEBUG レベルで出るため、tower_http=debug にしておくと見やすい。
    // - AppState は Router::with_state で各ハンドラへ共有する。
    Router::new()
        .route("/", get(posts::list))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
