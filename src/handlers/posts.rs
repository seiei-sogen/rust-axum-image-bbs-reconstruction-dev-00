//! 投稿（post）リソースのハンドラ。
//!
//! ハンドラの責務は「HTTP の入力を受け取る → リポジトリを呼ぶ → ビューモデルに詰めて返す」だけ。
//! SQL はリポジトリ（`repository::post_repo`）、HTML はテンプレート（`templates/`）に任せる。

use axum::extract::State;

use crate::errors::AppResult;
use crate::repository::post_repo;
use crate::state::AppState;
use crate::templates::IndexView;

/// `GET /` 投稿一覧（新しい順に1ページぶん）を描画する。
///
/// 学習メモ:
/// - `State(state): State<AppState>` は「引数の位置でパターンマッチする」書き方。
///   axum が Router に登録された AppState の複製を `State(...)` に包んで渡してくるので、
///   それを分解して中身だけを変数 `state` に受け取っている。
/// - axum は `IntoResponse` を実装した任意の型を返せる。
///   Askama の `#[derive(Template)]` は（askama_axum 経由で）テンプレート構造体に `IntoResponse` を実装するので、
///   `IndexView` を返すだけで `text/html; charset=utf-8` のレスポンスになる。
///   `AppResult<IndexView>` の `Err` 側は `AppError` の `IntoResponse` 実装で 500 になる。
/// - `post_repo::list(&state.db, ...)` の `&` は借用。プールは `state` が持ったまま貸すだけ。
pub async fn list(State(state): State<AppState>) -> AppResult<IndexView> {
    let page_size = state.config.page_size;
    let posts = post_repo::list(&state.db, page_size, 0).await?;
    let total = post_repo::count(&state.db).await?;
    // 割り算の切り上げ。投稿0件だと 0 ページになるので最低 1 にする（ページネーションは次のタスクで本格化）
    let total_pages = total.div_ceil(page_size).max(1);

    Ok(IndexView {
        posts,
        page: 1,
        total_pages,
    })
}
