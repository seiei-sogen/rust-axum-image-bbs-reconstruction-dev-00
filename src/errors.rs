//! アプリ共通のエラー型 `AppError` と、その `Result` 別名 `AppResult` を定義するモジュール。
//!
//! ハンドラ・リポジトリ・ストレージは、失敗をすべて `AppError` で返す。
//! HTTP のステータスとエラーページへの変換は、このファイルの `IntoResponse` 実装1か所だけで行う。
//!
//! | バリアント        | ステータス | 主な原因                                       |
//! |-------------------|------------|------------------------------------------------|
//! | `Validation`      | 400        | 入力値の不備・壊れたフォーム                   |
//! | `NotFound`        | 404        | 存在しないページ・投稿                         |
//! | `PayloadTooLarge` | 413        | 送信データがボディ上限を超えた                 |
//! | `Io`              | 500        | 画像ファイルの読み書き失敗                     |
//! | `Sqlx`            | 500        | DB 操作の失敗                                  |
//! | `Other`           | 500        | 上のどれにも当てはまらない想定外の失敗         |
//!
//! 学習メモ:
//! - Rust のエラー処理は `Result<T, E>` 型が基本。成功なら `Ok(T)`、失敗なら `Err(E)` を返す。
//! - `?` 演算子は「`Err(e)` なら `From::from(e)` で呼び出し元のエラー型へ変換して即 return する」糖衣構文。
//!   下の `#[from]` が `impl From<sqlx::Error> for AppError` などを自動生成するおかげで、
//!   `AppResult` を返す関数の中で `sqlx` や `tokio::fs` の呼び出しに `?` を付けるだけで `AppError` に包まれる。
//! - `impl IntoResponse for AppError` があるので、ハンドラは `AppResult<T>` をそのまま返せる。
//!   axum は `Ok(T)` なら `T` を、`Err(AppError)` なら下の `into_response` の結果をレスポンスにする。

use axum::extract::multipart::{MultipartError, MultipartRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

use crate::templates::ErrorView;

/// multipart を読み取れなかったときに利用者へ見せるメッセージ。
const FORM_UNREADABLE: &str = "フォームの送信内容を読み取れませんでした";

/// アプリ全体で使うエラー型。
///
/// `enum`（列挙型）にしておくと、「どの種類の失敗か」を `match` で網羅的に分岐できる。
///
/// 学習メモ:
/// - `#[derive(Error)]`（thiserror）は、`#[error("...")]` の文面で `Display`（`{}` での表示）を実装し、
///   `std::error::Error` トレイトも実装する。手で書くと数十行になる定型コードが消える。
/// - `#[from]` を付けたフィールドからは `From` 実装が自動生成される。これが `?` での自動変換の正体。
/// - `#[derive(Debug)]` はテストの `unwrap()` 失敗時やログ出力で中身を表示するために必要
///   （`std::error::Error` を実装する型は Debug も必須）。
#[derive(Debug, Error)]
pub enum AppError {
    /// 入力値のバリデーション失敗。利用者に見せるメッセージを複数まとめて持つ。
    ///
    /// 投稿フォームのハンドラはこれを受け取ったら、入力値を残したままフォームを 400 で再表示する。
    /// ハンドラが拾わずにそのまま返した場合（壊れた multipart など）は、エラーページの 400 になる。
    #[error("validation failed: {0:?}")]
    Validation(Vec<String>),

    /// ページや投稿が見つからない（404）。
    #[error("not found")]
    NotFound,

    /// リクエストのボディが上限（`build_app` の `DefaultBodyLimit`）を超えた（413）。
    #[error("payload too large")]
    PayloadTooLarge,

    /// ファイル I/O の失敗（画像の保存・削除）。500。
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// DB 操作の失敗。sqlx が返したエラーをそのまま包んで持ち運ぶ。500。
    #[error("db error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// その他の想定外の失敗。500。
    /// `transparent` は「表示文も原因も、包んだエラーのものをそのまま使う」という指定。
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// multipart の読み取り途中の失敗 → `AppError`。
///
/// ボディが上限を超えて読み取りが打ち切られた場合、axum は `err.status()` に 413 を入れてくる。
/// それ以外（区切り線が壊れている・途中で切れている等）は、送り手の不備として 400 にする。
/// 中身を見て振り分けるので、`#[from]` ではなく `From` を手で実装している。
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

/// `AppError` を HTTP レスポンス（ステータス + 簡素な HTML のエラーページ）へ変換する。
///
/// 学習メモ:
/// - `self` を値で受け取る（所有権を消費する）ので、中の `Vec<String>` をコピーせずに取り出せる。
/// - `err @ (A | B | C)` は「A・B・C のどれかにマッチしたら、値全体を `err` という名前で受け取る」書き方。
/// - DB やファイルの生のエラー文には SQL やファイルパスが含まれうるので、利用者には出さずログにだけ残す。
///   500 系のときだけ `tracing::error!` を出すのは、4xx は利用者側の問題でサーバの異常ではないから。
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, messages) = match self {
            AppError::Validation(messages) => (StatusCode::BAD_REQUEST, messages),
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                vec!["ページが見つかりません".to_string()],
            ),
            AppError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                vec!["送信データが大きすぎます。画像は5MB以内にしてください".to_string()],
            ),
            err @ (AppError::Io(_) | AppError::Sqlx(_) | AppError::Other(_)) => {
                // `%err` は Display 形式（上の `#[error(...)]` の文面）でログのフィールドに埋め込む指定
                tracing::error!(error = %err, "internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    vec![
                        "サーバでエラーが発生しました。時間をおいてもう一度お試しください"
                            .to_string(),
                    ],
                )
            }
        };
        // `(ステータス, テンプレート)` のタプルも IntoResponse を実装している。
        // ErrorView は Askama のテンプレートなので、メッセージは自動で HTML エスケープされる。
        (status, ErrorView { status, messages }).into_response()
    }
}

/// アプリ全体の標準 `Result` 型。
///
/// 毎回 `Result<Vec<Post>, AppError>` と書く代わりに `AppResult<Vec<Post>>` と書ける。
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::CONTENT_TYPE;
    use std::io::ErrorKind;

    /// エラーをレスポンスに変換し、ステータス・Content-Type・本文を取り出す。
    async fn render(err: AppError) -> (StatusCode, String, String) {
        let response = err.into_response();
        let status = response.status();
        let content_type = response.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            content_type,
            String::from_utf8(bytes.to_vec()).unwrap(),
        )
    }

    /// `?` で std::io::Error が AppError に変わることを確かめるための関数。
    fn read_protected_file() -> AppResult<()> {
        Err(std::io::Error::from(ErrorKind::PermissionDenied))?;
        Ok(())
    }

    /// `?` で anyhow::Error が AppError に変わることを確かめるための関数。
    fn do_something_unexpected() -> AppResult<()> {
        Err(anyhow::anyhow!("secret detail"))?;
        Ok(())
    }

    #[tokio::test]
    async fn app_error_validation_returns_400_with_escaped_messages() {
        let err = AppError::Validation(vec!["タイトルは必須です".into(), "<b>太字</b>".into()]);

        let (status, content_type, body) = render(err).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(content_type.starts_with("text/html"));
        assert!(body.contains("タイトルは必須です"));
        assert!(body.contains("&lt;b&gt;太字&lt;/b&gt;") && !body.contains("<b>太字"));
    }

    #[tokio::test]
    async fn app_error_not_found_returns_404() {
        let (status, content_type, body) = render(AppError::NotFound).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(content_type.starts_with("text/html"));
        assert!(body.contains("ページが見つかりません"));
    }

    #[tokio::test]
    async fn app_error_payload_too_large_returns_413() {
        let (status, _, body) = render(AppError::PayloadTooLarge).await;

        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body.contains("5MB"));
    }

    #[tokio::test]
    async fn app_error_sqlx_returns_500_without_details() {
        let (status, _, body) = render(AppError::from(sqlx::Error::RowNotFound)).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("サーバでエラーが発生しました"));
        assert!(!body.contains(&sqlx::Error::RowNotFound.to_string()));
    }

    #[tokio::test]
    async fn app_error_io_returns_500() {
        let err = read_protected_file().unwrap_err();
        assert!(matches!(err, AppError::Io(_)));

        let (status, _, _) = render(err).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn app_error_other_returns_500_without_details() {
        let err = do_something_unexpected().unwrap_err();
        assert!(matches!(err, AppError::Other(_)));

        let (status, _, body) = render(err).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!body.contains("secret detail"));
    }
}
