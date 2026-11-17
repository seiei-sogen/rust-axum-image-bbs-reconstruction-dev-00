//! axum の State Extractor で共有する状態。
//!
//! 「全ハンドラから使いたいもの」（DB 接続プール・アップロード先・設定値）を1つの構造体にまとめ、
//! `Router::with_state(state)` でルーターに登録する。
//! ハンドラは引数に `State<AppState>` と書くだけで、この構造体を受け取れる。
//!
//! 学習メモ:
//! - Rust にはグローバル変数を気軽に書き換える仕組みがない（データ競合を防ぐため）。
//!   そのため「共有したい値は構造体にまとめて、フレームワーク経由で各ハンドラへ渡す」形にする。
//! - axum はリクエストごとに `AppState` を `.clone()` してハンドラへ渡す。
//!   だから `AppState` は `Clone` を実装している必要がある（外すとコンパイルエラーになる）。

use std::path::PathBuf;

use sqlx::SqlitePool;

use crate::config::AppConfig;

/// アプリ全体で共有する状態。
///
/// 学習メモ:
/// - `#[derive(Clone)]` は、全フィールドを `.clone()` する実装を自動生成する。
/// - `SqlitePool` の中身は `Arc`（参照カウント付きの共有ポインタ）なので、
///   clone しても DB 接続が増えるわけではなく、同じプールを指すハンドルが増えるだけ。
/// - `PathBuf` の clone はパス文字列のコピーになるが、短い文字列なので実用上は問題にならない。
#[derive(Clone)]
pub struct AppState {
    /// DB 接続プール
    pub db: SqlitePool,
    /// アップロード画像の保存先ディレクトリ
    pub upload_dir: PathBuf,
    /// ページサイズや画像サイズ上限などの設定値
    pub config: AppConfig,
}
