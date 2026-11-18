---
title: "Rust + axum で画像掲示板を作る (6) Askama で投稿一覧を表示する"
emoji: "📋"
type: "tech"
topics: ["rust", "axum", "askama", "bootstrap", "web"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-05-post-repository.md) は `Post` 構造体と、`post_repo` の `list` / `count` / `find` を書いた。ただし、まだどの URL からも呼ばれていない。今回はそれを HTML にしてブラウザへ返す。

TASK-0006 で画面の器を作り、TASK-0007 で中身を流し込む。

- `askama` と `askama_axum` を追加し、`templates/base.html` と `templates/index.html` を作る
- テンプレート構造体 `IndexView` を定義し、`GET /` が HTML を返すようにする
- ルーターの組み立てを `src/lib.rs` の `build_app` へ移す
- `handlers::posts::list` で `post_repo` を呼び、投稿一覧を描画する
- `AppError` に `IntoResponse` を実装し、DB エラーを 500 にする
- `tests/` に最初の統合テストを置く

一覧は新しい順に1ページ分（20件）だけ出す。`?page=2` のようなページ送りは次回に回す。

## なぜ Askama を選ぶか

Askama は、テンプレートを実行時に解析するのではなく、**コンパイル時に Rust コードへ変換する**。

```toml:Cargo.toml
# askama：Jinja 風の HTML テンプレートを「コンパイル時に」Rust コードへ変換するテンプレートエンジン。
#   テンプレートの構文ミスや存在しない変数の参照は cargo build の時点でエラーになる。
#   拡張子 .html のテンプレートは自動で HTML エスケープされる（XSS 対策）。
askama = "0.12"
# askama_axum：Askama のテンプレート構造体に axum の IntoResponse を実装させる連携クレート。
#   これを入れると、ハンドラはテンプレート構造体をそのまま return するだけで HTML を返せる。
askama_axum = "0.4"
```

この掲示板にとっての利点は2つある。1つ目は、テンプレートの変数名が構造体のフィールド名と照合され、綴りを間違えれば `cargo build` が通らないことだ。2つ目は、HTML エスケープが既定で効くことだ。掲示板は他人が書いた文字列を画面に出すので、エスケープ漏れはそのまま XSS になる。

代わりに、テンプレートを直すたびに再コンパイルが走る。学習用の小さなアプリなので、この待ち時間は気にしないことにした。

## `base.html` と `extends` / `block`

テンプレートは `Cargo.toml` と同じ階層の `templates/` に置く。Askama はここを起点にファイルを探す。まず全ページ共通の外枠 `base.html` を書く。

```html:templates/base.html
<!doctype html>
<html lang="ja">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{% block title %}画像掲示板{% endblock %}</title>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/bootstrap@5.3.2/dist/css/bootstrap.min.css">
</head>
<body>
    <nav class="navbar bg-primary" data-bs-theme="dark">
      <div class="container"><span class="navbar-brand">画像掲示板</span></div>
    </nav>
    <main class="container py-4">
        {% block content %}{% endblock %}
    </main>
</body>
</html>
```

`{% block 名前 %}...{% endblock %}` は「子テンプレートが差し替えてよい穴」の宣言だ。ここでは穴を2つ開けている。

- `title` … ページタイトル。子が差し替えなければ、中に書いた「画像掲示板」がそのまま出る
- `content` … 本文。中身は空で、子が埋める前提

見た目は Bootstrap 5 を CDN から読み込んで整える。CSS を自分で書く手間を省き、Rust の学習に集中するためだ。

次に子テンプレートの `index.html` を書く。TASK-0006 の時点では、投稿フォームと固定文言だけを置いた。

```html:templates/index.html
{% extends "base.html" %}
{% block content %}
<h1 class="h3 mb-4">新規投稿</h1>

<form method="post" action="/posts" enctype="multipart/form-data" class="card card-body mb-4">
  <div class="mb-3">
    <label for="title" class="form-label">タイトル</label>
    <input id="title" name="title" class="form-control" maxlength="50">
  </div>
  <!-- ...（省略：本文の textarea と画像の file 入力も同じ形） -->
  <button class="btn btn-primary">投稿する</button>
</form>

<h2 class="h4 mb-3">投稿一覧</h2>
<p class="text-muted">投稿がまだありません</p>
{% endblock %}
```

1行目の `{% extends "base.html" %}` で「`base.html` を外枠として使う」と宣言する。そのうえで `{% block content %}` に書いた内容が、`base.html` の `content` の穴に入る。`title` ブロックは書いていないので、タイトルは既定の「画像掲示板」のままだ。

フォームの送信先 `/posts` はまだ存在しない。投稿作成は第8回で実装する。タスク文書の HTML からは、各 `<label>` に `for` を、入力欄に同じ値の `id` を足した点だけ変えている。これでラベルをクリックすると入力欄にフォーカスが移る。

## `IndexView` と `#[derive(Template)]`

テンプレートを描画する構造体を Rust 側に用意する。TASK-0006 時点の `src/templates/mod.rs` はこうなっている。

```rust:src/templates/mod.rs
use askama::Template;

/// 一覧画面（トップページ `/`）のビューモデル。
///
/// タスク文書では `IndexTemplate` と呼んでいるもの。設計書 `interfaces.rs` の名前 `IndexView` に合わせた。
/// 投稿一覧やページ情報のフィールドは後続タスクで追加する。
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexView {}
```

`#[derive(...)]` は、型の定義を見てコードを自動生成させる仕組みだ。`#[derive(Template)]` は Askama が提供する derive で、`#[template(path = "index.html")]` で指定したファイルをビルド中に読み込む。そして「この構造体のフィールドを埋め込みながら HTML を組み立てる」Rust コードを生成する。テンプレートの `{{ 変数名 }}` は、構造体の同名フィールドを参照する。

まだテンプレートに変数が無いので、フィールドも無い。名前はタスク文書の `IndexTemplate` ではなく、コメントのとおり設計書に合わせて `IndexView` にした。

### 存在しない変数を書くとビルドが落ちる

`index.html` の見出し「新規投稿」を、構造体に無い変数 `{{ titel }}` へ一時的に書き換えて `cargo build` する（確認後に元に戻した）。

```
error[E0609]: no field `titel` on type `&IndexView`
  --> src/templates/mod.rs:18:10
   |
18 | #[derive(Template)]
   |          ^^^^^^^^ unknown field
```

テンプレートの誤りが、Rust の型エラーとして出てくる。

## `askama_axum` で構造体をそのままレスポンスにする

TASK-0006 時点の `GET /` ハンドラは、`IndexView` を返すだけだ。

```rust:src/lib.rs
async fn root_handler() -> IndexView {
    IndexView {}
}
```

axum のハンドラは、戻り値の型が **`IntoResponse` トレイト** を実装していれば何を返してもよい。トレイトは「この型はこういう操作ができる」という約束事で、`IntoResponse` は「HTTP レスポンスに変換できる」という約束だ。第1回の `"Hello, axum!"` が返せたのも、axum が `&'static str` に `IntoResponse` を実装しているからだった。

`IndexView` の `IntoResponse` は、`askama_axum` を依存に入れたことで `#[derive(Template)]` が一緒に生成する。生成されたコードは `askama_axum::into_response` を呼ぶ。`askama_axum 0.4.0` のソースを読むと、この関数は次のことをしている。

- `render()` でテンプレートを描画し、成功すれば本文にする
- `Content-Type` をテンプレートの種類から決める（`.html` なら `text/html; charset=utf-8`）
- 描画に失敗したら 500 Internal Server Error を返す

そのため、ハンドラには `render()` の呼び出しもヘッダの組み立ても出てこない。この時点で `curl -i http://localhost:3000/` を叩くと、`content-type: text/html; charset=utf-8` の 200 が返り、`<title>画像掲示板</title>` も既定のまま出ていた。

## ルーターを `build_app` へ移す

TASK-0006 ではもう一つ、`main` の中で組み立てていたルーターを `src/lib.rs` の `build_app` 関数へ移した。

理由は、統合テストからルーターを呼ぶためだ。`main` の中でルーターを作ると、それを手に入れるにはサーバを起動するしかない。`build_app` を関数として外へ出しておけば、テストはこの関数でルーターを作り、ポートを listen せずに直接リクエストを流し込める。

TASK-0007 まで終えた時点の `build_app` は次のとおりだ。

```rust:src/lib.rs
pub mod config;
// ...（省略：errors / handlers / models / repository / state / templates も同じく pub mod、続いて use 文）

/// アプリのルーター（URL とハンドラの対応表 + ミドルウェア）を組み立てる。
pub fn build_app(state: AppState) -> Router {
    // `state` をルーターへムーブする前に、必要な値（usize はコピーされる）を取り出しておく
    let max_body_bytes = state.config.max_image_bytes;

    // ...（省略：TraceLayer の説明コメント）
    Router::new()
        .route("/", get(posts::list))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

`src/lib.rs` は前回 `Post` やリポジトリを公開するために作ったファイルだ。今回 `config` / `state` / `templates` / `handlers` も `pub mod` に加え、`main.rs` 側の `mod config;` と `mod state;` は消した。

`build_app` の冒頭で `max_body_bytes` を先に取り出しているのは、所有権の都合だ。最後の `.with_state(state)` で `state` はルーターの中へムーブ（所有権ごと移動）し、それ以降は触れない。だから必要な値はその前に読んでおく。`usize` のような整数型は代入でコピーされるので、取り出した値は `state` と無関係に使い続けられる。

`main.rs` は `let app = build_app(state);` の1行でルーターを受け取るだけになった。`main.rs` から見ると `src/lib.rs` は別のクレートなので、`use rust_axum_image_bbs::{build_app, config::AppConfig, state::AppState};` のようにクレート名で参照する。パッケージ名の `-` は、クレート名では `_` に置き換わる。

## 一覧ハンドラ `handlers::posts::list`

ここから TASK-0007 に入る。`root_handler` を消し、`src/handlers/posts.rs` に一覧用のハンドラを置いた（`src/handlers/mod.rs` は `pub mod posts;` だけ）。

```rust:src/handlers/posts.rs
//! ハンドラの責務は「HTTP の入力を受け取る → リポジトリを呼ぶ → ビューモデルに詰めて返す」だけ。
//! SQL はリポジトリ（`repository::post_repo`）、HTML はテンプレート（`templates/`）に任せる。

// ...（省略：use 文と学習メモのドキュメントコメント）
pub async fn list(State(state): State<AppState>) -> AppResult<IndexView> {
    let page_size = state.config.page_size;
    let posts = post_repo::list(&state.db, page_size, 0).await?;
    let total = post_repo::count(&state.db).await?;
    // 割り算の切り上げ。投稿0件だと 0 ページになるので最低 1 にする（ページネーションは次のタスクで本格化）
    let total_pages = total.div_ceil(page_size).max(1);

    Ok(IndexView {
        posts,
        page: 1,
        total_pages,
    })
}
```

上から順に見ていく。

`State(state): State<AppState>` は第4回で配線した `AppState` の受け取り口だ。左側の `State(state)` はパターンで、`State` という包みを開けて中身を `state` に入れている。`let State(state) = ...;` と同じことを引数の位置でやっている。

`post_repo::list(&state.db, ...)` の `&` は借用だ。接続プールの所有権は `state` が持ったままで、関数には一時的に貸すだけになる。だから続く `post_repo::count(&state.db)` でも同じプールを使える。`offset` は最初のページしか出さないので `0` で固定した。

`post_repo::list(...)` は `async fn` なので、呼んだだけでは実行されない。`.await` で完了を待つと `AppResult<Vec<Post>>` が得られる。そこに付いた `?` は、`Ok` なら中身を取り出し、`Err` ならその場でハンドラから `Err` を返す。

`total.div_ceil(page_size)` は符号なし整数の標準メソッドで、割り算して切り上げる。20件ずつなら3件は1ページ、21件は2ページになる。投稿0件だと `0` になるので、`.max(1)` で最低1ページにそろえる。こうしておけば `page: 1` が総ページ数を超えない。タスク文書では `f32` にキャストして `ceil()` する案だったが、コードは整数のまま計算している。

### 戻り値を `AppResult<IndexView>` にする

戻り値の型は `AppResult<IndexView>`、つまり `Result<IndexView, AppError>` だ。axum は `Result<T, E>` も、`T` と `E` の両方が `IntoResponse` を実装していればレスポンスに変換できる。

- `Ok(IndexView)` … `askama_axum` が生成した実装で HTML になる
- `Err(AppError)` … 次の節で書く `AppError` の実装で 500 になる

タスク文書には「TASK-0012 で `AppError` を導入するまで暫定的に `anyhow::Result` などで凌ぐ可能性あり」という注記があった。実際には前回の TASK-0005 で `AppError` を作ってあったので、`anyhow` は入れず、手元の `AppError` にレスポンスへの変換を足した。戻り値もタスク文書の `impl IntoResponse` ではなく、具体的な `IndexView` と書いている。型を読むだけで、このハンドラが一覧画面を返すと分かる。

## `AppError` に `IntoResponse` を実装する

`Err` 側をレスポンスにするため、`src/errors.rs` に実装を足す。

```rust:src/errors.rs
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

// ...（省略：AppError の定義と From<sqlx::Error> の実装は前回のまま）

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Sqlx(err) => {
                // `%err` は Display 形式でログのフィールドに埋め込む指定（`?err` なら Debug 形式）
                tracing::error!(error = %err, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
            }
        }
    }
}
```

`into_response(self)` は `&self` ではなく `self` を受け取る。値を消費する受け取り方なので、`match self` で取り出した `err` は借用ではなく所有した `sqlx::Error` になる。

利用者に返すのは `500` と決まった文言だけにした。DB のエラー文には SQL 文やファイルパスが混ざることがあり、画面に出すと内部の構造を漏らすことになる。詳細は `tracing::error!` でサーバのログにだけ残す。`(StatusCode, &str)` のタプルにも axum が `IntoResponse` を実装しているので、並べるだけでレスポンスになる。

バリアントがまだ `Sqlx` だけなので、`match` の腕も1本だ。404 やバリデーションエラーは第10回で同じ場所にまとめる。

## テンプレートで `if` / `for` / `match` を書く

ハンドラが詰めるフィールドを `IndexView` に足す。

```rust:src/templates/mod.rs
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexView {
    /// このページに表示する投稿（新しい順）。テンプレートの `{% for post in posts %}` で回す。
    pub posts: Vec<Post>,
    /// 現在のページ番号（1始まり）。
    pub page: u32,
    /// 総ページ数。投稿0件でも 1 にする。
    pub total_pages: u32,
}
```

`page` と `total_pages` はテンプレートでまだ使っていない。次回のページ送りで使う。

`index.html` の「投稿一覧」より下を、固定文言から一覧に書き換える。

```html:templates/index.html
<h2 class="h4 mb-3">投稿一覧</h2>
{% if posts.is_empty() %}
  <p class="text-muted">投稿がまだありません</p>
{% else %}
  <div class="row">
    {% for post in posts %}
    <article class="col-md-6 mb-3">
      <div class="card">
        {% match post.image %}{% when Some with (path) %}
        <img src="/uploads/{{ path }}" class="card-img-top" alt="">
        {% when None %}{% endmatch %}
        <div class="card-body">
          <h3 class="h5 card-title">{{ post.title }}</h3>
          <p class="card-text">{{ post.text }}</p>
          <small class="text-muted">{{ post.regist_date }}</small>
        </div>
      </div>
    </article>
    {% endfor %}
  </div>
{% endif %}
{% endblock %}
```

構文は Jinja 風だが、`{% %}` や `{{ }}` の中には Rust とほぼ同じ書き方で式を書ける。

`{% if posts.is_empty() %}` の `posts` は `IndexView` のフィールド、つまり `Vec<Post>` だ。`is_empty()` は `Vec` の普通のメソッドをそのまま呼んでいる。0件なら「投稿がまだありません」を、1件以上ならカードの並びを出す。

`{% for post in posts %}` は Rust の `for` と同じく、要素を1つずつ `post` に入れて繰り返す。`{{ post.title }}` のようにフィールドへアクセスできる。カードの `col-md-6` は Bootstrap のグリッド指定で、画面幅が md 以上なら2列、それより狭ければ1列になる。

### `{% match %}` で `Option<String>` を出し分ける

画像の列は前回 `image: Option<String>` で受け取るようにした。`None` が画像なし、`Some(ファイル名)` が画像ありだ。

Rust なら `match post.image` で `Some(path)` と `None` の2本の腕に分けるところだ。Askama では `{% match 式 %}` と `{% when パターン %}` で同じ分岐を書く。Rust と違うのは、中身を取り出すときに **`with` キーワード** が要る点だ。`{% when Some with (path) %}` で `Some` の中身を `path` に束縛する。`None` のときは何も出さないので、`{% when None %}` のあとすぐ `{% endmatch %}` で閉じている。

`path` は `Some` の腕の中でしか使えない。そのため「画像なしなのに `<img src="/uploads/">` が出る」という書き間違いが起きにくい。

`regist_date` は `chrono::NaiveDateTime` で、`{{ }}` に書くとこの型の `Display` 実装で `2026-11-17 14:25:21` の形になる。第3回で決めたとおり DB には UTC で保存しているので、表示も UTC のままだ。

## 最初の統合テストを書く

前回までのテストは、リポジトリの関数を直接呼ぶ単体テストだった。今回は `build_app` で作った本物のルーターに HTTP リクエストを投げ、返ってきた HTML を検査する。統合テストは第12回でまとめて扱うので、ここでは仕組みだけを押さえる。

テストだけで使うクレートは `[dev-dependencies]` に書く。本番のバイナリには含まれない。

```toml:Cargo.toml
[dev-dependencies]
# tower：`ServiceExt::oneshot` で、サーバを listen せずにルーターへ直接リクエストを1回投げるために使う。
#   axum 0.7.9 自身が依存している tower 0.5 に揃えて、同じクレートを二重にビルドしないようにする。
tower = { version = "0.5", features = ["util"] }
# tempfile：テストごとに使い捨ての一時ディレクトリ（UPLOAD_DIR 用）を作る。drop 時に自動で削除される。
tempfile = "3"
```

`tests/` 直下の `.rs` ファイルは、それぞれが独立したテスト用クレートとしてビルドされる。共通の準備処理は `tests/common/mod.rs` に置き、各テストファイルから `mod common;` で取り込む。サブディレクトリのファイルはテストクレート扱いされないからだ。

```rust:tests/common/mod.rs
use tower::ServiceExt; // `oneshot` メソッドを使えるようにするトレイト

// ...（省略：他の use と TestApp 構造体）

pub async fn setup() -> TestApp {
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite に接続できること");
    // ...（省略：マイグレーションの適用と、UPLOAD_DIR 用の TempDir の作成）
    let state = AppState {
        db: db.clone(),
        upload_dir: upload_dir.path().to_path_buf(),
        config: AppConfig::from_env_or_default(),
    };
    // ...（省略：build_app(state) で作ったルーターと db を TestApp に詰めて返す）
}

pub async fn get(router: &Router, uri: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}
```

`setup` はテスト1件ごとに、マイグレーション済みのインメモリ SQLite でアプリを組み立てる。ルーターは `main.rs` と同じ `build_app` で作る。

`get` の中心は `oneshot` だ。axum の `Router` は tower の `Service` でもあり、`tower::ServiceExt` トレイトをスコープに入れると `oneshot` メソッドが使える。リクエストを1つ渡してレスポンスを1つ受け取るだけで、ネットワークもポートも使わない。

`tests/integration_list.rs` にはテストを5件置いた。0件の空表示、新しい順の並び、画像ありの投稿だけに出る `<img>`、HTML エスケープ、DB エラー時の 500 だ。最後の1件を載せる。

```rust:tests/integration_list.rs
#[tokio::test]
async fn list_returns_500_when_db_is_unavailable() {
    let app = common::setup().await;
    // プールを閉じると以降の問い合わせはすべて失敗する（DB 障害の再現）
    app.db.close().await;

    let (status, _html) = common::get(&app.router, "/").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
```

`setup` で `AppState` に渡したのは `db.clone()` だった。プールの複製は同じ中身を共有しているので、テスト側で閉じればルーター側も閉じる。ハンドラの `post_repo::list(...).await?` が `Err` になり、`AppError` の `into_response` を通って 500 が返る。

## 動かして確かめる

### テストを流す

```sh
$ cargo test --test integration_list
```

```
running 5 tests
test list_returns_500_when_db_is_unavailable ... ok
test list_escapes_html_in_posts ... ok
test list_shows_empty_message_when_no_posts ... ok
test list_shows_image_only_for_posts_with_image ... ok
test list_shows_posts_newest_first ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### 空の一覧を見る

`data/` を消してからサーバを起動する。

```sh
$ rm -rf data
$ cargo run
```

```
2026-11-17T14:25:16.673677Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T14:25:16.673913Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

別ターミナルから `curl -i http://localhost:3000/` を叩く。

```html
HTTP/1.1 200 OK
content-type: text/html; charset=utf-8
content-length: 1385
date: Tue, 17 Nov 2026 14:25:16 GMT

<!doctype html>
...
<h2 class="h4 mb-3">投稿一覧</h2>

  <p class="text-muted">投稿がまだありません</p>
...
```

投稿が0件なので、`{% if posts.is_empty() %}` の側が出ている。

### 投稿を入れて一覧を見る

投稿作成の画面はまだ無いので、SQLite に直接 `INSERT` する。2件目はタイトルと本文に HTML タグを入れ、3件目は画像ファイル名を持たせる。

```sh
$ sqlite3 data/app.db "INSERT INTO post(title, text) VALUES('hello', 'world');"
$ sqlite3 data/app.db "INSERT INTO post(title, text) VALUES('<b>太字のつもり</b>', '<script>alert(1)</script>');"
$ sqlite3 data/app.db "INSERT INTO post(title, text, image) VALUES('画像つき', '1.jpg を指す投稿', '1.jpg');"
```

サーバは起動したままでよい。もう一度 `curl -i http://localhost:3000/` を叩く。

```html
HTTP/1.1 200 OK
content-type: text/html; charset=utf-8
content-length: 2425
date: Tue, 17 Nov 2026 14:25:21 GMT

...
    <article class="col-md-6 mb-3">
      <div class="card">
        
        <img src="/uploads/1.jpg" class="card-img-top" alt="">
        
        <div class="card-body">
          <h3 class="h5 card-title">画像つき</h3>
...
          <h3 class="h5 card-title">&lt;b&gt;太字のつもり&lt;/b&gt;</h3>
          <p class="card-text">&lt;script&gt;alert(1)&lt;/script&gt;</p>
...
          <h3 class="h5 card-title">hello</h3>
...
```

最後に入れた「画像つき」が先頭に来ている。`post_repo::list` の `ORDER BY id DESC` がそのまま画面の順になっている。

タイトルの `<b>太字のつもり</b>` は `&lt;b&gt;太字のつもり&lt;/b&gt;` になった。ブラウザでは太字にならず、タグを含んだ文字列がそのまま見える。本文の `<script>` も文字列として表示され、実行されない。テンプレートの `{{ post.title }}` には、エスケープのための記述を何も付けていない。

`<img>` は `image` に値がある「画像つき」にだけ出ている。残り2件は `image` が `NULL`、つまり `None` なので、`<img>` の行が無い。

### 画像の URL はまだ 404

`<img>` の `src` にある `/uploads/1.jpg` を叩くと、404 が返る。

```sh
$ curl -i http://localhost:3000/uploads/1.jpg
```

```
HTTP/1.1 404 Not Found
content-length: 0
date: Tue, 17 Nov 2026 14:25:21 GMT
```

ルーターには `/` しか登録していないからだ。テンプレートは `/uploads/ファイル名` を組み立てるだけで、その URL に応える仕組みはまだ無い。`uploads/` の中身を配信する `tower-http` の `ServeDir` は第9回で入れる。

## 詰まったポイントと気づき

- **テンプレートの誤りは derive の行で報告される**: 存在しない変数を書くと、エラーは `templates/index.html` ではなく `src/templates/mod.rs` の `#[derive(Template)]` の行を指す。どのフィールド名が無いかは分かるが、テンプレートの何行目かは出ない。フィールド名でテンプレートを検索して探す
- **タグの位置に空行が残る**: `curl` で見た HTML には、`{% if %}` や `{% for %}` を書いた行がそのまま空行として残っていた。ブラウザの表示には影響しないので、今は気にしないことにした
- **投稿日時は UTC のまま出る**: `regist_date` を `Display` でそのまま出しているので、日本時間より9時間ずれて見える。第3回の「UTC で保存し、表示時に変換する」の後半はまだ手を付けていない
- **インメモリ SQLite は接続を1本にする**: インメモリ DB は接続が全部閉じると中身も消える。前回書いたとおり、sqlx 0.8 では同じ接続プール内の接続は同じ DB を共有するので、`max_connections(1)` は必須ではない。それに頼らず、マイグレーションとハンドラが必ず同じ DB を見るようにするための安全策として付けている
- **`oneshot` はルーターを消費する**: `oneshot` は `self` を受け取るので、そのまま2回投げると1回目でルーターがムーブ済みになる。`get` ヘルパで毎回 `clone()` した複製に投げることで、同じ `TestApp` に何度でもリクエストできる

## 次回予告

第7回はページネーションを実装する。

- `GET /?page=N` でページ番号を受け取れるようにする
- 総ページ数・OFFSET・範囲外のページ番号の丸めを計算する純粋な関数 `paginate` を作り、単体テストで境界値を固める
- テンプレートに「前へ / 次へ」のページ送り（ページャ）を足す

今回 `IndexView` に足したまま使っていない `page` と `total_pages` が、次回ようやく画面に出る。

- 次回: 「Rust + axum で画像掲示板を作る (7) ページネーションを実装する」

## 参考リンク

- [askama 0.12 (docs.rs)](https://docs.rs/askama/0.12.1/askama/) — `Template` トレイトと derive の使い方
- [askama_axum 0.4 (docs.rs)](https://docs.rs/askama_axum/0.4.0/askama_axum/) — Askama と axum の連携クレート
- [axum 0.7: `IntoResponse`](https://docs.rs/axum/0.7/axum/response/trait.IntoResponse.html) — ハンドラの戻り値に使える型
- [tower 0.5: `ServiceExt::oneshot`](https://docs.rs/tower/0.5/tower/trait.ServiceExt.html#method.oneshot)
- [tempfile: `TempDir`](https://docs.rs/tempfile/3/tempfile/struct.TempDir.html)
- [`u32::div_ceil`](https://doc.rust-lang.org/std/primitive.u32.html#method.div_ceil)
- [Bootstrap 5.3: Card](https://getbootstrap.com/docs/5.3/components/card/)
