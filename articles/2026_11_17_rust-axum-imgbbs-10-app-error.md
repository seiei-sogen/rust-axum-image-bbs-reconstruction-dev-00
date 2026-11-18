---
title: "Rust + axum で画像掲示板を作る (10) AppError と IntoResponse でエラー処理をまとめる"
emoji: "🧯"
type: "tech"
topics: ["rust", "axum", "thiserror", "askama", "web"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-09-image-upload.md) は画像を `{id}.{ext}` で保存して `ServeDir` で配信し、保存に失敗したら投稿を手で取り消すところまで進めた。今回は、第5回から少しずつ育ててきたエラー型 `AppError` を仕上げる。

- `thiserror` で `AppError` を書き直し、手書きの `From` を `#[from]` に置き換える
- バリアントを `Validation` / `NotFound` / `PayloadTooLarge` / `Io` / `Sqlx` / `Other` の6種にそろえる
- `IntoResponse` を1つの `match` にまとめ、どのエラーも Askama の `error.html` で返す
- 未定義のパスを Router の `fallback` で 404 ページにする
- 単体テスト6件と統合テスト4件を足す

コミットは `TASK-0012: AppError を thiserror で完成させ、HTML のエラーページと 404 fallback を追加`（`7738d4b`）の1つだ。

## AppError のこれまで

`AppError` は必要になった回で1つずつバリアントを足してきた。`git log --oneline -- src/errors.rs` で追うと、今回を含めて6つのコミットがある。

| 追加したもの | 回 | コミット | ステータス |
|---|---|---|---|
| `Sqlx` と手書きの `From<sqlx::Error>` | 第5回 | `116208c`（TASK-0005） | （まだ変換なし） |
| `IntoResponse`（`Sqlx` を平文の 500 に） | 第6回 | `c7331d2`（TASK-0007） | 500 |
| `Validation(Vec<String>)` | 第8回 | `1c59298`（TASK-0009） | 400 |
| `PayloadTooLarge` と multipart の `From` 2つ | 第8回 | `7e5d6a6`（TASK-0010） | 413 / 400 |
| `Io` と手書きの `From<std::io::Error>` | 第9回 | `a149314`（TASK-0011） | 500 |
| `NotFound` / `Other`、`thiserror` 化 | 今回 | `7738d4b`（TASK-0012） | 404 / 500 |

前回の時点（`a149314`）の定義はこうだった。

```rust:src/errors.rs（a149314 時点）
#[derive(Debug)]
pub enum AppError {
    Validation(Vec<String>),
    PayloadTooLarge,
    Io(std::io::Error),
    Sqlx(sqlx::Error),
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Sqlx(err)
    }
}

// ...（省略：From<MultipartError> と From<MultipartRejection>）

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            // メッセージは利用者向けの固定文なので、そのまま返してよい
            AppError::Validation(messages) => {
                (StatusCode::BAD_REQUEST, messages.join("\n")).into_response()
            }
            // ...（省略：PayloadTooLarge は平文の 413）
            AppError::Io(err) => {
                // ファイルパスなどの内部情報は利用者に見せず、ログにだけ残す
                tracing::error!(error = %err, "io error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
            }
            // ...（省略：Sqlx も同じ形で、ログの文面だけ "database error"）
        }
    }
}
```

動いてはいたが、気になる点が3つ残っていた。

1つ目は、包むだけの `From` がバリアントの数だけ並ぶことだ。型名が違うほかは同じ5行になる。

2つ目は、`AppError` 自身に `Display` も `std::error::Error` も無いことだ。ログには中身を取り出して渡すしかなく、500 系のアームがバリアントごとに分かれている。

3つ目は、利用者に返す本文だ。`a149314` で未定義のパスと、multipart でない POST を投げるとこうなる。

```sh
$ curl -i http://localhost:3000/no-such-page
HTTP/1.1 404 Not Found
content-length: 0
date: Tue, 17 Nov 2026 14:48:24 GMT

$ curl -i -X POST -H 'Content-Type: application/json' -d '{"title":"hello"}' http://localhost:3000/posts
HTTP/1.1 400 Bad Request
content-type: text/plain; charset=utf-8
content-length: 60
date: Tue, 17 Nov 2026 14:48:24 GMT

フォームの送信内容を読み取れませんでした
```

404 は axum の既定の応答で本文が空、400 は `text/plain` の1行だけで、ナビゲーションも「一覧へ戻る」リンクも無い。

## `thiserror` で AppError を書き直す

`Cargo.toml` に2つ足す。

```toml:Cargo.toml
# thiserror：エラー型の定型コード（Display / std::error::Error / From の実装）を derive で自動生成する。
#   `#[error("...")]` で表示文、`#[from]` で `?` 用の From 変換が作られる。
thiserror = "1"
# anyhow：どんなエラーでも1つの型 `anyhow::Error` に包めるクレート。
#   AppError::Other に入れて、専用のバリアントを作るまでもない想定外のエラーの受け皿にする。
anyhow = "1"
```

`thiserror` は設計書 `architecture.md` の依存表で `1.x` と決めてあり、`Cargo.lock` では `1.0.69` に解決された。完成した `AppError` はこうなる。

```rust:src/errors.rs
use axum::extract::multipart::{MultipartError, MultipartRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

use crate::templates::ErrorView;

/// multipart を読み取れなかったときに利用者へ見せるメッセージ。
const FORM_UNREADABLE: &str = "フォームの送信内容を読み取れませんでした";

/// アプリ全体で使うエラー型。
///
/// 学習メモ:
/// - `#[derive(Error)]`（thiserror）は、`#[error("...")]` の文面で `Display`（`{}` での表示）を実装し、
///   `std::error::Error` トレイトも実装する。手で書くと数十行になる定型コードが消える。
/// - `#[from]` を付けたフィールドからは `From` 実装が自動生成される。これが `?` での自動変換の正体。
#[derive(Debug, Error)]
pub enum AppError {
    /// 入力値のバリデーション失敗。利用者に見せるメッセージを複数まとめて持つ。
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
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

`#[derive(Debug, Error)]` の `Error` が `thiserror` の **derive マクロ** で、`Debug` と同じく型の定義からトレイトの実装をコンパイル時に生成する。生成されるのは次の3つだ。

| 書くもの | 生成されるもの |
|---|---|
| `#[error("...")]` | `impl Display for AppError`（`{}` で表示したときの文面） |
| `#[derive(Error)]` そのもの | `impl std::error::Error for AppError` |
| フィールドの `#[from]` | `impl From<そのフィールドの型> for AppError` |

### `#[error("...")]` と `Display`

`Display` は `{}` で表示するためのトレイトで、`format!("{}", err)` や `err.to_string()` がこれを使う。`#[error("...")]` の文字列が、そのバリアントの表示文になる。

`{0}` は「タプルの0番目のフィールドを `Display` で埋め込む」、`{0:?}` は「`Debug` で埋め込む」という意味だ。`Validation` の中身の `Vec<String>` は `Display` を実装していないので、`{0:?}` にしている。`Other` の `#[error(transparent)]` は文面を書かず、表示文も後述の `source()` も、包んだ `anyhow::Error` のものをそのまま使う指定だ。

:::message
タスク文書の実装例では `Io` と `Sqlx` も `#[error(transparent)]` だったが、このプロジェクトは `io error: {0}` / `db error: {0}` と接頭辞を付けた。今回は 500 系のログの文面を `"internal server error"` に一本化したので、種類は `error=` フィールドの接頭辞で読み分ける。
:::

### `std::error::Error`

`std::error::Error` は「これはエラーを表す型だ」という印のトレイトで、`Debug` と `Display` の両方を実装していることが条件だ。`Error` と一緒に `Debug` も derive しているのはこのためだ。`thiserror` はこのトレイトの `source()`（原因になった下位のエラーを返すメソッド）も実装し、README によれば `#[from]` を付けたフィールドがその原因になる。今回のコードは `source()` を直接は呼ばない。

### `#[from]` と `From`

いちばん効くのが `#[from]` だ。前回は `Io(std::io::Error)` の定義とは別に、`impl From<std::io::Error> for AppError` の5行を手で書いていた。今回は `Io(#[from] std::io::Error)` とフィールドに付けるだけで、同じ `From` が生成される。`Sqlx` と、今回増えた `Other` も同じだ。なお README によると、`#[from]` を付けたバリアントは元のエラー以外のフィールドを持てない。「1つの型を包むだけ」のバリアント専用の道具である。

### 手書きのまま残した `From`

第8回の `From<MultipartError>` と `From<MultipartRejection>` は、手書きのまま残している。

```rust:src/errors.rs
/// multipart の読み取り途中の失敗 → `AppError`。
///
/// ボディが上限を超えて読み取りが打ち切られた場合、axum は `err.status()` に 413 を入れてくる。
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
impl From<MultipartRejection> for AppError {
    fn from(_err: MultipartRejection) -> Self {
        AppError::Validation(vec![FORM_UNREADABLE.to_string()])
    }
}
```

`#[from]` が作れるのは「受け取った値をそのバリアントに入れる」変換だけだ。`MultipartError` は中のステータスを見て2つに振り分け、`MultipartRejection` は中身を捨てて固定文にする。どちらも包むだけではないので、手で書くしかない。同じ enum の中で `#[from]` と手書きの `From` は混ぜて使える。

## `?` は `From` を通って AppError になる

`Result` を返す関数の中で `式?` と書くと、次のように動く。

1. 式が `Ok(v)` なら、`v` を取り出して先へ進む
2. 式が `Err(e)` なら、`From::from(e)` で関数の戻り値のエラー型に変換し、`return Err(..)` する

2 で使われるのが、`#[from]` で生成された `From` だ。`AppResult<T>`（`Result<T, AppError>` の別名）を返す関数の中では、`From` 実装がある型のエラーなら何にでも `?` を付けられる。

画像の保存では、`tokio::fs` の `std::io::Error` が `AppError::Io` になる。

```rust:src/storage/image_store.rs
pub async fn save_image(upload_dir: &Path, id: i64, image: &ValidatedImage) -> AppResult<String> {
    // 起動時にも作っているが、実行中に消された場合に備えて毎回作る（既にあれば何もしない）
    tokio::fs::create_dir_all(upload_dir).await?;
    let filename = file_name(id, image.extension());
    tokio::fs::write(upload_dir.join(&filename), image.bytes()).await?;
    Ok(filename)
}
```

`.await?` は、非同期処理の完了を待ってからその `Result` に `?` を適用する並びだ。リポジトリ層の `post_repo::count` などでは、同じように `fetch_one(db).await?` が `sqlx::Error` を `AppError::Sqlx` にしている。投稿作成のハンドラの `multipart?` は手書きの `From<MultipartRejection>` を通るが、`?` から見れば生成された `From` との区別は無い。

`?` を使わずに変換したいときは `.into()` を呼ぶ（`From<A> for B` があれば `Into<B> for A` は標準ライブラリが用意する）。`delete_image` の `return Err(err.into())` がその例だ。`From` 実装が無い型に `?` を付けるとコンパイルエラーになるので、変換の抜けは実行前に見つかる。

## IntoResponse を1つの `match` にまとめる

`AppError` を HTTP レスポンスに変えるのは、`src/errors.rs` のこの実装1か所だけだ。ハンドラは `AppResult<T>` を返すだけで、axum が `Err(AppError)` のときにこの `into_response` を呼ぶ。

```rust:src/errors.rs
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
```

前回はアームごとに `.into_response()` を呼んでいた。今回の `match` はステータスと利用者向けメッセージを決めるだけで、レスポンスを作るのは最後の1行に集めた。どのエラーも同じ `ErrorView` を通るので、見た目がそろう。

Rust の `match` は値を返す **式** で、選ばれたアームの値が左辺の `(status, messages)` に入る。だからどのアームも同じ型 `(StatusCode, Vec<String>)` を返す。`NotFound` の固定文をわざわざ `vec![...to_string()]` にしているのはこのためだ。`match` は網羅的でないとコンパイルが通らないので、バリアントを足せば、ここにアームを書くまでビルドが失敗する。

500 系の3つは `err @ (...)` で1つにまとめた。中身を取り出さず `AppError` のまま受け取るので、`%err` で `#[error(...)]` の文面をログに出せる。前回は `Display` が無く、これができなかった。`%` は `Display` 形式、`?` は `Debug` 形式の指定で、違いは後で実際のログで見る。利用者への本文は固定文だけで、`err` の中身は入れない。

## エラーページは Askama のテンプレートで描く

エラーページ用のビューモデル `ErrorView` を足した。

```rust:src/templates/mod.rs
/// エラーページ（400 / 404 / 413 / 500）のビューモデル。`AppError` の `IntoResponse` から使う。
#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorView {
    /// HTTP ステータス。テンプレートでは `404 Not Found` のように表示される（StatusCode の Display）。
    pub status: StatusCode,
    /// 利用者に見せるメッセージ。内部の詳細（SQL やファイルパス）は入れない。
    pub messages: Vec<String>,
}
```

テンプレートは第6回の `base.html` を継承するので、ナビゲーションも Bootstrap も一覧画面と同じになる。

```html:templates/error.html
{% extends "base.html" %}
{% block title %}{{ status }} | 画像掲示板{% endblock %}
{% block content %}
<h1 class="h3 mb-3">{{ status }}</h1>
<div class="alert alert-danger" role="alert">
  <ul class="mb-0">
    {% for m in messages %}<li>{{ m }}</li>{% endfor %}
  </ul>
</div>
<a href="/" class="btn btn-secondary">一覧へ戻る</a>
{% endblock %}
```

`askama_axum` がテンプレート構造体に `IntoResponse` を実装している（第6回）。それ単体では 200 になるので、`(status, ErrorView { .. })` とタプルの先頭に `StatusCode` を置いて上書きする。

タスク文書の実装例は、HTML を `format!("<li>{m}</li>")` のように組み立てていた（リポジトリには入っていないコードだ）。`format!` は文字列をつなぐだけなので、メッセージに `<script>` が入っていればそのまま HTML になる。今の `Validation` のメッセージはバリデータの固定文だけだが、入力値を含む文面を足した時点で XSS の穴になる。Askama は `.html` のテンプレートで `{{ ... }}` を自動でエスケープするので、メッセージがどこから来ても `<` は `&lt;` として出る。

## Validation が IntoResponse まで届くとき

`AppError::Validation` は、ふだんは `IntoResponse` まで届かない。投稿のハンドラが途中で受け止めるからだ。

```rust:src/handlers/posts.rs
    // `validate` は入力を借用（&input）するだけなので、失敗したときも `input` はまだ手元にある
    let form = match validate(&input, &state.config) {
        Ok(form) => form,
        Err(AppError::Validation(errors)) => {
            // multipart は一度読んだら読み直せないので、さっき作った `input` をそのままフォームに戻す
            let view = IndexView {
                form: input,
                errors,
                ..index_view(&state, 1).await?
            };
            return Ok((StatusCode::BAD_REQUEST, view).into_response());
        }
        Err(err) => return Err(err),
    };
```

第8回のとおり、入力値を残したまま一覧画面を 400 で描き直す。エラーページに飛ばすと、利用者は入力をやり直すことになる。ここは `Ok(...)` で返すので、`into_response` の `Validation` アームは通らない。

アームに届くのは、`?` で上がってくる multipart の失敗だ。Content-Type が `multipart/form-data` でない（`MultipartRejection`）か、区切り線が壊れている（413 以外の `MultipartError`）ときで、フォームを1欄も読めていないので残すべき入力値も無い。だからエラーページの 400 にする。前回までは `text/plain` の1行だったところだ。今後のハンドラが `Validation` を受け止め忘れても、500 ではなくエラーページの 400 になる。

## 未定義パスを fallback で 404 にする

`build_app` に1行足した。

```rust:src/lib.rs
    Router::new()
        .route("/", get(posts::list))
        .route("/posts", post(posts::create))
        .nest_service("/uploads", uploads)
        // どのルートにも当たらない未定義パスは、AppError::NotFound の 404 ページにする。
        // `.layer()` は「それより前に登録したもの」にだけかかるので、fallback は layer より前に置く。
        .fallback(|| async { AppError::NotFound })
        // multipart の読み取りはこの上限で打ち切られる（超えたら 413）
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
```

`|| async { AppError::NotFound }` は、引数なしで非同期に `AppError::NotFound` を返すクロージャだ。axum のハンドラは `IntoResponse` を実装した値を返せばよいので、`AppError` をそのまま返せる。置き場所にも意味がある。`Router::layer` のドキュメントには「ミドルウェアは既にあるルートにだけかかるので、ルート（と fallback）を先に足してから `layer` を呼ぶ」とある。`fallback` を後ろに書くと、未定義パスに `TraceLayer` がかからない。

`fallback` で拾えるのは「どのルートにも当たらない」リクエストだけだ。次の2つは別の部品が応答を返すので、HTML のエラーページにならない。

- `GET /posts`: パスはあるがメソッドが違う。`Router::fallback` のドキュメントのとおり fallback は呼ばれず、axum が本文なしの 405 を返す
- `GET /uploads/999.png`: `/uploads` 以下は `ServeDir` に丸ごと渡しているので、`ServeDir` が本文なしの 404 を返す

設計書のエラー表も 405 は「axum 標準」で、このプロジェクトは2つともそのままにした。そろえるなら、axum 0.7.9 の `Router::method_not_allowed_fallback` と、tower-http 0.5 の `ServeDir::not_found_service` が使える。

## anyhow はどこで使い、どこで使わないか

`anyhow::Error` はどんなエラーでも1つの型に包めるが、包んだ後は元の型が見えない。`into_response` は失敗の種類で 400 と 500 を出し分けるので、種類を `match` できる `enum` の `AppError` は残し、`anyhow::Error` で置き換えない。

`anyhow` を使ったのは `AppError::Other(#[from] anyhow::Error)` だけで、専用のバリアントを作るほどではない失敗の受け皿だ。ただしアプリのコードに `Other` を作る箇所はまだ無く、通るのは単体テストだけだ。`main.rs` の戻り値も第3回からの `Result<(), Box<dyn std::error::Error>>` のままで、設計書の依存表が `anyhow` の用途に挙げる「初期化エラー」には使っていない。

## テストを書く

### AppError の単体テスト

`src/errors.rs` の末尾に6件置いた。

:::details src/errors.rs のテスト（全6件）
```rust:src/errors.rs
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
```
:::

1件目はメッセージに `<b>太字</b>` を混ぜ、本文が `&lt;b&gt;太字&lt;/b&gt;` になっていることでエスケープを確かめる。`read_protected_file` / `do_something_unexpected` は `Err(...)?` と書くための小さな関数で、`?` が `AppError::Io` / `AppError::Other` を作ることを `matches!`（値がパターンに当たれば `true` を返すマクロ）で確かめる。500 系は、元のエラーの文面が本文に出ていないことも見ている。

### エラー応答の統合テスト

`tests/integration_errors.rs` で、ルーター全体を通したエラー応答を4件確かめる。

:::details tests/integration_errors.rs
```rust:tests/integration_errors.rs
//! エラー応答（404 / 400 / 500）の統合テスト。
//!
//! どの失敗も `AppError` を通って、同じ見た目の HTML エラーページになることを確かめる。

mod common;

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};

#[tokio::test]
async fn unknown_path_returns_404_html_page() {
    let app = common::setup().await;

    let res = common::send(
        &app.router,
        Request::get("/no-such-page").body(Body::empty()).unwrap(),
    )
    .await;

    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert!(res.content_type.unwrap().starts_with("text/html"));
    assert!(res.body.contains("ページが見つかりません"));
    assert!(res.body.contains(r#"href="/""#), "トップへ戻るリンクがある");
}

#[tokio::test]
async fn missing_upload_returns_404() {
    let app = common::setup().await;

    let res = common::send(
        &app.router,
        Request::get("/uploads/999.png")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn non_multipart_post_returns_400_error_page() {
    let app = common::setup().await;
    let request = Request::post("/posts")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"hello"}"#))
        .unwrap();

    let res = common::send(&app.router, request).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST);
    assert!(res.content_type.unwrap().starts_with("text/html"));
    assert!(res
        .body
        .contains("フォームの送信内容を読み取れませんでした"));
}

#[tokio::test]
async fn db_failure_returns_500_page_without_internal_details() {
    let app = common::setup().await;
    // プールを閉じると以降の問い合わせはすべて失敗する（DB 障害の再現）
    app.db.close().await;

    let res = common::send(&app.router, Request::get("/").body(Body::empty()).unwrap()).await;

    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(res.content_type.unwrap().starts_with("text/html"));
    assert!(res.body.contains("サーバでエラーが発生しました"));
    assert!(!res.body.to_lowercase().contains("pool"));
}
```
:::

`missing_upload_returns_404` は、本文が空の `ServeDir` の 404 なのでステータスだけを見る。`db_failure_...` は第6回と同じくプールを閉じて DB 障害を再現し、`sqlx::Error` の表示文に含まれる pool という語が本文に無いことを確かめる。このファイルは `tests/common/mod.rs` の `get` を使わないので、未使用の警告を避けるため `get` に `#[allow(dead_code)]` を足した。

### 実行する

```sh
$ cargo test errors
...
running 7 tests
test errors::tests::app_error_not_found_returns_404 ... ok
test errors::tests::app_error_payload_too_large_returns_413 ... ok
test errors::tests::app_error_io_returns_500 ... ok
test errors::tests::app_error_other_returns_500_without_details ... ok
test errors::tests::app_error_sqlx_returns_500_without_details ... ok
test errors::tests::app_error_validation_returns_400_with_escaped_messages ... ok
test validators::post_form::tests::all_errors_are_returned_together ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 37 filtered out; finished in 0.00s
...
     Running tests/integration_errors.rs (target/debug/deps/integration_errors-ffd4644cbc351f6a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s
...
```

6件は通ったが、バリデータのテストが混ざり、`integration_errors.rs` の4件は `filtered out` になった。`cargo test` の引数は **テスト名** の部分一致で絞り込むもので、ファイル名では絞らないからだ。統合テストのファイルを丸ごと走らせるには `--test` でファイル名を指定する。

```sh
$ cargo test --test integration_errors
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.20s
     Running tests/integration_errors.rs (target/debug/deps/integration_errors-ffd4644cbc351f6a)

running 4 tests
test db_failure_returns_500_page_without_internal_details ... ok
test unknown_path_returns_404_html_page ... ok
test non_multipart_post_returns_400_error_page ... ok
test missing_upload_returns_404 ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
```

## 動かして確かめる

データを消してから起動する。

```sh
$ rm -rf data uploads
$ cargo run
...
2026-11-17T14:45:24.444435Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T14:45:24.444870Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

### 未定義パス（404）

```sh
$ curl -i http://localhost:3000/no-such-page
HTTP/1.1 404 Not Found
content-type: text/html; charset=utf-8
content-length: 762
date: Tue, 17 Nov 2026 14:45:30 GMT

<!doctype html>
...
    <title>404 Not Found | 画像掲示板</title>
...
<h1 class="h3 mb-3">404 Not Found</h1>
<div class="alert alert-danger" role="alert">
  <ul class="mb-0">
    <li>ページが見つかりません</li>
  </ul>
</div>
<a href="/" class="btn btn-secondary">一覧へ戻る</a>
...
```

前回は `content-length: 0` だった応答が、`base.html` を継承した HTML になった。`404 Not Found` は `StatusCode` の `Display` だ。サーバのログは `TraceLayer` の DEBUG 2行（`status=404`）だけで、ERROR は出ない。

### 6MB の画像（413）

先頭だけ PNG のマジックバイトにした 6MB のファイルを、フォームと同じ形で送る。

```sh
$ python3 -c "open('big.png','wb').write(b'\x89PNG\r\n\x1a\n'+b'\0'*(6*1024*1024-8))"
$ curl -i -F title=big -F image=@big.png http://localhost:3000/posts
HTTP/1.1 100 Continue

HTTP/1.1 413 Payload Too Large
content-type: text/html; charset=utf-8
content-length: 823
date: Tue, 17 Nov 2026 14:45:34 GMT

<!doctype html>
...
<h1 class="h3 mb-3">413 Payload Too Large</h1>
...
    <li>送信データが大きすぎます。画像は5MB以内にしてください</li>
...
```

`100 Continue` は、大きなボディを送る前に curl が `Expect: 100-continue` で許可を求めた応答だ。ボディが `DefaultBodyLimit` を超えたところで multipart の読み取りが失敗し、`From<MultipartError>` が `PayloadTooLarge` に振り分けている。

### 画像の保存失敗（500）

次の投稿の画像ファイル名 `1.png` で **ディレクトリ** を先に作っておく。DB をまっさらにしたので、次の投稿の id は 1 だ。ファイルを書く場所にディレクトリがあれば、`tokio::fs::write` は失敗する。

```sh
$ printf '\x89PNG\r\n\x1a\nsmall' > small.png
$ mkdir -p uploads/1.png
$ curl -i -F title=hello -F image=@small.png http://localhost:3000/posts
HTTP/1.1 500 Internal Server Error
content-type: text/html; charset=utf-8
content-length: 849
date: Tue, 17 Nov 2026 14:45:38 GMT

<!doctype html>
...
<h1 class="h3 mb-3">500 Internal Server Error</h1>
...
    <li>サーバでエラーが発生しました。時間をおいてもう一度お試しください</li>
...
```

本文は固定文だけで、`Is a directory` のような原因は入っていない。原因はサーバのログに出る。

```
2026-11-17T14:45:38.691423Z DEBUG request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_request: started processing request
2026-11-17T14:45:38.694188Z  WARN request{method=POST uri=/posts version=HTTP/1.1}: rust_axum_image_bbs::handlers::posts: image could not be saved, rolling back the post post_id=1
2026-11-17T14:45:38.694586Z  WARN request{method=POST uri=/posts version=HTTP/1.1}: rust_axum_image_bbs::handlers::posts: rollback: failed to remove image file post_id=1 error=Io(Os { code: 21, kind: IsADirectory, message: "Is a directory" })
2026-11-17T14:45:38.696221Z ERROR request{method=POST uri=/posts version=HTTP/1.1}: rust_axum_image_bbs::errors: internal server error error=io error: Is a directory (os error 21)
2026-11-17T14:45:38.696325Z DEBUG request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=4 ms status=500
2026-11-17T14:45:38.696347Z ERROR request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_failure: response failed classification=Status code: 500 Internal Server Error latency=4 ms
```

- 1つ目の WARN は前回の `rollback_create` が取り消しを始めた記録だ
- 2つ目の WARN は、ディレクトリの `1.png` を `remove_file` で消せなかった記録で、わざと作った状況のせいだ。行の削除は続けて行われ、`post` テーブルの行数は 0 だった
- `rust_axum_image_bbs::errors` の ERROR が、今回 `into_response` に書いた `tracing::error!` だ
- 最後の ERROR は `TraceLayer` の `on_failure` で、応答が 5xx だった記録だ

`%` と `?` の違いもここで見比べられる。`into_response` は `error = %err` なので、`#[error("io error: {0}")]` の文面どおり `io error: Is a directory (os error 21)` と出る。`rollback_create` は `error = ?err` なので、`Debug` 形式の `Io(Os { code: 21, ... })` になる。

確認が終わったら `rmdir uploads/1.png` で消しておく。

### メソッド違い（405）とファイルの無い画像（404）

最後に、fallback を通らない2つを見る。

```sh
$ curl -i http://localhost:3000/posts
HTTP/1.1 405 Method Not Allowed
allow: POST
content-length: 0
date: Tue, 17 Nov 2026 14:45:30 GMT

$ curl -i http://localhost:3000/uploads/999.png
HTTP/1.1 404 Not Found
content-length: 0
date: Tue, 17 Nov 2026 14:45:30 GMT

```

どちらも本文が空だ。405 には、受け付けるメソッドを示す `allow: POST` が付いている。

## 詰まったポイントと気づき

- **前回までの `AppError` には `Display` が無かった**: 500 系のアームをバリアントごとに分けて中身を取り出していた。`thiserror` で `Display` が付き、1つにまとめられた
- **`#[from]` は包むだけの変換にしか使えない**: 中身で振り分ける `From<MultipartError>` は手書きのまま残した
- **タスク文書の `format!` の例はエスケープしない**: `error.html` に置き換え、`<b>太字</b>` のエスケープをテストで固定した
- **fallback は `layer` より前に書く**: `Router::layer` は呼んだ時点で登録済みのものにだけかかる
- **`cargo test errors` は統合テストを走らせない**: 引数はファイル名ではなくテスト名の部分一致だ
- **500 では ERROR が2行出る**: `AppError` と `TraceLayer` の `on_failure` の2つで、原因は `rust_axum_image_bbs::errors` の行にある
- **`Cargo.lock` に `thiserror` が2つ並ぶ**: アプリが入れたのは 1.0.69 だが、`sqlx-core` 0.8.6 が 2.0.18 に依存している（`cargo tree -i thiserror@2.0.18` で確かめた）。メジャーバージョンが違えば別のクレートとして両方ビルドされるので、ビルドは通る

## 次回予告

第11回は、編集と削除を足して CRUD を完成させる。

- 編集フォームを作る
- 編集の送信で、タイトルと本文の更新、画像の差し替え、画像だけの削除を扱う
- 投稿を削除するときに、画像ファイルも一緒に消す
- 存在しない id には、今回そろえた `AppError::NotFound` で 404 ページを返す

今回は fallback からしか使っていない `NotFound` を、ハンドラの中から返すことになる。

- 次回: 「Rust + axum で画像掲示板を作る (11) 編集と削除で CRUD を完成させる」

## 参考リンク

- [thiserror 1.x (docs.rs)](https://docs.rs/thiserror/1/thiserror/) — `#[error]` / `#[from]` / `#[error(transparent)]`
- [anyhow (docs.rs)](https://docs.rs/anyhow/1/anyhow/)
- [axum 0.7 `IntoResponse` (docs.rs)](https://docs.rs/axum/0.7/axum/response/trait.IntoResponse.html)
- [axum 0.7 `Router` (docs.rs)](https://docs.rs/axum/0.7/axum/struct.Router.html) — `fallback` / `layer` / `method_not_allowed_fallback`
- [tower-http 0.5 `ServeDir` (docs.rs)](https://docs.rs/tower-http/0.5/tower_http/services/struct.ServeDir.html)
- [askama 0.12 (docs.rs)](https://docs.rs/askama/0.12/askama/)
- [The Rust Programming Language: `?` 演算子](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html#a-shortcut-for-propagating-errors-the--operator)
