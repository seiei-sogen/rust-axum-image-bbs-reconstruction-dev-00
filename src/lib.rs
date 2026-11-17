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
pub mod storage;
pub mod templates;
pub mod validators;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::errors::AppError;
use crate::handlers::posts;
use crate::state::AppState;

/// リクエストボディの上限を決めるとき、画像の上限（`max_image_bytes`）に上乗せする余裕（64KiB）。
///
/// multipart のボディには画像のほかに、区切り線・各欄のヘッダ・タイトルと本文の値が入る。
/// 上限を画像の上限ちょうどにすると、ちょうど 5MB の画像（受け付けるべきもの）まで 413 で弾かれてしまう。
/// - 画像 ≤ 5MB              → 受け付ける
/// - 5MB < 画像 ≤ 5MB + 余裕 → ボディは読めるのでバリデータまで届き、フォーム再表示の 400
/// - それより大きい          → 読み取りの途中で打ち切って 413（巨大なボディをメモリに溜めない）
///
/// タイトル 50 文字・本文 500 文字は UTF-8 でも数 KB なので、64KiB あれば区切りやヘッダを含めて十分足りる。
const MULTIPART_OVERHEAD_BYTES: usize = 64 * 1024;

/// アプリのルーター（URL とハンドラの対応表 + ミドルウェア）を組み立てる。
///
/// 学習メモ:
/// - 引数 `state` は値渡し（所有権ごと受け取る）。最後の `.with_state(state)` で
///   ルーターの中へムーブされ、以降は各ハンドラが `State<AppState>` で複製（Clone）を受け取る。
/// - 戻り値の `Router` は `main.rs` では `axum::serve` に渡し、統合テストでは `oneshot` で直接呼び出す。
pub fn build_app(state: AppState) -> Router {
    // `state` をルーターへムーブする前に、必要な値（usize はコピーされる）を取り出しておく
    let max_body_bytes = state.config.max_image_bytes + MULTIPART_OVERHEAD_BYTES;
    // `/uploads/5.png` → `{upload_dir}/5.png` を返す静的ファイル配信（REQ-013）。
    // ServeDir は `..` を含むパスを拒否するので、upload_dir の外のファイルは読めない。
    // `clone()` するのは、`state` 本体はこの後 `with_state` へムーブするため。
    let uploads = ServeDir::new(state.upload_dir.clone());

    // ---------- ルーター組み立て ----------
    //
    // TraceLayer::new_for_http() は Tower のミドルウェア（Layer）として動作する。
    // Router に `.layer(...)` で重ねると、すべてのリクエスト/レスポンスが
    // このレイヤーを通過するたびに tracing スパンが生成されてログが出る。
    //
    // 学習メモ:
    // - axum の `.layer()` は「ミドルウェアを外側から被せる」イメージ。
    //   複数呼べば複数のレイヤーが積み重なる。
    // - TraceLayer はリクエスト開始時（on_request）と終了時（on_response、ステータスと所要時間つき）に
    //   DEBUG レベルでログを出す。既定の RUST_LOG（main.rs 参照）で tower_http=debug にしているのは、
    //   このログを表示するため。
    // - AppState は Router::with_state で各ハンドラへ共有する。
    Router::new()
        .route("/", get(posts::list))
        .route("/posts", post(posts::create))
        // `:id` はパスパラメータ。ハンドラ側で `Path<i64>` として受け取る
        // 同じ URL でも、GET はフォーム表示、POST は更新処理と、メソッドごとにハンドラを分けられる
        .route(
            "/posts/:id/edit",
            get(posts::edit_form).post(posts::edit_submit),
        )
        // `route` は「この URL にこのハンドラ関数」、`nest_service` は「この URL 以下を丸ごとこのサービスに」。
        // ServeDir には `/uploads` を取り除いた残り（`/5.png`）が渡る。
        .nest_service("/uploads", uploads)
        // どのルートにも当たらない未定義パスは、AppError::NotFound の 404 ページにする。
        // `.layer()` は「それより前に登録したもの」にだけかかるので、fallback は layer より前に置く。
        .fallback(|| async { AppError::NotFound })
        // multipart の読み取りはこの上限で打ち切られる（超えたら 413）
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
