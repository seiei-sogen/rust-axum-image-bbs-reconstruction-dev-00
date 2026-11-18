//! 投稿エンティティ。旧PHP `post` テーブルと一対一で対応する。
//!
//! 学習メモ:
//! - `#[derive(sqlx::FromRow)]` を付けると、SELECT 結果の1行を「カラム名 = フィールド名」で
//!   この構造体へ自動マッピングしてくれる（`sqlx::query_as::<_, Post>(...)` で使う）。
//! - SQLite には専用の日時型がなく、`regist_date` などは `"2026-05-22 14:30:00"` 形式の TEXT で保存される。
//!   sqlx の `chrono` feature を有効にしているので、TEXT を `chrono::NaiveDateTime` に変換して受け取れる。
//!   `Naive` は「タイムゾーン情報を持たない日時」という意味（SQLite 側も TZ を持たないので対応が素直）。

use chrono::NaiveDateTime;

/// 投稿1件を表す構造体。
///
/// `Clone` を派生しておくと、テンプレートへ渡すときなどに `.clone()` で複製できる。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Post {
    /// 投稿ID（SQLite の AUTOINCREMENT 主キー）。SQLite の INTEGER は 64bit なので `i64`。
    pub id: i64,
    /// タイトル（1〜50文字）。
    pub title: String,
    /// 本文（0〜500文字。空文字を許容）。
    pub text: String,
    /// 画像ファイル名（例: `"5.jpg"`）。画像なし投稿は DB 上 NULL なので `Option<String>` で受ける。
    /// `None` = 画像なし、`Some(name)` = 画像あり、を型で表現できるのが Rust の強み。
    pub image: Option<String>,
    /// 登録日時（UTC で保存されている）。
    pub regist_date: NaiveDateTime,
    /// 更新日時（UTC で保存されている）。
    pub update_date: NaiveDateTime,
}
