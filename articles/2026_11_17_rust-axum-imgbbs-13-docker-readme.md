---
title: "Rust + axum で画像掲示板を作る (13) Docker と README で配布できる形にする"
emoji: "🐳"
type: "tech"
topics: ["rust", "axum", "docker", "tracing", "sqlite"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-12-integration-test.md) は統合テストで主要シナリオを固め、`scripts/e2e.sh` を2つ目のテストの層として整えた。最終回の今回は、手元にない人がそのまま動かせる形にする。

- multi-stage の `Dockerfile` で、実行用イメージにバイナリ1つだけを入れる
- `docker-compose.yml` で DB と画像をホストに置き、コンテナを消しても投稿が残るようにする
- `.dockerignore` を「必要なものだけ通す」書き方にする
- ログに `post_id` とリクエストごとのステータスを出し、`LOG_FORMAT=json` で JSON に切り替えられるようにする
- `README.md` を、clone した人が最初に読む入口にする

コミットは `81d23f4` と `fe2baf3`（TASK-0019）、`4dc73a3`（TASK-0020）だ。連載の最後なので、コードはその後の修正（`288b627` / `ba2ccf7`）も入ったブランチの先端から引用する。最後の節で13回分を振り返る。

## 実行用イメージにはバイナリ1つだけを入れる

```dockerfile:Dockerfile
# syntax=docker/dockerfile:1
#
# rust-axum-image-bbs のコンテナイメージ（マルチステージビルド）。
# ...（省略: ステージの説明。本文で書く）

# builder と実行ステージは同じ Debian（bookworm）にそろえる。
# バイナリは glibc に動的リンクされるので、ビルドした環境より古い glibc では動かない。
FROM rust:1.97-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
# ...（省略: キャッシュマウントの説明。本文で書く）
# --locked は Cargo.lock のバージョンをそのまま使う（ビルドのたびに依存が変わらないように）。
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && cp target/release/rust-axum-image-bbs /usr/local/bin/

FROM debian:bookworm-slim
# root ではなく専用ユーザ（uid 1000）で動かす。アプリに乗っ取られても、コンテナ内の root 権限は渡らない。
# 書き込むのは data/（SQLite）と uploads/（画像）だけなので、その2つだけをこのユーザの持ち物にする。
RUN useradd --uid 1000 --user-group --no-create-home app \
    && mkdir -p /app/data /app/uploads \
    && chown app:app /app/data /app/uploads
WORKDIR /app
COPY --from=builder /usr/local/bin/rust-axum-image-bbs /usr/local/bin/rust-axum-image-bbs
USER app
ENV DATABASE_URL=sqlite:///app/data/app.db \
    UPLOAD_DIR=/app/uploads \
    PORT=3000 \
    RUST_LOG=info
# DB と画像はコンテナの外（ボリューム）に置く。コンテナを作り直しても投稿が消えない。
VOLUME ["/app/data", "/app/uploads"]
EXPOSE 3000
CMD ["rust-axum-image-bbs"]
```

`FROM` が2回ある。1つの `Dockerfile` に複数のステージを書き、前のステージの成果物だけを `COPY --from=builder` で後のステージへ持ち込む。これが **multi-stage ビルド** だ。

コンパイラはビルドにしか要らない。builder のベース `rust:1.97-slim-bookworm` は手元で 1.25GB あるが、実行用イメージは `debian:bookworm-slim` にバイナリを1つ足しただけの 126MB になった。中に入られたときに使える道具が少ない、という安全面の効果もある。

### 実行に要るのはバイナリだけ

タスクファイルの案では、実行ステージに `migrations/` と `templates/` もコピーしていた。完成形はバイナリしかコピーしない。どちらもバイナリに入っているからだ。

- テンプレート: Askama は `#[template(path = "index.html")]` の HTML をコンパイル時に Rust コードへ変換する（第6回）
- マイグレーション: `sqlx::migrate!("./migrations")` はコンパイル時マクロで、SQL をバイナリに埋め込む（第3回）
- SQLite: sqlx がライブラリごとバイナリに静的リンクする

手元のバイナリを `ldd`（実行時に読み込む共有ライブラリを表示するコマンド）で見ると、並ぶのは `libgcc_s.so.1` / `libm.so.6` / `libc.so.6` などで、SQLite は無い。`libc.so.6` が glibc で、こちらには動的リンクしている。ビルドした環境より古い glibc の上では動かないので、builder を `rust:1.97-slim-bookworm` と Debian のバージョンまで指定し、実行ステージとそろえた。

タスクファイルの案にあった `pkg-config` / `libssl-dev` / `ca-certificates` は入れていない。このアプリは外へ HTTPS で通信せず、sqlx にも TLS 系の feature を入れていない（第3回）。

### ビルドに DB は要らない

タスクファイルの案には `ENV SQLX_OFFLINE=true` と、「`cargo sqlx prepare` 済みの `.sqlx/` を commit しておく」という注意書きがあった。`SQLX_OFFLINE` は `query!` / `query_as!` マクロのための設定だ。マクロはコンパイル時に SQL を検証するので、ビルド中に生きた DB か、`cargo sqlx prepare` で作ったキャッシュが要る。

このプロジェクトは第5回で、マクロではなく実行時 API（`sqlx::query_as::<_, Post>("...")`）を選んだ。`src/repository/post_repo.rs` の冒頭に、マクロだと「`git clone && cargo run` や Docker ビルドがそのままでは通らない」と理由を書いてある。第5回で払った代償（SQL の誤りが実行時まで分からない）の見返りが、ここで回収される。`SQLX_OFFLINE` は何も変えないので書いていない。

### `rust:1.75` ではなく `rust:1.97`

タスクファイルは builder に `rust:1.75-slim` を指定していた。このバージョンでは今の `Cargo.lock` をビルドできない。

`Cargo.lock` の `uuid 1.29.3` は、自分の `Cargo.toml` で `rust-version = "1.89.0"`（ビルドに要る Rust の最低バージョン）を宣言している。`cargo tree -i uuid` で辿ると、画像形式の判定に使う `infer` が `cfb` 経由で引き込んでいた。コードは `uuid` を直接使わないが、ビルドには 1.89 以上が要る。開発で使っている 1.97 に合わせ、README の動作要件にも 1.89 以上と書いた。

### キャッシュマウント

`RUN --mount=type=cache,target=...` は BuildKit（今の `docker build` の中身）の機能で、指定したディレクトリをビルドをまたいで使い回す。対象はダウンロードしたクレート（`/usr/local/cargo/registry`）と、コンパイル済みの成果物（`/app/target`）だ。

`COPY src ./src` の後の `RUN` は、ソースを1文字変えただけで再実行になり、キャッシュマウントが無ければ依存クレートを全部コンパイルし直す。ビルドログの抜粋で比べる。1回目の終わりはこうだった。

```text
#20 103.8    Compiling sqlx v0.8.6
#20 104.3    Compiling rust-axum-image-bbs v0.1.0 (/app)
#20 115.9     Finished `release` profile [optimized] target(s) in 1m 55s
```

`src/` を変えた後の2回目はこうなった。

```text
#18 0.234    Compiling rust-axum-image-bbs v0.1.0 (/app)
#18 10.33     Finished `release` profile [optimized] target(s) in 10.24s
```

1回目は `Compiling` の行が175行あって1分55秒、2回目は自分のクレートの1行だけで10秒だった。キャッシュマウントの中身はイメージに残らないので、`cargo build` と同じ `RUN` の中でバイナリを `/usr/local/bin/` へコピーしている。`RUN` を分けると、2つ目からは `target/` が見えない。

### uid 1000 の専用ユーザ

何も書かなければ、コンテナの中のプロセスは root で動き、乗っ取られればコンテナの中で何でもできる。実行ステージは `useradd --uid 1000` で `app` ユーザを作り、`USER app` に切り替えてから起動する。書き込む `data/` と `uploads/` だけを `app` の持ち物にした。uid を 1000 にしたのは、多くの Linux で最初に作った一般ユーザが 1000 番だからで、この番号は後でバインドマウントの罠に関わってくる。`ENV` に既定値を入れたので、compose を使わない `docker run` でもそのまま起動する。

## docker-compose.yml

```yaml:docker-compose.yml
# `docker compose up --build` でイメージをビルドして起動する。ブラウザで http://localhost:3000/ を開く。
# ...（省略）
# コンテナの中のアプリは uid 1000 のユーザで動くので、data/ と uploads/ は uid 1000 が書き込める必要がある。
services:
  app:
    build: .
    image: rust-axum-image-bbs
    # PID 1 に小さな init（tini）を置き、`docker compose stop` の SIGTERM をアプリへ届ける。
    # これが無いと、アプリはシグナルを受け取れず、10秒待ってから強制終了される。
    init: true
    ports:
      - "3000:3000"
    volumes:
      - ./data:/app/data
      - ./uploads:/app/uploads
    environment:
      DATABASE_URL: sqlite:///app/data/app.db
      UPLOAD_DIR: /app/uploads
      PORT: "3000"
      RUST_LOG: info
    restart: unless-stopped
```

`./data:/app/data` は、ホストの `./data` をコンテナの `/app/data` に見せる **バインドマウント** だ。アプリが書いた `/app/data/app.db` は、ホストの `data/app.db` になる。`docker compose down` はホストのディレクトリには触らないので、次の `up` で新しいコンテナが同じファイルを開く。

`DATABASE_URL` はスラッシュが3つ続く。`sqlite://` の後ろに絶対パスが付いた形だ。`restart: unless-stopped` は、落ちたら起動し直し、自分で `stop` したときは起動し直さない。古い記事にある `version: "3.8"` の行は、試しに書くと `docker compose config` が ``the attribute `version` is obsolete`` という警告を出したので、書いていない。

### `init: true` と `docker compose stop`

`init: true` にすると、Docker は小さな init プロセス（tini）を PID 1 に置き、アプリをその子として起動する。

Linux では、PID 1 のプロセスに、自分でハンドラを登録していないシグナルが届かない。普通のプロセスなら SIGTERM の既定の動作で終了するが、PID 1 では無視される。このアプリは SIGTERM のハンドラを登録していない。`docker compose stop` は SIGTERM を送って10秒待ち、終わらなければ SIGKILL で強制終了する。tini が PID 1 にいれば、SIGTERM は tini からアプリへ渡り、アプリは既定の動作で終了する。`init` の有無で `docker compose stop` の時間を3回ずつ測った。

| `init` | 1回目 | 2回目 | 3回目 | 終了コード |
|---|---|---|---|---|
| `true` | 0.390 秒 | 0.389 秒 | 0.333 秒 | 143 |
| `false` | 12.152 秒 | 10.251 秒 | 10.457 秒 | 137 |

終了コードは `128 + シグナル番号` で、143 は SIGTERM（15）、137 は SIGKILL（9）だ。`init: false` では毎回10秒待った後に SIGKILL で殺されている。

## .dockerignore は「通すものだけ」を書く

```text:.dockerignore
# docker build に送るファイル（ビルドコンテキスト）を、ビルドに要るものだけに絞る。
# いったん全部を除外し、`!` で必要なものだけ戻す。
# target/（数 GB になる）、.git/、.env（秘密情報が入りうる）、data/、uploads/ などはイメージに入らない。
*
!Cargo.toml
!Cargo.lock
!src/
!migrations/
!templates/
```

`docker build` はカレントディレクトリの中身（**ビルドコンテキスト**）を Docker に送ってから始まり、`.dockerignore` に書いたものは送られない。タスクファイルの案は、除外するもの（`target/`, `.git/`, `.env` など）を並べる書き方だった。これだと、後で `.env.local` を作ればそれがビルドコンテキストに入る。完成形は逆にして、`*` で全部を除外し、`COPY` が使う5つだけを `!` で戻した。

## e2e スクリプトを起動中のコンテナに向ける

前回の `scripts/e2e.sh` は、自分でビルドしてサーバを起動していた。TASK-0019 で、すでに動いているサーバにも向けられるようにした。

```sh:scripts/e2e.sh
PORT="${E2E_PORT:-3999}"
BASE="${E2E_BASE_URL:-http://127.0.0.1:${PORT}}"
# ...（省略）
UPLOADS="${E2E_UPLOAD_DIR:-}"
```

`${E2E_BASE_URL:-...}` は「環境変数が空なら後ろの値を使う」というシェルの書き方で、渡すとビルドと起動を飛ばす。`E2E_UPLOAD_DIR` にはコンテナの `/app/uploads` をホストから見たパスを渡し、拒否した投稿のファイルが残っていないことをファイル数で確かめる。

## ログを仕上げる

ここから TASK-0020 だ。運用で追いたいのは「どのリクエストがどのステータスで何ミリ秒かかったか」と「作成・更新・削除がどの投稿に起きたか」だ。逆に、利用者が入力したタイトルや本文はログに出したくない。

### 初期化: 既定を info にし、`LOG_FORMAT=json` を足す

`Cargo.toml` で `tracing-subscriber` に `json` feature を足し（`features = ["env-filter", "json"]`）、`src/main.rs` の初期化をこう変えた。

```rust:src/main.rs
use std::io::IsTerminal; // `stdout().is_terminal()`（出力先が端末かどうか）を使えるようにするトレイト
// ...（省略）
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(std::io::stdout().is_terminal())
            .init();
    }
```

Rust として見ておきたい点が3つある。

1つ目は `.as_deref()` だ。`std::env::var` は `Result<String, VarError>` を返す。所有権を持つ `String` は、`Ok("json")`（中身は `&str`）とそのまま比べられない。`.as_deref()` は `Result` を消費せずに、中身を `&str` として借りた `Result<&str, &VarError>` を作るので、`== Ok("json")` と比べられる。未設定なら `Err` で `false` になる。

2つ目は、`if` の両側に `.init()` まで書いている理由だ。`.json()` を呼ぶと builder の **型** が変わる。Rust の変数は1つの型しか持てないので、「`if` で builder を作り分けて変数に入れ、後で共通に `.init()`」とは書けない。

3つ目は `use std::io::IsTerminal;` だ。`is_terminal()` はトレイトのメソッドなので、第3回の `FromStr` と同じく、トレイトをスコープに入れないと呼べない。`with_ansi(...)` で色の制御文字を端末のときだけ付けるので、`docker compose logs` にエスケープシーケンスが混ざらない。

### TraceLayer の span と終了時のログを INFO に上げる

第2回の既定は `info,tower_http=debug` だった。`info` にすると、`TraceLayer` の DEBUG のログは見えなくなる。`src/lib.rs` の `build_app` で、出力レベルを上げた。

```rust:src/lib.rs
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
// ...（省略）
    // - ヘッダやボディはログに出さない（DefaultMakeSpan の既定）。タイトルや本文がログに残らない。
    Router::new()
        // ...（省略: ルート定義・fallback・DefaultBodyLimit）
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
```

`make_span_with` でリクエストごとの span `request{method=... uri=... version=...}` を、`on_response` で終了時の `finished processing request latency=... status=...` を INFO にした。span まで上げるのは、span がフィルタで落ちると、中で出したログに `request{...}` の文脈が付かなくなるからだ。試しに `RUST_LOG=info,tower_http=warn cargo run` で投稿すると、`INFO create{post_id=3}: rust_axum_image_bbs::handlers::posts: post created` の1行だけになり、URL が読めなくなった。開始時の行は、終了時の行と情報が重なるので DEBUG のままにした。

### `#[tracing::instrument]` で `post_id` を載せる

作成・更新・削除のハンドラには、関数の実行中ずっと有効な span を作る属性マクロ `#[tracing::instrument]` を付けた。中で出したログには span の名前とフィールドが付く。

```rust:src/handlers/posts.rs
#[tracing::instrument(skip_all, fields(post_id = tracing::field::Empty))]
pub async fn create(
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> AppResult<Response> {
    // ...（省略: multipart を読み、検証に落ちたら 400 でフォームを再表示）

    // 画像のファイル名には投稿 id を使うので、先に画像なしで INSERT して id を決める
    let id = post_repo::insert(&state.db, form.title(), form.text()).await?;
    tracing::Span::current().record("post_id", id);

    // ...（省略: 画像の保存。失敗したら rollback_create で取り消す）

    tracing::info!("post created");
    Ok(Redirect::to(CREATED_REDIRECT).into_response())
}
```

`#[tracing::instrument]` は、何も指定しなければ引数をすべて span に記録する。`AppState` や `Multipart` は記録しても役に立たず、フォームの中身にも近い。タスクファイルの案は `skip(state, multipart)` と名指しで外す形だったが、完成形は `skip_all` で全部外した。引数を後で足しても、外し忘れることがない。

`post_id` は INSERT して初めて決まる。そこで `fields(post_id = tracing::field::Empty)` で空欄として宣言しておき、INSERT の直後に `tracing::Span::current().record("post_id", id)` で埋める。`Span::current()` は今いる span、つまり `create` の span を返す。

更新（`edit_submit`）と削除（`delete`）は URL の `:id` が最初から分かるので、`#[tracing::instrument(skip_all, fields(post_id = id))]` と書いた。`fields` の式は span を作るときに評価されるので、`skip_all` でも `id` を使える。

第9回の、画像の保存に失敗したら投稿を取り消す処理では、原因もログに載せた。

```rust:src/handlers/posts.rs
async fn rollback_create(state: &AppState, id: i64, reason: &AppError) {
    tracing::warn!(
        post_id = id,
        reason = %reason,
        "image could not be saved, rolling back the post"
    );
    // ...（省略: 画像ファイルと行を消す。失敗したら error = ?err で WARN を出す）
}
```

`tracing` のマクロでは、値の前の記号で表示の仕方を選ぶ。`%reason` は `Display`（`{}` の形）、`?err` は `Debug`（`{:?}` の形）で記録する。`AppError::Io` の `Display` は第10回で `#[error("io error: {0}")]` と書いたので、`reason=io error: ...` の1行になる。利用者へのエラーページには内部の詳細を出さないので（第10回）、詳細はこのログにだけ残る。

### ログに出さないものをテストで確かめる

「タイトルと本文がログに出ない」は、`tracing::info!(title = form.title(), ...)` と1行足すだけで崩れる。`tests/integration_logs.rs` で確かめている。1つ目のテストを抜粋する。

```rust:tests/integration_logs.rs
    let (logs, _guard) = common::capture_logs();
    // ...（省略: 「ひみつのタイトル」「ひみつの本文」で作成し、「べつのタイトル」「べつの本文」で編集し、削除する）

    let logs = logs.text();
    let post_id = format!("post_id={id}");
    for event in ["post created", "post updated", "post deleted"] {
        assert!(
            logs.lines()
                .any(|line| line.contains(event) && line.contains(&post_id)),
            "「{event}」の行に {post_id} が付く。ログ:\n{logs}"
        );
    }
    // ...（省略: TraceLayer の行に method=POST / uri=/posts / status=303 が出ることを確かめる）
    for content in [
        "ひみつのタイトル",
        "ひみつの本文",
        "べつのタイトル",
        "べつの本文",
    ] {
        assert!(!logs.contains(content), "{content} はログに出ない");
    }
```

2つ目のテストは、第9回と同じく保存先 `1.png` にディレクトリを置いて保存を失敗させ、WARN の行に `post_id=1` と `reason=io error` が出ることを見る。

`_guard` は drop されるとログ集めを止める値だ。名前の付いた変数はスコープの終わりまで生きるが、`let _ = ...` だとその場で drop されて何も集まらない。集める仕組みと並列実行での失敗の直し方は前回見た。テストは `main.rs` を通らず別の subscriber を使うので、`LOG_FORMAT=json` は手で確かめた。

## README を入口にする

TASK-0020 の前の `README.md` は2行しかなかった。clone した人が最初に開くファイルなので、概要・動作要件・ローカルでの動かし方・環境変数の一覧・ログ・テスト・just のレシピ・Docker・ディレクトリ構成・ドキュメントへのリンクを書いた。

タスクファイルの骨子と違う点が2つある。ローカルの手順に `sqlx database create` と `sqlx migrate run` は無い。DB の作成もマイグレーションも起動時に `main.rs` がやるので（第3回）、`cp .env.example .env` と `cargo run` だけで済む。もう1つは、Docker の節に入れた uid の注意だ。

```markdown:README.md
- コンテナの中のアプリは root ではなく uid 1000 のユーザで動きます。Linux で、リポジトリを置いたユーザの uid が 1000 でない場合は、
  先に `sudo chown 1000:1000 data uploads` で書き込めるようにしてください（`id -u` で自分の uid を確認できます）
```

この罠は「詰まったポイントと気づき」で再現する。

## 動かして確かめる

### Docker で起動して投稿する

同じターミナルで curl を打つので、README の `docker compose up --build` に `-d` を付けて起動する。

```sh
$ docker compose up --build -d
$ docker image ls rust-axum-image-bbs
IMAGE                        ID             DISK USAGE   CONTENT SIZE   EXTRA
rust-axum-image-bbs:latest   051f8aba5df0        126MB         31.6MB        
```

同じ表示で `debian:bookworm-slim` は 116MB なので、差は約 10MB だ。タスクファイルの目安「200〜400MB」より小さい。中身も見る（compose の進捗表示の行は省いた）。

```sh
$ docker compose run --rm app sh -c 'ls -l /usr/local/bin/rust-axum-image-bbs; ls -la /app'
-rwxr-xr-x 1 root root 8543480 Nov 17 15:22 /usr/local/bin/rust-axum-image-bbs
total 16
drwxr-xr-x 4 root root 4096 Nov 17 15:04 .
drwxr-xr-x 1 root root 4096 Nov 17 15:28 ..
drwxr-xr-x 2 app  app  4096 Nov 17 15:27 data
drwxr-xr-x 2 app  app  4096 Nov 17 15:27 uploads
```

バイナリは 8.5MB で、`/app` に `templates/` も `migrations/` も無い。

1x1 ピクセルの PNG（`cat.png`）を用意し、画像つき・画像なし・タイトル空の3件を送る。

```sh
$ curl -s -i -F 'title=はじめてのコンテナ' -F 'text=Docker で動いている掲示板から投稿した' -F 'image=@cat.png' http://localhost:3000/posts
HTTP/1.1 303 See Other
location: /?notice=created
content-length: 0
date: Tue, 17 Nov 2026 15:25:11 GMT

$ curl -s -i -F 'title=画像なし' -F 'text=' http://localhost:3000/posts | head -1
HTTP/1.1 303 See Other
$ curl -s -i -F 'title=' -F 'text=タイトル空' http://localhost:3000/posts | head -1
HTTP/1.1 400 Bad Request
$ docker compose logs --no-log-prefix
2026-11-17T15:25:09.808956Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T15:25:09.809137Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
2026-11-17T15:25:11.812936Z  INFO request{method=POST uri=/posts version=HTTP/1.1}:create{post_id=1}: rust_axum_image_bbs::handlers::posts: post created
2026-11-17T15:25:11.813161Z  INFO request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=38 ms status=303
2026-11-17T15:25:11.848357Z  INFO request{method=POST uri=/posts version=HTTP/1.1}:create{post_id=2}: rust_axum_image_bbs::handlers::posts: post created
2026-11-17T15:25:11.848385Z  INFO request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=17 ms status=303
2026-11-17T15:25:11.857678Z  INFO request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=0 ms status=400
```

`request{...}:create{post_id=1}` が span の入れ子で、外側が `TraceLayer`、内側が `#[instrument]` だ。400 の3件目は検証で落ちて INSERT まで進まないので、`post created` の行は無く、リクエストの行だけが出る。タイトルも本文もログに出ていない。

### アプリが動いているユーザ

```sh
$ docker compose exec app id
uid=1000(app) gid=1000(app) groups=1000(app)
$ docker compose top
SERVICE  #   UID     PID   PPID  C   STIME  TTY  TIME      CMD
app      1   ubuntu  7262  7205  0   15:24  ?    00:00:00  /sbin/docker-init -- rust-axum-image-bbs
app      1   ubuntu  7408  7262  0   15:24  ?    00:00:00  rust-axum-image-bbs
```

`docker compose top` はホストから見た一覧なので、UID の欄にはホストで uid 1000 に付いた名前（この環境では `ubuntu`）が出る。`/sbin/docker-init` が tini で、アプリはその子だ。`init: false` にすると一覧はアプリの1行だけになり、アプリが PID 1 になった。

### down してから up しても投稿が残る

```sh
$ docker compose down
$ docker compose up -d
$ curl -s http://localhost:3000/ | grep -E "card-title|/uploads/"
          <h3 class="h5 card-title">画像なし</h3>
        <img src="/uploads/1.png" class="card-img-top" alt="">
          <h3 class="h5 card-title">はじめてのコンテナ</h3>
$ curl -s -o got.png -w '%{http_code} %{content_type} %{size_download}\n' http://localhost:3000/uploads/1.png
200 image/png 70
$ cmp got.png cat.png && echo same
same
```

コンテナを作り直しても2件の投稿が残り、配信された画像は送った `cat.png` とバイト単位で同じだ。

続けて `data/` と `uploads/` を空にして起動し直し、e2e スクリプトを向けた。

```sh
$ E2E_BASE_URL=http://127.0.0.1:3000 E2E_UPLOAD_DIR=uploads scripts/e2e.sh
```

67件がすべて `ok` になり、最後の行は `passed: 67, failed: 0` だった。`6MB upload is 413`（ボディサイズの上限）や `served image equals the upload`（静的配信）は、ルーターだけを叩く統合テストでは見えない配線だ。

### cargo run でログを見る

Docker を使わずに `cargo run` で起動し、curl で1件作って（`POST /posts`）、編集して（`POST /posts/1/edit`）、削除した（`POST /posts/1/delete`）。最後に一覧を開いた。

```sh
$ cargo run
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s
     Running `target/debug/rust-axum-image-bbs`
2026-11-17T15:27:42.717830Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T15:27:42.718220Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
2026-11-17T15:27:51.567683Z  INFO request{method=POST uri=/posts version=HTTP/1.1}:create{post_id=1}: rust_axum_image_bbs::handlers::posts: post created
2026-11-17T15:27:51.567848Z  INFO request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=7 ms status=303
2026-11-17T15:27:51.584423Z  INFO request{method=POST uri=/posts/1/edit version=HTTP/1.1}:edit_submit{post_id=1}: rust_axum_image_bbs::handlers::posts: post updated
2026-11-17T15:27:51.584540Z  INFO request{method=POST uri=/posts/1/edit version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=4 ms status=303
2026-11-17T15:27:51.594751Z  INFO request{method=POST uri=/posts/1/delete version=HTTP/1.1}:delete{post_id=1}: rust_axum_image_bbs::handlers::posts: post deleted
2026-11-17T15:27:51.594843Z  INFO request{method=POST uri=/posts/1/delete version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=3 ms status=303
2026-11-17T15:27:51.602602Z  INFO request{method=GET uri=/ version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=1 ms status=200
```

`#[instrument]` は名前を指定しなければ関数名を span の名前に使う。`GET /` には付けていないので、リクエストの1行だけが出る。

`LOG_FORMAT=json cargo run` で起動して1件投稿すると、次の2行が出た。`post_id` が 2 なのは、直前に作った id 1 を削除した後だからで、`AUTOINCREMENT`（第3回）により削除した id は再利用されない。

```json
{"timestamp":"2026-11-17T15:28:05.007228Z","level":"INFO","fields":{"message":"post created"},"target":"rust_axum_image_bbs::handlers::posts","span":{"post_id":2,"name":"create"},"spans":[{"method":"POST","uri":"/posts","version":"HTTP/1.1","name":"request"},{"post_id":2,"name":"create"}]}
{"timestamp":"2026-11-17T15:28:05.007645Z","level":"INFO","fields":{"message":"finished processing request","latency":"6 ms","status":303},"target":"tower_http::trace::on_response","span":{"method":"POST","uri":"/posts","version":"HTTP/1.1","name":"request"},"spans":[{"method":"POST","uri":"/posts","version":"HTTP/1.1","name":"request"}]}
```

span は `span`（今いる span）と `spans`（外側から順に全部）に分かれる。`post_id` も `status` も数値で入るので、ログ集約ツールで「`status` が 500 以上」のように絞り込める。

ログのテストも流す。

```sh
$ cargo test --test integration_logs
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.15s
     Running tests/integration_logs.rs (target/debug/deps/integration_logs-304f89b0c911fbc7)

running 2 tests
test rollback_logs_the_post_id_and_the_reason ... ok
test create_edit_and_delete_log_the_post_id_but_not_the_content ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

## 詰まったポイントと気づき

- **uid 1000 でないディレクトリをバインドマウントすると起動しない**: root の持ち物のディレクトリを `data/` と `uploads/` に割り当てて起動すると、こうなった

  ```sh
  $ docker compose ps -a --format 'table {{.Service}}\t{{.Status}}'
  SERVICE   STATUS
  app       Restarting (1) 1 second ago
  $ docker compose logs
  app-1  | Error: Database(SqliteError { code: 14, message: "unable to open database file" })
  app-1  | Error: Database(SqliteError { code: 14, message: "unable to open database file" })
  app-1  | Error: Database(SqliteError { code: 14, message: "unable to open database file" })
  ```

  uid 1000 のアプリは root の持ち物の `data/` に `app.db` を作れず、`restart: unless-stopped` で起動し直しを繰り返す。`sudo chown 1000:1000 data uploads` の後に `docker compose restart` すると、`server listening` が出て 200 が返った
- **同じメッセージでも原因が違う**: 第3回の `unable to open database file` は親ディレクトリが無いせいだった。今回はあるが書き込めない。`ls -ln data` で持ち主の番号を見るのが早い
- **ホストに無いディレクトリは root の持ち物で作られる**: バインドマウントの元が無いと Docker が作り、試すと持ち主は 0（root）で同じエラーになった。`data/` と `uploads/` に `.gitkeep` を置いて clone 時から存在させているのはこのためだ
- **エラーの行にタイムスタンプが無い**: 上のエラーは `tracing` を通っていない。`main` が `?` で `Err` を返して終わると、Rust の実行環境が `Error: {:?}` の形で標準エラーに出す
- **e2e のファイル数に `.gitkeep` が混ざる**: コンテナへ流した最初の回、ファイル数の確認だけが 3 のところ 4 で落ちた。一時ディレクトリを使う普段の実行では起きない。`fe2baf3` で `find ... ! -name .gitkeep` とした

## 連載を終えて

### 13回の一覧

| 回 | タイトル | 中身 |
|---|---|---|
| 1 | [プロジェクト初期化と Hello, axum!](./2026_05_22_rust-axum-imgbbs-01-init.md) | 最小のサーバ |
| 2 | [tracing と TraceLayer で開発体験を整える](./2026_05_23_rust-axum-imgbbs-02-tracing.md) | `tracing` とリクエストのログ |
| 3 | [SQLite + sqlx 接続とマイグレーション](./2026_05_25_rust-axum-imgbbs-03-sqlx-migration.md) | 接続プールと `sqlx::migrate!` |
| 4 | [AppConfig / AppState と環境変数読み込み](./2026_11_17_rust-axum-imgbbs-04-appstate.md) | 設定と共有状態、`.env` |
| 5 | [Post モデルとリポジトリ層を sqlx で書く](./2026_11_17_rust-axum-imgbbs-05-post-repository.md) | 実行時 API のクエリ、最小の `AppError` |
| 6 | [Askama で投稿一覧を表示する](./2026_11_17_rust-axum-imgbbs-06-askama-list.md) | 一覧の HTML と最初の統合テスト |
| 7 | [ページネーションを実装する](./2026_11_17_rust-axum-imgbbs-07-pagination.md) | `?page=` で20件ずつ |
| 8 | [バリデーションと multipart での投稿作成](./2026_11_17_rust-axum-imgbbs-08-validation-create.md) | 検証済みの型と `POST /posts` |
| 9 | [画像アップロードとロールバック](./2026_11_17_rust-axum-imgbbs-09-image-upload.md) | 画像の保存と配信、失敗時の取り消し |
| 10 | [AppError と IntoResponse でエラー処理をまとめる](./2026_11_17_rust-axum-imgbbs-10-app-error.md) | `thiserror` とエラーページ |
| 11 | [編集と削除で CRUD を完成させる](./2026_11_17_rust-axum-imgbbs-11-edit-delete.md) | 差し替え・削除と、その順序 |
| 12 | [統合テストで主要シナリオを固める](./2026_11_17_rust-axum-imgbbs-12-integration-test.md) | 受け入れ基準の対応表と e2e |
| 13 | Docker と README で配布できる形にする | この記事 |

### できあがったもの

旧 PHP 版のユーザ画面と同じことができる掲示板になった。投稿一覧を新しい順に20件ずつ出す。タイトル（50文字まで）・本文（500文字まで）・画像（jpg / gif / png、5MB まで、形式は中身で判定）で投稿できる。編集では画像の差し替えや画像だけの削除ができ、削除は画像ファイルごと消す。

ブランチの先端で測った数字はこうだ。

- `cargo test`: 120件すべて成功（`src/` のユニットテスト49件、`tests/` の7ファイルの統合テスト71件）
- `scripts/e2e.sh`: 起動中のコンテナに対して67件すべて成功
- 実行用イメージ: 126MB（うちバイナリ 8.5MB）
- 行数: `src/` の `.rs` が 2,274 行、`tests/` が 2,008 行（どちらも学習メモを含む）

### 計画から外れたところ

実装は、最初に書いたタスクファイルから何か所かで外れた。理由はその回の記事とコードのコメントに残してある。

- **実行時の sqlx クエリ**（第5回）: 計画は `query_as!` マクロだった。今回の `Dockerfile` が `cargo build` だけで済んだのはこのためだ。SQL の誤りは、全クエリを実際の SQLite に流すユニットテストで拾う
- **`AppError` を最初から**（第5回）: 計画は TASK-0012 だったが、リポジトリの戻り値のために最小の形で作り、必要になった回でバリアントを足し、第10回で `thiserror` にした。リポジトリの関数は第5回から `AppResult` を返したままだ
- **最初のハンドラから `tests/`**（第6回）: 計画では本格的な統合テストは TASK-0017 からだった。ハンドラごとにテストを足したので、第12回は足りない分を埋めて対応表を作る回になった
- **差し替えは「保存 → DB → 古いファイル」**（第11回）: 計画は古い画像を先に消していた。保存に失敗しても、DB が存在しないファイルを指さない
- **削除は「行 → ファイル」**（`288b627`）: 計画はファイルが先だった。ファイルを消せなくても、残るのは誰も参照しないファイルだけになる

### 最後のレビューで直したこと

ここまでの記事を書き終えたあと、完成したコードを通しでレビューし、実際に動かして見つかった不具合を3つ直した（`7a664ec`）。各回の記事はその回の時点のコードを載せているので、ここにまとめておく。

- **同じ形式で画像を差し替えると、ブラウザに古い画像が出る**: `5.png` を別の PNG に差し替えても URL は `/uploads/5.png` のままなので、ブラウザは手元に保存した画像を出し続けた。テンプレートの `src` を `/uploads/{{ path }}?v={{ post.update_date.and_utc().timestamp() }}` にして、更新のたびに URL が変わるようにした
- **本文の改行が表示で消える**: HTML は改行を空白1つとして表示する。本文の `<p>` に `style="white-space: pre-wrap;"` を付けた
- **NUL 文字を含むタイトルで 500 になる**: Rust の `chars().count()` は `"\0abc"` を4文字と数えるが、SQLite の `length()` は NUL の手前で数えるのをやめる。検証を通ったあと DB の CHECK 制約で落ちていた。`validate` で NUL を含む入力を弾くようにした

どれも `cargo test` は通っていた。3件とも、テストに無い入力や操作だったからだ。それぞれテストを足し、修正を外すと落ちることを確かめてある。同じレビューで、編集フォームと再表示時の本文について HTML エスケープのテストが無いことも分かったので、それも足した。テストは113件から120件になった。

### 残っていること

コードから分かる、手を付けていない点だ。

- **認証が無い**: 誰でもどの投稿でも編集・削除できる。旧 PHP 版のログインと管理者機能は、要件（REQ-405）でスコープから外した
- **CSRF 対策が無い**: フォームにトークンが無い。今は認証が無いので誰でも直接 `POST /posts/5/delete` を送れるが、認証を足すなら同時に要る
- **日時が UTC のまま表示される**: `templates/index.html` は `{{ post.regist_date }}` をそのまま出す。第3回で書いた「表示時にローカル変換」は入れていない
- **編集の UPDATE が2回に分かれている**: 差し替えでは `post_repo::update` と `post_repo::update_image_path` を別々に実行し、トランザクションでまとめていない。2つ目だけが失敗すると、タイトルは新しく画像列は古い行が残る
- **保存の後に DB が失敗すると、ファイルが残りうる**: 差し替えで保存した後に UPDATE が失敗すると、拡張子が違えば新しいファイルは参照されずに残る。拡張子が同じならファイルはもう新しい中身で、行だけが古い。残ったファイルは投稿の削除時に消える
- **同じ投稿を同時に編集すると、画像が失われることがある**: 1つの投稿に PNG への差し替えと JPEG への差し替えを同時に送ると、互いに相手のファイルを「不要になった古い拡張子」として消し、行が存在しないファイルを指すことがある。編集どうしを直列化していないためで、直すなら投稿ごとの排他（たとえば `AppState` に `tokio::sync::Mutex` を持たせる）が要る
- **停止時に処理中のリクエストを待たない**: `axum::serve` に `with_graceful_shutdown` を付けていないので、SIGTERM でその場で終わる

## 参考リンク

- [Docker: Multi-stage builds](https://docs.docker.com/build/building/multi-stage/)
- [Dockerfile reference: RUN --mount=type=cache](https://docs.docker.com/reference/dockerfile/#run---mounttypecache)
- [Docker: .dockerignore files](https://docs.docker.com/build/concepts/context/#dockerignore-files)
- [Compose file reference: init](https://docs.docker.com/reference/compose-file/services/#init)
- [Compose file reference: version and name](https://docs.docker.com/reference/compose-file/version-and-name/)
- [`tracing::instrument`（docs.rs）](https://docs.rs/tracing/latest/tracing/attr.instrument.html)
- [`tracing::field::Empty`（docs.rs）](https://docs.rs/tracing/latest/tracing/field/struct.Empty.html)
- [tracing-subscriber の JSON 形式（docs.rs）](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html)
- [tower-http の trace モジュール（docs.rs）](https://docs.rs/tower-http/0.5/tower_http/trace/index.html)
