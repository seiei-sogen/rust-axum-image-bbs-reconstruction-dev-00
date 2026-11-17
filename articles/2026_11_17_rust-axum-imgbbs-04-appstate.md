---
title: "Rust + axum で画像掲示板を作る (4) AppConfig / AppState と環境変数読み込み"
emoji: "🧰"
type: "tech"
topics: ["rust", "axum", "dotenvy", "sqlx", "web"]
published: false
---

## はじめに

[前回](./2026_05_25_rust-axum-imgbbs-03-sqlx-migration.md) は SQLite の接続プールを作り、起動時にマイグレーションを流すところまで進めた。ただし接続プールは `main` の中に置いたままで、ハンドラからは使えなかった。今回はそれを **ハンドラまで届ける配線** を作る。

具体的にはこの5つを進める。

- `dotenvy` を追加し、起動直後に `.env` を読み込む
- `src/config.rs` に、ページングと画像アップロードの設定値を集めた `AppConfig` を作る
- `src/state.rs` に、接続プール・アップロード先・`AppConfig` を束ねた `AppState` を作る
- `PORT` と `UPLOAD_DIR` を環境変数から読むようにする
- `Router::with_state()` で `AppState` を配線し、`DefaultBodyLimit` で 5MB の上限を付ける

一つ断っておく。前回の予告では「`DATABASE_URL` や `PORT` や `UPLOAD_DIR` を構造体にまとめる」と書いたが、実際のコードはそうなっていない。`AppConfig` が持つのは `page_size` / `max_image_bytes` / `allowed_mime` の3つで、`PORT` / `DATABASE_URL` / `UPLOAD_DIR` は `main` で直接 `std::env::var` で読んでいる。

## `dotenvy` を追加する

`Cargo.toml` の末尾に1行足す。

```toml:Cargo.toml
# dotenvy：ローカル開発用の .env を起動時に読み込む。
dotenvy = "0.15"
```

`dotenvy` は `.env` ファイルの `KEY=VALUE` をプロセスの環境変数に流し込むクレートだ。読み込んだあとは、普通に `std::env::var` で値を取り出せる。

## `AppConfig` と `AppState` の責務を分ける

新しく作る型は2つある。

| 型 | 中身 | 性質 |
|---|---|---|
| `AppConfig` | 1ページの件数、画像の最大バイト数、許可する MIME タイプ | 起動後に変わらない値だけ |
| `AppState` | 接続プール、アップロード先ディレクトリ、`AppConfig` | ハンドラが仕事に使う道具一式 |

`AppConfig` は **ルール** を、`AppState` は外部へ触りに行くための **道具** を持つ。ルールは単体テストで値を確かめられるので、道具と混ぜずに独立させておく。

### `src/config.rs`

```rust:src/config.rs
//! アプリ全体の設定値。
//!
//! 現時点ではページングと画像アップロードの安全なデフォルト値を集約する。

const DEFAULT_PAGE_SIZE: u32 = 20;
const DEFAULT_MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const DEFAULT_ALLOWED_MIME: &[&str] = &["image/jpeg", "image/gif", "image/png"];

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub page_size: u32,
    pub max_image_bytes: usize,
    pub allowed_mime: &'static [&'static str],
}

impl AppConfig {
    pub fn from_env_or_default() -> Self {
        Self {
            page_size: DEFAULT_PAGE_SIZE,
            max_image_bytes: DEFAULT_MAX_IMAGE_BYTES,
            allowed_mime: DEFAULT_ALLOWED_MIME,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn default_config_matches_task_limits() {
        let config = AppConfig::from_env_or_default();

        assert_eq!(config.page_size, 20);
        assert_eq!(config.max_image_bytes, 5 * 1024 * 1024);
        assert_eq!(
            config.allowed_mime,
            ["image/jpeg", "image/gif", "image/png"]
        );
    }
}
```

**`&'static [&'static str]` は「プログラムが終わるまで生きている文字列の配列への参照」だ。** 文字列リテラルはバイナリに埋め込まれ、プロセスが動いている間ずっと存在する。それを表すライフタイムが `'static` だ。`Vec<String>` で持つと起動時にヒープへコピーが走るが、中身が固定なら参照で足りる。参照のコピーは安いので、`AppConfig` の `Clone` も安い。

`#[derive(Debug, Clone)]` は、`{:?}` で表示する `Debug` と、値を複製する `Clone` の2つのトレイトの実装をコンパイラに書かせる。`Clone` が要る理由は後の節で書く。フィールドの `pub` は、ハンドラから `state.config.page_size` と読むために要る。Rust の構造体のフィールドは既定で非公開だ。

タスクファイルには「`cargo test --doc` で書く程度は許容」とあったが、実装は `#[cfg(test)]` の普通の単体テストにした。このクレートは `lib.rs` を持たないバイナリクレートなので、doc テストを書いても実行されない。

:::message
関数名は `from_env_or_default` だが、**今のところ環境変数は一切読んでいない**。中身は3つの `const` を詰めて返すだけだ。タスクファイルの完了条件も「デフォルト値関数 `AppConfig::from_env_or_default()` がある」で、名前は将来の拡張の余地を残したものと読める。今回「環境変数から読む」のは、`main` が直接読む `PORT` / `UPLOAD_DIR` / `DATABASE_URL` だけである。
:::

### `src/state.rs`

```rust:src/state.rs
//! axum の State Extractor で共有する状態。

use std::path::PathBuf;

use sqlx::SqlitePool;

use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub upload_dir: PathBuf,
    pub config: AppConfig,
}
```

- `db: SqlitePool` … 前回 `main` の中に作った接続プール
- `upload_dir: PathBuf` … 画像の保存先。`PathBuf` は所有権を持つパス型で、`String` と `&str` の関係のように、借用版の `&Path` と対になる
- `config: AppConfig` … 前の節で作った設定値

`use crate::config::AppConfig;` の `crate` はこのクレートのルート、つまり `main.rs` を指す。`main.rs` に `mod config;` と書いたので、`src/config.rs` は `crate::config` というモジュールになる。

## `axum::extract::State` と `Clone` の関係

ハンドラが `AppState` を受け取る仕組みは、axum の **`State` エクストラクタ** だ。`State<AppState>` を引数に書いたハンドラが呼ばれるたびに、axum は Router が持っている `AppState` を **`clone()` して** 渡す。axum-core のソースでは、状態の型がそのまま欲しい場合の取り出しは `input.clone()` を返すだけの実装だ。だから `AppState` には `Clone` が要る。`Router::new()` 自体も、状態の型に `Clone + Send + Sync + 'static` を要求している。

では毎リクエストの `clone()` は重くないのか。フィールドごとに見るとこうなる。

| フィールド | `clone()` で起きること |
|---|---|
| `db: SqlitePool` | 内部の `Arc` の参照カウントを1つ増やすだけ。接続は複製されない |
| `upload_dir: PathBuf` | パス文字列をヒープ上にコピーする |
| `config: AppConfig` | 数値2つと参照1つのコピー |

sqlx のドキュメントにも「`Pool` の clone は参照カウント付きハンドルを作るだけなので安い」とある。複数のハンドラが同時に動いても、全員が同じ接続プールを共有する。タスクファイルの例にあった「Clone は内部で Arc が回るので安価」は接続プールについては正しいが、`PathBuf` は文字列のコピーを伴う。短いパス1本なので、この規模なら気にする量ではないと判断した。

:::message
設計ドキュメントは当初 `Arc<AppState>` を渡す方針だったが、TASK-0004 のコミットで `AppState` をそのまま渡す形に書き換えた。コミットに理由はない。私の理解では、接続プールが内部で `Arc` を持つので、外側をもう一度包む必要が薄い。
:::

## `dotenvy` で `.env` を読み込むタイミング

`main.rs` の冒頭はこうなった。

```rust:src/main.rs
mod config;
mod state;

use axum::{extract::DefaultBodyLimit, extract::State, routing::get, Router};
use config::AppConfig;
// ...（省略）
use state::AppState;
use std::path::PathBuf;
// ...（省略）

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // ---------- ロギング初期化 ----------
    // ...（省略。前回までと同じ）
```

`mod config;` と `mod state;` は2つのファイルをモジュールとして取り込む宣言だ。Rust はファイルを置いただけではコンパイルしない。

`dotenvy::dotenv().ok();` は **ロギング初期化より前** に置く。理由は `RUST_LOG` だ。`EnvFilter::try_from_default_env()` はその時点の `RUST_LOG` を読むので、`.env` の読み込みが後だと `.env` に書いた `RUST_LOG` が反映されない。この動きは後で確かめる。

`dotenvy::dotenv()` の挙動は、ソースのドキュメントによると次のとおりだ。

- カレントディレクトリから親ディレクトリへ順に `.env` を探す
- 同じ名前の環境変数が **すでにあれば上書きしない**
- `.env` が見つからなければ `Err` を返す

戻り値は `Result` だが、ここでは `.ok()` で `Option` に変えて捨てている。つまり **`.env` が無くても、書式が壊れていても、エラーは黙って無視される**。`.env` を置かない環境でもそのまま起動するための選択で、タスクファイルにも「学習用途として許容。本番運用なら `.expect(".env not found")` も検討」とある。`.ok()` を付けずに `dotenvy::dotenv();` とだけ書くと ``unused `Result` that must be used`` という警告が出るので、`.ok()` は「承知の上で無視する」という意思表示でもある。

## 環境変数のフォールバックパターン

`DATABASE_URL` の読み取りは前回のままだ。今回は `UPLOAD_DIR` と `PORT` が加わった。

```rust:src/main.rs
let upload_dir =
    PathBuf::from(std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string()));
std::fs::create_dir_all(&upload_dir)?;
```

`UPLOAD_DIR` は `DATABASE_URL` と同じ形で、未設定なら `"uploads"` になる。続く `create_dir_all` で、前回 `data/` で踏んだ「ディレクトリが無くて落ちる」問題を最初から避けておく。

引数が `&upload_dir` と **借用** になっている点に注目したい。そのまま渡すと所有権が関数へ移り（ムーブ）、この後で `AppState` に入れられなくなる。`&` を付けて「貸すだけ」にしているので、所有権は `upload_dir` 変数に残る。

```rust:src/main.rs
let port: u16 = std::env::var("PORT")
    .ok()
    .and_then(|value| value.parse().ok())
    .unwrap_or(3000);
let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
```

`PORT` は文字列を数値に変える必要があるので少し長い。1段ずつ追う。

1. `std::env::var("PORT")` … `Result<String, VarError>`
2. `.ok()` … `Option<String>` に変える。未設定なら `None`
3. `.and_then(|value| value.parse().ok())` … `Some` のときだけ数値に変換する。失敗したら `None`
4. `.unwrap_or(3000)` … `None` なら 3000

`value.parse()` は変換先の型を書いていないが、左辺の `let port: u16` から `u16`（0〜65535 の整数）だと型推論される。`3000` はただの整数なので `unwrap_or` に直接渡す。一方 `"uploads".to_string()` はヒープ確保を伴うので、失敗したときだけ呼ばれる `unwrap_or_else` のクロージャに入れている。

`bind` の引数は `(&str, u16)` のタプルだ。tokio はこれをアドレスとして受け取れるので、`format!` で文字列を組み立てなくて済む。

## `AppState` を組み立てて `Router::with_state()` で配線する

マイグレーションの後に、`AppState` の組み立てとルーターへの配線が続く。

```rust:src/main.rs
let config = AppConfig::from_env_or_default();
let max_body_bytes = config.max_image_bytes;
let state = AppState {
    db: pool,
    upload_dir,
    config,
};

// AppState は Router::with_state で各ハンドラへ共有する。

// ...（ルーター組み立てのコメントは省略）
let app: Router = Router::new()
    .route("/", get(root_handler))
    .layer(DefaultBodyLimit::max(max_body_bytes))
    .layer(TraceLayer::new_for_http())
    .with_state(state);
```

`AppState { db: pool, upload_dir, config }` で、3つの変数の所有権が `state` に移る。`upload_dir` と `config` はフィールド名と変数名が同じなので、`upload_dir: upload_dir` を省略して書ける。この後で `pool` や `config` を使うと「ムーブ済みの値を使った」というコンパイルエラーになる。その手前の `max_body_bytes` は、上限値を先に取り出している。`usize` は `Copy` を実装した型なので、この代入は値のコピーで、`config` の所有権は動かない。

### `with_state()` は最後に呼ぶ

`Router` は状態の型を型パラメータに持つ。`get(root_handler)` を登録した時点で、このルーターは「`AppState` を渡されないと動かない」`Router<AppState>` になる。`.with_state(state)` で実物を渡すと、もう状態を要求しない `Router<()>` に変わる。`let app: Router` の `Router` は `Router<()>` の省略形だ。

`.with_state(state)` を書き忘れると、型が合わずにコンパイルエラーになる。実際に外してビルドした出力から抜き出すとこうなる。

```
error[E0308]: mismatched types
...
    = note: expected struct `Router<()>`
               found struct `Router<AppState>`
```

タスクファイルには「Router 構築の最後で `.with_state()` を呼ぶこと」とある。試しに `.with_state(state)` を `.layer(...)` より前へ移すと、この構成ではビルドは通った。それでも最後に置くのは、`with_state()` の後に足したルートは `AppState` を受け取れないからだ。

## `DefaultBodyLimit` で 5MB の上限を用意する

axum のドキュメントによると、`Bytes` エクストラクタは既定で **2MB を超えるボディを受け付けない**。内部で `Bytes` を使う `String` / `Json` / `Form` も同じだ。画像は 5MB まで受け付ける要件なので、`.layer(DefaultBodyLimit::max(max_body_bytes))` で必要な分だけ上限を広げる。

タスクファイルの例は `DefaultBodyLimit::max(5 * 1024 * 1024)` と数値を直接書いていた。実装では `AppConfig` の `max_image_bytes` を使う。画像の上限と HTTP ボディの上限が同じ値から来るので、片方だけ書き換えて食い違う事故が起きない。

:::message
`DefaultBodyLimit` は、それを適用するエクストラクタにだけ効く。今のハンドラはボディを読まないので、**今回のコードでは上限の効果を観察できない**。axum の `Multipart` はこの上限を適用するので、画像を受け取るハンドラを書いた時点で働き始める。
:::

## ハンドラで `State<AppState>` を受け取る

配線を確かめるために、`root_handler` で `AppState` を受け取るようにした。

```rust:src/main.rs
async fn root_handler(State(state): State<AppState>) -> &'static str {
    tracing::debug!(
        db_pool = ?state.db,
        upload_dir = %state.upload_dir.display(),
        page_size = state.config.page_size,
        max_image_bytes = state.config.max_image_bytes,
        allowed_mime = ?state.config.allowed_mime,
        "root handler received application state"
    );

    "Hello, axum!"
}
```

引数の `State(state): State<AppState>` は **パターンによる分解** だ。`State<AppState>` は `AppState` を1つ包んだ構造体なので、引数の位置で `State(state)` と書くと、中身を `state` という名前で受け取れる。

`tracing::debug!` の `名前 = 値` は構造化フィールドで、`?` は `Debug` で、`%` は `Display` で表示する。`PathBuf` は `Display` を実装していないので `.display()` を挟む。`AppState` には `Debug` が無いので、フィールドを1つずつ並べている。

## 動かして確かめる

### `RUST_LOG=debug` で起動してリクエストを送る

`.env.example` は `PORT` のコメントだけが実装に合わせて変わった。

```sh:.env.example
# サーバーのリッスンポート番号。
# 未設定または不正値の場合は 3000 を使う。
PORT=3000
```

`.env` は `.gitignore` 済みなので、リポジトリには `.env.example` だけを置き、各自がコピーして `.env` を作る。相対パスはカレントディレクトリ基準で解決されるので、プロジェクトルートで実行する。`RUST_LOG=debug` を付けて起動する。

```sh
$ cp .env.example .env
$ RUST_LOG=debug cargo run
```

別ターミナルから `curl` で叩く。

```sh
$ curl -i http://localhost:3000/
HTTP/1.1 200 OK
content-type: text/plain; charset=utf-8
content-length: 12
date: Tue, 17 Nov 2026 14:10:17 GMT

Hello, axum!
```

サーバ側のログはこうなった。`RUST_LOG=debug` だと sqlx が発行した SQL も DEBUG で出る。マイグレーションの SQL 全文を含んで長いので、その行だけ省いた。

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running `target/debug/rust-axum-image-bbs`
...（sqlx::query の DEBUG 行を省略）
2026-11-17T14:10:16.906136Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T14:10:16.906325Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
2026-11-17T14:10:17.748438Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: tower_http::trace::on_request: started processing request
2026-11-17T14:10:17.748519Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: rust_axum_image_bbs: root handler received application state db_pool=Pool { size: 1, num_idle: 1, is_closed: false, options: PoolOptions { max_connections: 5, min_connections: 0, connect_timeout: 30s, max_lifetime: Some(1800s), idle_timeout: Some(600s), test_before_acquire: true } } upload_dir=uploads page_size=20 max_image_bytes=5242880 allowed_mime=["image/jpeg", "image/gif", "image/png"]
2026-11-17T14:10:17.748570Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=0 ms status=200
```

`root handler received application state` の行が、今回の配線の確認になる。

- `db_pool=Pool { size: 1, ... max_connections: 5 ... }` … 前回 `main` で作った接続プールがハンドラまで届いている。開いている接続は1本で、上限は前回設定した5本
- `upload_dir=uploads` … `.env` の `UPLOAD_DIR` の値。コード内の既定値と同じなので、`.env` が効いているかはまだ区別できない
- `page_size=20` / `max_image_bytes=5242880` / `allowed_mime=[...]` … `AppConfig` の既定値。5242880 は 5 × 1024 × 1024 だ

### `.env` の値を変えて起動し直す

`.env` が本当に読まれているかを確かめる。`UPLOAD_DIR` を書き換え、`RUST_LOG` も `.env` に書く（コメント行は省いて示す）。

```sh:.env
DATABASE_URL=sqlite://data/app.db
PORT=3000
UPLOAD_DIR=uploads_dotenv
RUST_LOG=info,tower_http=debug,rust_axum_image_bbs=debug
```

今度はシェルで何も指定せずに `cargo run` で起動し、`curl http://localhost:3000/` を送る。`root_handler` の行はこうなった（前後の行と、1回目と同じ `db_pool` の中身は `...` で省いた）。

```
2026-11-17T14:10:29.410363Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: rust_axum_image_bbs: root handler received application state db_pool=Pool { ... } upload_dir=uploads_dotenv page_size=20 max_image_bytes=5242880 allowed_mime=["image/jpeg", "image/gif", "image/png"]
```

確認できたことは2つある。

1. `upload_dir=uploads_dotenv` になっている。`.env` の値が `std::env::var` から読めている
2. シェルで `RUST_LOG` を指定していないのに、`root_handler` の DEBUG ログが出ている。`dotenvy::dotenv()` をロギング初期化より前に置いた効果だ

このときのログに sqlx の DEBUG 行は無かった。`.env` の `RUST_LOG` で `rust_axum_image_bbs` と `tower_http` だけを DEBUG にしたからだ。プロジェクトルートには `uploads_dotenv/` もできていた。

### シェルの環境変数は `.env` より強い

`.env` はそのままで、シェルから `UPLOAD_DIR` を渡して起動する。

```sh
$ UPLOAD_DIR=uploads_shell cargo run
```

`curl` を送ったときの `root_handler` の行はこうなった（同じく省略あり）。

```
2026-11-17T14:10:30.424082Z DEBUG request{method=GET uri=/ version=HTTP/1.1}: rust_axum_image_bbs: root handler received application state db_pool=Pool { ... } upload_dir=uploads_shell page_size=20 max_image_bytes=5242880 allowed_mime=["image/jpeg", "image/gif", "image/png"]
```

`.env` には `uploads_dotenv` と書いてあるが、シェルで渡した `uploads_shell` が勝っている。`dotenvy::dotenv()` は既存の環境変数を上書きしないので、優先順位は「シェルの環境変数 > `.env` > コード内の既定値」になる。`.env` を書き換えても効かないときは、以前 `export` した値がシェルに残っていないかを疑うとよい。

### 単体テスト

```sh
$ cargo test
```

```
running 1 test
test config::tests::default_config_matches_task_limits ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## 詰まったポイントと気づき

- **`#[derive(Clone)]` を外したときの最初のエラーが分かりにくい**: 外してビルドするとエラーが2つ出る。1つ目は `.route(...)` の行で「`root_handler` が `Handler` を満たさない」という、`Clone` の話が出てこないメッセージだ。原因が分かるのは2つ目で、`#[derive(Clone)]` を付けるよう `help` まで出る。`Handler` のエラーが出たら続きまで読むとよい

```
error[E0277]: the trait bound `fn(State<AppState>) -> impl Future<Output = &'static str> {root_handler}: Handler<_, _>` is not satisfied
...
error[E0277]: the trait bound `AppState: Clone` is not satisfied
...
help: consider annotating `AppState` with `#[derive(Clone)]`
```

- **`from_env_or_default` は環境変数を読まない**: 名前から期待すると外れる。`PAGE_SIZE=50` を `.env` に書いても何も変わらない
- **`PORT` の不正値は黙って 3000 になる**: `PORT=abc` や `PORT=70000` は `parse` に失敗し、何も言わずに 3000 で起動するコードだ。仕様どおりだが打ち間違いに気づきにくいので、起動ログの `server listening on ...` でポートを確かめるとよい
- **古いコメントが残っている**: この時点の `main.rs` には実装と合わないコメントが3か所ある。`PORT` 読み取りの直前の「0.0.0.0:3000 で TCP リスナを開く」、`DATABASE_URL` の箇所の「今回はシンプルに std::env で取得する」、接続プールの箇所の「`AppState { db_pool: SqlitePool, ... }`」（実際は `db`）だ。記事を書くために読み返して気づいた

## 次回予告

第5回は `post` テーブルを Rust から読む。

- `Post` 構造体を定義し、`post` テーブルの1行を Rust の型として表す
- リポジトリモジュールに `list` / `count` / `find` を書き、接続プール（`SqlitePool`）を受け取って SQL を実行する
- インメモリ SQLite に対してテストを書き、DB アクセスの部分だけを単体で確かめる

今回で Phase 1a「プロジェクト基盤」は完了だ。次回から一覧表示に向けた実装に入る。

- 次回: 「Rust + axum で画像掲示板を作る (5) Post モデルとリポジトリ層を sqlx で書く」

## 参考リンク

- [axum 0.7: `State` エクストラクタ](https://docs.rs/axum/0.7/axum/extract/struct.State.html) — 「When states need to implement `Clone`」の節
- [axum 0.7: `Router::with_state`](https://docs.rs/axum/0.7/axum/struct.Router.html#method.with_state)
- [axum 0.7: `DefaultBodyLimit`](https://docs.rs/axum/0.7/axum/extract/struct.DefaultBodyLimit.html)
- [dotenvy crate (docs.rs)](https://docs.rs/dotenvy/0.15/dotenvy/) — `dotenv()` と `dotenv_override()` の違い
- [sqlx: `Pool`](https://docs.rs/sqlx/0.8/sqlx/struct.Pool.html) — `Clone` が安価である理由
- [`std::env::var`](https://doc.rust-lang.org/std/env/fn.var.html)
