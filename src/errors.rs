//! アプリ共通のエラー型 `AppError` と、その `Result` 別名 `AppResult` を定義するモジュール。
//!
//! 現時点では DB 操作の失敗（`sqlx::Error`）、入力値のバリデーション失敗、送信サイズの超過を扱う。
//! 画像 I/O・404 などのバリアントは、必要になったタスクで順に追加していく。
//!
//! 学習メモ:
//! - Rust のエラー処理は `Result<T, E>` 型が基本。成功なら `Ok(T)`、失敗なら `Err(E)` を返す。
//! - `?` 演算子は「`Err(e)` なら `From::from(e)` で呼び出し元のエラー型へ変換して即 return する」糖衣構文。
//!   下の `impl From<sqlx::Error> for AppError` があるおかげで、
//!   `AppResult` を返す関数の中で `sqlx` の呼び出しに `?` を付けるだけで `AppError::Sqlx` に包まれる。
//! - `impl IntoResponse for AppError` があるので、ハンドラは `AppResult<T>` をそのまま返せる。
//!   axum は `Ok(T)` なら `T` を、`Err(AppError)` なら下の `into_response` の結果をレスポンスにする。

use axum::extract::multipart::{MultipartError, MultipartRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// multipart を読み取れなかったときに利用者へ見せるメッセージ。
const FORM_UNREADABLE: &str = "フォームの送信内容を読み取れませんでした";

/// アプリ全体で使うエラー型。
///
/// `enum`（列挙型）にしておくと、「どの種類の失敗か」を `match` で網羅的に分岐できる。
/// `#[derive(Debug)]` はテストの `unwrap()` 失敗時やログ出力で中身を表示するために必要。
#[derive(Debug)]
pub enum AppError {
    /// 入力値のバリデーション失敗。利用者に見せるメッセージを複数まとめて持つ。
    ///
    /// 投稿フォームのハンドラはこれを受け取ったら、入力値を残したままフォームを 400 で再表示する。
    /// ハンドラが拾わずにそのまま返した場合は、下の `into_response` で 400 になる。
    Validation(Vec<String>),
    /// リクエストのボディが上限（`build_app` の `DefaultBodyLimit`）を超えた（413）。
    PayloadTooLarge,
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

/// multipart の読み取り途中の失敗 → `AppError`。
///
/// ボディが上限を超えて読み取りが打ち切られた場合、axum は `err.status()` に 413 を入れてくる。
/// それ以外（区切り線が壊れている・途中で切れている等）は、送り手の不備として 400 にする。
impl From<MultipartError> for AppError {
    fn from(err: MultipartError) -> Self {
        if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::PayloadTooLarge
        } else {
            AppError::Validation(vec![FORM_UNREADABLE.to_string()])
        }
    }
}

/// `Multipart` 抽出器そのものの失敗（Content-Type が multipart/form-data でない等）→ 400。
///
/// 学習メモ: `_err` の先頭の `_` は「受け取るが使わない」印。付けないと未使用変数の警告が出る。
impl From<MultipartRejection> for AppError {
    fn from(_err: MultipartRejection) -> Self {
        AppError::Validation(vec![FORM_UNREADABLE.to_string()])
    }
}

/// `AppError` を HTTP レスポンスへ変換する。
///
/// 学習メモ:
/// - `self` を値で受け取る（所有権を消費する）ので、中の `sqlx::Error` を取り出してそのまま使える。
/// - DB の生のエラー文には SQL やファイルパスが含まれうるので、利用者には出さずログにだけ残す。
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            // メッセージは利用者向けの固定文なので、そのまま返してよい
            AppError::Validation(messages) => {
                (StatusCode::BAD_REQUEST, messages.join("\n")).into_response()
            }
            AppError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "画像サイズは5MB以内にしてください",
            )
                .into_response(),
            AppError::Sqlx(err) => {
                // `%err` は Display 形式でログのフィールドに埋め込む指定（`?err` なら Debug 形式）
                tracing::error!(error = %err, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
            }
        }
    }
}

/// アプリ全体の標準 `Result` 型。
///
/// 毎回 `Result<Vec<Post>, AppError>` と書く代わりに `AppResult<Vec<Post>>` と書ける。
pub type AppResult<T> = Result<T, AppError>;
