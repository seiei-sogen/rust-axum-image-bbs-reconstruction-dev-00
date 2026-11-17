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

/// 一覧画面（トップページ `/`）のビューモデル。
///
/// タスク文書では `IndexTemplate` と呼んでいるもの。設計書 `interfaces.rs` の名前 `IndexView` に合わせた。
/// 投稿一覧やページ情報のフィールドは後続タスクで追加する。
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexView {}
