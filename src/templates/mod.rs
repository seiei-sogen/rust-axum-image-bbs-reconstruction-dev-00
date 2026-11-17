//! Askama テンプレート構造体を定義するモジュール。
//!
//! 学習メモ:
//! - `#[derive(Template)]` と `#[template(path = "index.html")]` を付けると、
//!   Askama がコンパイル時に `templates/index.html`（Cargo.toml と同じ階層の templates/ が起点）を読み込み、
//!   HTML を組み立てる Rust コードを自動生成する。実行時にテンプレートを解析するコストはゼロ。
//! - テンプレート内の `{{ 変数名 }}` は、この構造体の同名フィールドを参照する。
//!   存在しないフィールドを書くとコンパイルエラーになるので、タイプミスに実行前に気づける。
//! - 拡張子が `.html` のテンプレートは、`{{ ... }}` の出力が自動で HTML エスケープされる
//!   （`<script>` が `&lt;script&gt;` になる）。ユーザー入力をそのまま出しても XSS にならない（NFR-104）。

use askama::Template;
use axum::http::StatusCode;

use crate::models::post::Post;
use crate::validators::post_form::PostFormInput;

/// 一覧画面（トップページ `/`）のビューモデル。
///
/// タスク文書では `IndexTemplate` と呼んでいるもの。設計書 `interfaces.rs` の名前 `IndexView` に合わせた。
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexView {
    /// このページに表示する投稿（新しい順）。テンプレートの `{% for post in posts %}` で回す。
    pub posts: Vec<Post>,
    /// 現在のページ番号（1始まり）。
    pub page: u32,
    /// 総ページ数。投稿0件でも 1 にする。
    pub total_pages: u32,
    /// 投稿フォームに書き戻す入力値。バリデーション失敗時だけ中身が入り、通常は空（REQ-201）。
    pub form: PostFormInput,
    /// バリデーションエラーのメッセージ。空ならエラー欄を出さない（REQ-201）。
    pub errors: Vec<String>,
    /// 直前の操作の成功メッセージ（REQ-202）。メッセージは固定文なので `&'static str` で持つ。
    pub success_msg: Option<&'static str>,
}

/// エラーページ（400 / 404 / 413 / 500）のビューモデル。`AppError` の `IntoResponse` から使う。
#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorView {
    /// HTTP ステータス。テンプレートでは `404 Not Found` のように表示される（StatusCode の Display）。
    pub status: StatusCode,
    /// 利用者に見せるメッセージ。内部の詳細（SQL やファイルパス）は入れない。
    pub messages: Vec<String>,
}
