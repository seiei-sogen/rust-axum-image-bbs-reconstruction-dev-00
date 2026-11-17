//! 投稿（post）リソースのハンドラ。
//!
//! ハンドラの責務は「HTTP の入力を受け取る → リポジトリを呼ぶ → ビューモデルに詰めて返す」だけ。
//! SQL はリポジトリ（`repository::post_repo`）、HTML はテンプレート（`templates/`）、
//! ページ計算は `handlers::pagination` に任せる。

use axum::extract::{Query, State};

use crate::errors::AppResult;
use crate::handlers::pagination::{paginate, Pagination, PaginationQuery};
use crate::repository::post_repo;
use crate::state::AppState;
use crate::templates::IndexView;

/// `GET /?page=N` 投稿一覧（新しい順に1ページぶん）を描画する。
///
/// 学習メモ:
/// - `State(state): State<AppState>` は「引数の位置でパターンマッチする」書き方。
///   axum が Router に登録された AppState の複製を `State(...)` に包んで渡してくるので、
///   それを分解して中身だけを変数 `state` に受け取っている。
/// - `Option<Query<PaginationQuery>>` にすると、`?page=abc` のように変換できないとき
///   axum は 400 を返さず `None` を渡してくれる。`None` なら 1 ページ目として扱う。
/// - axum は `IntoResponse` を実装した任意の型を返せる。
///   Askama の `#[derive(Template)]` は（askama_axum 経由で）テンプレート構造体に `IntoResponse` を実装するので、
///   `IndexView` を返すだけで `text/html; charset=utf-8` のレスポンスになる。
///   `AppResult<IndexView>` の `Err` 側は `AppError` の `IntoResponse` 実装で 500 になる。
/// - `post_repo::list(&state.db, ...)` の `&` は借用。プールは `state` が持ったまま貸すだけ。
pub async fn list(
    State(state): State<AppState>,
    query: Option<Query<PaginationQuery>>,
) -> AppResult<IndexView> {
    // `map_or(既定値, クロージャ)`: Some なら中身に関数を適用し、None なら既定値を使う
    let requested_page = query.map_or(1, |Query(q)| q.page);
    let page_size = state.config.page_size;

    // 範囲外のページを最終ページに丸めるため、先に総件数を数えてから OFFSET を決める
    let total = post_repo::count(&state.db).await?;
    // 構造体のフィールドを一度に取り出す「分割代入（destructuring）」
    let Pagination {
        page,
        total_pages,
        offset,
    } = paginate(requested_page, total, page_size);
    let posts = post_repo::list(&state.db, page_size, offset).await?;

    Ok(IndexView {
        posts,
        page,
        total_pages,
    })
}
