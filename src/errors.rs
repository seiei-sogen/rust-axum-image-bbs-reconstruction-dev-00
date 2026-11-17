//! アプリ共通のエラー型 `AppError` と、その `Result` 別名 `AppResult` を定義するモジュール。
//!
//! 現時点では DB 操作の失敗（`sqlx::Error`）だけを扱う。
//! 画像 I/O・バリデーション・404 などのバリアントは、必要になったタスクで順に追加していく。
//!
//! 学習メモ:
//! - Rust のエラー処理は `Result<T, E>` 型が基本。成功なら `Ok(T)`、失敗なら `Err(E)` を返す。
//! - `?` 演算子は「`Err(e)` なら `From::from(e)` で呼び出し元のエラー型へ変換して即 return する」糖衣構文。
//!   下の `impl From<sqlx::Error> for AppError` があるおかげで、
//!   `AppResult` を返す関数の中で `sqlx` の呼び出しに `?` を付けるだけで `AppError::Sqlx` に包まれる。

/// アプリ全体で使うエラー型。
///
/// `enum`（列挙型）にしておくと、「どの種類の失敗か」を `match` で網羅的に分岐できる。
/// `#[derive(Debug)]` はテストの `unwrap()` 失敗時やログ出力で中身を表示するために必要。
#[derive(Debug)]
pub enum AppError {
    /// DB 操作の失敗。sqlx が返したエラーをそのまま包んで持ち運ぶ。
    Sqlx(sqlx::Error),
}

/// `sqlx::Error` → `AppError` への変換。
///
/// 学習メモ:
/// - 引数 `err` は値渡し（所有権ごと受け取る）。受け取った `err` は `AppError::Sqlx` の中へ
///   ムーブされるので、コピーは発生しない。
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Sqlx(err)
    }
}

/// アプリ全体の標準 `Result` 型。
///
/// 毎回 `Result<Vec<Post>, AppError>` と書く代わりに `AppResult<Vec<Post>>` と書ける。
pub type AppResult<T> = Result<T, AppError>;
