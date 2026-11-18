---
title: "Rust + axum で画像掲示板を作る (12) 統合テストで主要シナリオを固める"
emoji: "🧪"
type: "tech"
topics: ["rust", "axum", "testing", "sqlx", "tracing"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-11-edit-delete.md) は編集と削除を足して CRUD を完成させた。今回は機能を足さない。これまで1回に1〜2件ずつ見せてきたテストを、**テスト全体** として見直す。

- 単体テストと統合テストの置き場所の違いを確かめる
- `tests/common/mod.rs` の `setup()` と、`oneshot` と multipart のヘルパを読む
- 受け入れ基準の全テストケースとテストの対応表を作る
- 並列実行のときだけ約1割落ちたログのテストを直す
- `scripts/e2e.sh` で、本物のサーバを外から叩く2段目の確認を置く

コミットは `8c5935d`（TASK-0017）、`eb33843`（TASK-0018）、`55efecd`（TASK-0018 の修正）の3つだ。テストのコードはブランチの先頭（`e148b0a`）の時点のものを載せる。

## テストの全体像

| 種類 | 置き場所 | 件数 | 見えるもの |
|---|---|---|---|
| 単体テスト | `src/` の各ファイル末尾の `#[cfg(test)] mod tests` | 48 | そのモジュールの中身（非公開の項目も） |
| 統合テスト | `tests/integration_*.rs` の7ファイル | 65 | ライブラリクレートの `pub` な部分だけ |

単体テストはコードのすぐ横に `#[cfg(test)] mod tests { use super::*; ... }` の形で置く。`#[cfg(test)]` は「`cargo test` のときだけコンパイルする」という印だ。`mod tests` は親モジュールの子なので、`use super::*;` で親の非公開の項目まで取り込める。

統合テストは `tests/` 直下に置く。1ファイルが **別のクレート** としてビルドされ、外部の利用者と同じ立場でライブラリクレートを使う。

```rust:tests/common/mod.rs
use rust_axum_image_bbs::{build_app, config::AppConfig, state::AppState};
```

ここで第5回の `src/lib.rs` と第6回の `build_app` が効いてくる。統合テストが取り込めるのはライブラリクレートだけで、`src/main.rs` のバイナリクレートからは何も取り込めない。`build_app` が `lib.rs` にあるので、テストは `main.rs` と同じルーターを関数呼び出しで手に入れられる。

## `tests/common/mod.rs` は TASK-0007 から育ってきた

タスクファイルの計画では、`tests/common/mod.rs` は TASK-0017 で新しく作るはずだった。実際には最初の HTTP テストを書いた TASK-0007 で作り、機能を足すたびに広げてきた。

| コミット | タスク | 足したもの |
|---|---|---|
| `c7331d2` | TASK-0007 一覧 | `TestApp`、`setup`、`get`、`insert_post` |
| `7e5d6a6` | TASK-0010 作成 | `TestResponse`、`send`、`Part`、`post_multipart`、`post_count` |
| `a149314` | TASK-0011 画像 | `TestResponse` の `bytes` と `content_type`、`upload_files` |
| `eb33843` | TASK-0018 | `CapturedLogs`、`capture_logs` |
| `55efecd` | TASK-0018 の修正 | ログの集め方を作り直す |

TASK-0017 のコミットメッセージにはこうある。

> tests/common/mod.rs の setup() とリクエストヘルパは前のタスクで作成済みなので作り直さない。

各テストは最初から `common::setup()` を使って書かれたので、TASK-0017 でテストをファイル間で移す作業は起きなかった。足したのは、テスト基盤そのものを確かめる `tests/integration_smoke.rs` と、`lint` の変更だけだ。

`tower` は計画の `0.4` ではなく、axum 0.7.9 が依存している `0.5` に揃えた。`Cargo.toml` のコメントによると、同じクレートを二重にビルドしないためだ。

### `common.rs` ではなく `common/mod.rs` にする理由

`tests/` 直下の `.rs` ファイルは、どれも1つのテストクレートになる。ヘルパを `tests/common.rs` に置くと、それもテストクレートとしてビルドされ、`cargo test` の出力に中身の無い `Running tests/common.rs` が並ぶ。サブディレクトリの中のファイルはテストクレートにならない。使う側は各ファイルの先頭で `mod common;` と宣言し、コンパイラが `tests/common/mod.rs` をそのテストクレートの一部としてコンパイルする。

### `#[allow(dead_code)]` が要る理由

`mod common;` は7つのテストクレートでそれぞれコンパイルされる。`integration_smoke.rs` は `upload_files` を使わないので、`integration_smoke` クレートの中では未使用の関数になる。TASK-0017 で `lint` のレシピに `--all-targets` を足し、`tests/` も検査するようにした（コメントは後の TASK-0020 で今の1行になった）。

```make:Justfile
# Lint。tests/ も含めて検査し（--all-targets）、警告はエラー扱いにして見逃さないようにする
lint:
    cargo clippy --all-targets -- -D warnings
```

試しに `upload_files` の `#[allow(dead_code)]` を外して、このコマンドを流した。

```
error: function `upload_files` is never used
   --> tests/common/mod.rs:128:8
    |
128 | pub fn upload_files(app: &TestApp) -> Vec<String> {
    |        ^^^^^^^^^^^^
    |
    = note: `-D dead-code` implied by `-D warnings`
    = help: to override `-D warnings` add `#[expect(dead_code)]` or `#[allow(dead_code)]`

error: could not compile `rust-axum-image-bbs` (test "integration_logs") due to 1 previous error
warning: build failed, waiting for other jobs to finish...
error: could not compile `rust-axum-image-bbs` (test "integration_smoke") due to 1 previous error
error: could not compile `rust-axum-image-bbs` (test "integration_errors") due to 1 previous error
```

使わないテストクレートごとにエラーになる。`setup` のように7ファイルすべてが使う関数には付いていない。

## `setup()` で毎回まっさらなアプリを作る

`setup` の本体を最後に変えたのは `55efecd` で、`init_log_capture()` の行が増えた。

```rust:tests/common/mod.rs
/// テスト1件ぶんのアプリ一式。
pub struct TestApp {
    pub router: Router,
    pub db: SqlitePool,
    // ...（省略：doc コメント）
    #[allow(dead_code)]
    pub upload_dir: TempDir,
}

// ...（省略：doc コメント）
pub async fn setup() -> TestApp {
    // アプリのログを出す前に、ログを集める subscriber を登録しておく（`capture_logs` の説明を参照）
    init_log_capture();
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite に接続できること");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("マイグレーションが適用できること");

    let upload_dir = TempDir::new().expect("一時ディレクトリを作れること");
    let state = AppState {
        db: db.clone(),
        upload_dir: upload_dir.path().to_path_buf(),
        config: AppConfig::from_env_or_default(),
    };

    TestApp {
        router: build_app(state),
        db,
        upload_dir,
    }
}
```

in-memory SQLite に本番と同じマイグレーションを当て、`TempDir` でアップロード先を作り、`build_app` のルーターと一緒に `TestApp` に詰める。in-memory の DB とプールの関係は第5回で書いた。sqlx 0.8.6 のソース（`sqlx-sqlite` の `src/options/parse.rs`）を読むと、`sqlite::memory:` を解釈するたびに、プロセス全体で1つのカウンタを進め、`file:sqlx-in-memory-{番号}` という名前の DB にしている。`setup` を呼ぶたびに別の番号の DB ができる。

`db` フィールドはルーターに渡したのと同じプールなので、テストで `INSERT` した行はハンドラから見える。

`upload_dir` フィールドは `TempDir` を **所有** している。`TempDir` は drop されるとディレクトリを中身ごと消す型だ。`AppState` には、借用の `&Path` から `to_path_buf()` で作った `PathBuf` を渡す。`TempDir` 本体は `TestApp` が持ち続け、テストが終わって `app` が drop されるときにディレクトリが消える。読まれなくても所有していること自体が役目なので、`#[allow(dead_code)]` を付けている。

### テスト同士が干渉しないことをテストする

TASK-0017 で足した `tests/integration_smoke.rs` の2件目だ。

```rust:tests/integration_smoke.rs
#[tokio::test]
async fn two_setups_do_not_share_db_or_upload_dir() {
    let first = common::setup().await;
    let second = common::setup().await;

    // 片方の DB に入れた投稿は、もう片方からは見えない
    common::insert_post(&first.db, "first だけの投稿", "", None).await;
    assert_eq!(common::post_count(&first.db).await, 1);
    assert_eq!(common::post_count(&second.db).await, 0);
    let (_, html) = common::get(&second.router, "/").await;
    assert!(!html.contains("first だけの投稿"));

    // アップロード先も別のディレクトリ
    assert_ne!(first.upload_dir.path(), second.upload_dir.path());

    // TestApp を drop すると、一時ディレクトリも消える（次のテストにファイルを持ち越さない）
    // `to_path_buf()` でパスを複製しておくのは、drop の後で `first` を参照できなくなるから
    let first_dir = first.upload_dir.path().to_path_buf();
    assert!(first_dir.exists());
    drop(first);
    assert!(!first_dir.exists(), "drop で一時ディレクトリが削除される");
    assert!(second.upload_dir.path().exists(), "もう片方は残っている");
}
```

タスクファイルが🟡で挙げていた2つの完了条件を、そのまま確かめている。「`setup` を2回呼ぶと別々の DB とディレクトリができる」と「`TestApp` を drop すると `TempDir` も消える」だ。

最後の4行に所有権のルールが出ている。`first.upload_dir.path()` は `first` の中を指す借用なので、`drop(first)` の後に使えば借用チェッカーがコンパイルエラーにする。そこで drop の前に、`first` から独立した `PathBuf` を作っておく。同じファイルの1件目は、`GET /` が 200 を返すことだけを見るスモークテストだ。

## `oneshot` でルーターを直接呼ぶ

`get` は第6回で見せた。ここでは、ヘッダも返す汎用版の `send` を読む。戻り値の `TestResponse` は、ステータス、`Location`、本文の文字列 `body`、生のバイト列 `bytes`、`Content-Type` を持つ構造体だ。

```rust:tests/common/mod.rs
/// 任意のリクエストをルーターへ投げ、ステータス・Location・本文を返す。
#[allow(dead_code)]
pub async fn send(router: &Router, request: Request<Body>) -> TestResponse {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    // ...（省略：Location と Content-Type のヘッダを読む）
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    TestResponse {
        status,
        location,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        bytes: bytes.to_vec(),
        content_type,
    }
}
```

axum の `Router` は tower の `Service` でもある。`use tower::ServiceExt;` でトレイトをスコープに入れると `oneshot` が使える。リクエストを1つ渡すとルーターを呼び出し、レスポンスを1つ返す。TCP の接続もポートも使わないので、テストを並列に何十件走らせてもポート番号はぶつからない。

`oneshot` は `self` を値で受け取る。呼ぶとルーターの所有権が中へ移り、呼び出し元からは使えなくなる。`router.clone()` で作った複製を渡せば、`TestApp` の `router` は残る。だから1つのテストの中で「投稿する → 一覧を取る → 画像を取る」と何度もリクエストできる。引数を `&Router` にしているので、呼ぶ側は `&app.router` と書くだけでよい。

`from_utf8_lossy` は UTF-8 として読めない部分を置き換え文字にするので、画像のレスポンスでも panic しない。

## multipart のボディを手で組み立てる

投稿の作成と編集は `multipart/form-data` で受け取る。クライアント用のクレートは入れず、ボディをヘルパの中で組み立てている（TASK-0010 で作成）。

```rust:tests/common/mod.rs
/// multipart/form-data の1欄。
#[allow(dead_code)]
pub enum Part<'a> {
    /// テキスト欄: (name, 値)
    Text(&'a str, &'a str),
    /// ファイル欄: (name, ファイル名, 中身)
    File(&'a str, &'a str, &'a [u8]),
}

/// ブラウザのフォーム送信と同じ形の multipart/form-data で `POST uri` する。
#[allow(dead_code)]
pub async fn post_multipart(router: &Router, uri: &str, parts: &[Part<'_>]) -> TestResponse {
    const BOUNDARY: &str = "----test-boundary-7MA4YWxkTrZu0gW";
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match part {
            Part::Text(name, value) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}")
                    .as_bytes(),
            ),
            Part::File(name, filename, bytes) => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                         Content-Type: application/octet-stream\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(bytes);
            }
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    let request = Request::post(uri)
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    send(router, request).await
}
```

multipart のボディは、**境界の文字列**（boundary）で区切った欄の並びだ。タイトルと画像を1つずつ送ると、こうなる。

```
------test-boundary-7MA4YWxkTrZu0gW
Content-Disposition: form-data; name="title"

テスト
------test-boundary-7MA4YWxkTrZu0gW
Content-Disposition: form-data; name="image"; filename="test.jpg"
Content-Type: application/octet-stream

（画像のバイト列）
------test-boundary-7MA4YWxkTrZu0gW--
```

各欄は `--` + 境界の行で始まり、ヘッダ、空行、値と続く。最後は境界の後ろに `--` を付けた行で閉じる。改行はすべて `\r\n` だ。境界の文字列は `Content-Type` ヘッダの `boundary=` でサーバに伝える。ファイル欄の種類は画像でも `application/octet-stream` にしている。サーバは申告された種類ではなく、ファイルの先頭のバイトで形式を判定するからだ（第8回）。

`Part<'a>` は文字列もバイト列も所有せず、呼び出し側の値を借用している（ライフタイム `'a` は前回扱った）。テストは配列リテラルをその場で渡すだけなので、`String` や `Vec<u8>` に複製しなくてよい。

タスクファイルは `multipart_body(boundary, fields, file)` という、ファイルを1つだけ `Option` で受ける形を計画していた。プロジェクトは `Part` の配列にした。欄は書いた順にボディへ並び、ファイルだけのリクエストも同じ関数で書ける。

テスト用の画像ファイル（計画では `tests/fixtures/sample.jpg`）も置いていない。判定に使う先頭のバイトだけを定数にしている。

```rust:tests/integration_create.rs
/// 形式は先頭バイト（マジックナンバー）で判定されるので、後ろは中身を見分けるための適当な文字列。
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0jpeg body";
// ...（省略：GIF / PDF / EXE も同じ形）
```

## 何を確かめるか: ステータス・本文・DB・ディスク

利用者から見える結果は4か所に現れる。

| 確かめる場所 | 例 |
|---|---|
| ステータスコード | 成功は 303、検証エラーは 400、無い投稿は 404 |
| 本文 | エラーメッセージ、残った入力値、一覧に出たタイトル |
| DB の行 | 行が増えたか、`image` 列に何が入ったか |
| ディスク上のファイル | `UPLOAD_DIR` に何が残ったか、中身は送ったものと同じか |

TASK-0018 で足した JPEG のテストが、このうち3か所を一度に見ている。

```rust:tests/integration_create.rs
/// 投稿 id の (title, text, image) を DB から読む。
async fn row(db: &sqlx::SqlitePool, id: i64) -> (String, String, Option<String>) {
    sqlx::query_as("SELECT title, text, image FROM post WHERE id = ?")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn jpeg_with_title_and_text_is_saved_as_id_jpg() {
    let app = common::setup().await;

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "テスト"),
            Part::Text("text", "本文"),
            Part::File("image", "test.jpg", JPEG),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let id = newest_id(&app.db).await;
    let jpg = format!("{id}.jpg");
    assert_eq!(
        row(&app.db, id).await,
        ("テスト".into(), "本文".into(), Some(jpg.clone()))
    );
    assert_eq!(common::upload_files(&app), [jpg.as_str()]);
    assert_eq!(
        std::fs::read(app.upload_dir.path().join(&jpg)).unwrap(),
        JPEG
    );
}
```

`row` は `query_as` で1行をタプルとして受け取るので、構造体を用意しなくてよい。比べる側の `"テスト".into()` は、相手の型から変換先を推論し、`&str` を `String` に変える。

`upload_files` の結果を `[jpg.as_str()]` と比べるので、**ファイルがちょうど1つだけ** あることまで確かめている。一時ファイルが残っていても落ちる。DB だけを見ると「行はあるが画像ファイルが無い」を見逃し、ファイルだけを見ると「ファイルはあるが `image` 列が空」を見逃す。この2つは別々に壊れうるので、両方を確かめる。

偽装ファイルのテストは、3つのケースを `for` で回し、ループの中で毎回 `setup()` を呼ぶ。前のケースの行やファイルが次の検査に混ざらない。各 `assert` の最後の引数に `"{filename}"` を渡し、どのケースで落ちたかが失敗のメッセージで分かるようにしている。

## 受け入れ基準との対応表

TASK-0018 のタスクファイルが挙げた対象は、受け入れ基準のうち8件だった。プロジェクトは `spec/acceptance-criteria.md` の **全37件** を対象にし、テストケース（TC）ごとに、確かめているテストを表にした。表は `docs/tumigidoc/rust-axum-image-bbs/implements/TASK-0018/coverage.md` にある。作成の部分から3行を抜き出す。

| TC | 確かめる内容 | テスト |
|---|---|---|
| TC-004-01 | title + text + jpg で行ができ、`{id}.jpg` に保存、`image` 列に記録 | `integration_create::jpeg_with_title_and_text_is_saved_as_id_jpg` |
| TC-004-E04 | pdf は 400、形式のメッセージ、行もファイルも作られない | `integration_create::other_formats_are_rejected_even_when_named_like_images` |
| TC-004-E07 | 中身が pdf で名前が `.jpg` でも 400 | `integration_create::other_formats_are_rejected_even_when_named_like_images` |

表を作ると、確かめていない TC が分かる。TASK-0018 で足した11件はその穴を埋めたもので、コミットメッセージによると、どれも今のアプリのまま通った。

テストの名前は、タスクファイルの例（`tc_004_01_create_with_jpeg_succeeds`）と違って TC の番号を含まない。名前は「何を確かめるか」を書き、TC との対応は表が持つ。上の表のように、1つのテストが複数の TC を受け持つこともある。

そのままではテストできない TC もある。TC-EDGE-004-01 の「DB ファイル不在で起動」は、起動処理がファイルを作ってマイグレーションも当てるので起きない。テストでは起動処理を通らずに「テーブルの無い DB」でアプリを組み立てる。

表を自動で検査する仕組みは無い。テストの名前を変えたら、表も手で直す。テスト名が実在するかは、後の「動かして確かめる」で調べる。

## ログを確かめるテストと、並列実行で1割落ちた話

TC-EDGE-004-01 は、500 を返すだけでなく、原因を ERROR のログに出すことも求めている。テストでログを読むために、TASK-0018 で `capture_logs` を足した。

```rust:tests/integration_errors.rs
#[tokio::test]
async fn unmigrated_db_returns_500_and_logs_the_cause_as_error() {
    // マイグレーションを当てていない空の DB（post テーブルが無い）でアプリを組み立てる
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    // ...（省略：TempDir を作り、build_app(AppState { db, ... }) で router を組み立てる）
    let (logs, _guard) = common::capture_logs();

    let (status, html) = common::get(&router, "/").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(html.contains("サーバでエラーが発生しました"));
    assert!(!html.contains("no such table"), "原因は利用者には見せない");
    // 運用者が原因を追えるよう、ERROR のログに DB のエラー内容が残る（EDGE-004）
    let logs = logs.text();
    assert!(
        logs.lines()
            .any(|line| line.contains("ERROR") && line.contains("no such table: post")),
        "ログ:\n{logs}"
    );
}
```

マイグレーションを当てない DB が欲しいので、`setup()` を使わずに `build_app` を直接呼んでいる。`_guard` は変数に入れておくこと自体に意味がある。`let _ = ...` と書くと戻り値はその場で drop され、何も集まらない（`capture_logs` のコメント）。

### 最初の形: テストごとに subscriber を付ける

修正前の `eb33843` では、テストごとに専用の subscriber を作っていた。

```rust:tests/common/mod.rs
// ...（省略：doc コメント）
#[allow(dead_code)]
pub fn capture_logs() -> (CapturedLogs, DefaultGuard) {
    let logs = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .finish();
    (logs, tracing::subscriber::set_default(subscriber))
}
```

`CapturedLogs` は `Arc<Mutex<Vec<u8>>>` を包んだ型で、今も同じだ。`Arc` は1つの値を複数の持ち主で共有するポインタで、`clone()` しても中のバッファは複製されない。`Mutex` は書き込みを1度に1人に絞る。subscriber とテストが同じバッファを指すので、テストは後から `logs.text()` で中身を読める。

`set_default` は「このスレッドだけ」の subscriber を設定する。`#[tokio::test]` は既定で1スレッドのランタイムでテストを動かすので、ハンドラの中のログもこのスレッドに届く、という設計だった。

### 並列実行のときだけ落ちる

この形は、`cargo test` で並列に流すとときどき落ちた。修正コミット `55efecd` のメッセージに数字がある。

> 並列で 10/100 回失敗、1スレッドでは 0/100

落ちたときは「ログ:」の後ろが空で、1行も集まっていなかった。原因について、メッセージはこう書いている。

> tracing はログ文ごとに「出す価値があるか」の判定をプロセス全体で覚えるため、並列に走る別のテストが先に判定を済ませると、ログが1行も集まらないことがあった

`tracing::error!(...)` のようなログ文は、ソースコード上の1か所ごとに「この文を聞きたい subscriber はいるか」の判定を覚え、その記憶はプロセス全体で1つだ。別のスレッドが判定を済ませ、それが「誰も聞いていない」で記憶されると、subscriber を付けたテストにもログが届かない。1スレッドで順に流せば割り込むスレッドが無いので、落ちなかった。

### 直した形: subscriber はプロセスに1つ

修正では subscriber をプロセス全体で1つだけ登録し、ログの1行ごとに「今のスレッドのバッファ」へ振り分ける。

```rust:tests/common/mod.rs
thread_local! {
    /// このスレッドのログの行き先。`capture_logs` の間だけ `Some` になる。
    static CAPTURE: RefCell<Option<CapturedLogs>> = const { RefCell::new(None) };
}

// ...（省略：doc コメント）
struct CurrentThreadLogs;

impl<'a> MakeWriter<'a> for CurrentThreadLogs {
    type Writer = Box<dyn std::io::Write>;

    fn make_writer(&'a self) -> Self::Writer {
        match CAPTURE.with(|capture| capture.borrow().clone()) {
            Some(logs) => Box::new(logs),
            None => Box::new(std::io::sink()),
        }
    }
}

// ...（省略：doc コメント。原因の説明は上の引用と同じ）
fn init_log_capture() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(CurrentThreadLogs)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("subscriber を登録できること");
    });
}

/// `capture_logs` の戻り値。drop されると、このスレッドのログ集めをやめる。
pub struct CaptureGuard;

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        CAPTURE.with(|capture| capture.borrow_mut().take());
    }
}

// ...（省略：doc コメント）
#[allow(dead_code)]
pub fn capture_logs() -> (CapturedLogs, CaptureGuard) {
    init_log_capture();
    let logs = CapturedLogs::default();
    CAPTURE.with(|capture| *capture.borrow_mut() = Some(logs.clone()));
    (logs, CaptureGuard)
}
```

`thread_local!` は、スレッドごとに別々の値を持つ静的変数を作るマクロだ。`CAPTURE.with(...)` が渡すのは共有の借用 `&` だけなので、書き換えるために `RefCell`（借用のルールを実行時に確かめる型）で包む。`init_log_capture` は `std::sync::Once` で、プロセスに1回しか成功しない `set_global_default` を最初の1回だけ呼ぶ。

登録した subscriber は、1行書くたびに `make_writer` で書き込み先を受け取る。今のスレッドの `CAPTURE` が `Some` ならそのテストのバッファへ、`None` なら捨てる先の `std::io::sink()` へ書く。2つの分岐は型が違うので、どちらも `Box<dyn std::io::Write>`（「`Write` を実装した何か」を表すトレイトオブジェクト）に包む。

`CaptureGuard` は drop されると `CAPTURE` を `None` に戻す。使い方は最初の形と同じなので、テスト側は書き換えずに済んだ。`setup()` の先頭でも `init_log_capture()` を呼ぶので、どのテストがアプリを動かす前にも subscriber は1つ登録済みだ。修正後は、並列で100回流して失敗は0回になった（コミットメッセージ）。

## 2段目: `scripts/e2e.sh` で本物のサーバを叩く

`oneshot` のテストでは `src/main.rs` の起動処理が1行も動かない。そこで、本物のバイナリを起動して `curl` で叩くスクリプトを置いている。一時ディレクトリに DB とアップロード先を作ってバイナリを起動し、終了時に `trap` で止める。冒頭のコメントによると、狙いは「起動処理・静的配信・ボディサイズ上限など、プロセス全体の配線ミス」に気づくことだ。具体的には次の3つになる。

- 起動処理: 環境変数の読み込み、DB ファイルの作成、マイグレーション、ポートへの bind
- 静的ファイルの配信: 保存した画像を本物の接続越しに `/uploads/...` から取り出す
- ボディの上限: 6MB の画像を本物の接続で送ると 413 になる

`cargo test` が速く細かい1段目、`scripts/e2e.sh` がプロセス全体を通す2段目という分担だ。

## 動かして確かめる

### 全部のテストを流す

ファイルごとの結果の行だけを抜き出す。

```sh
$ cargo test 2>&1 | grep -E "Running|test result"
```

```
     Running unittests src/lib.rs (target/debug/deps/rust_axum_image_bbs-e8b5661699a1d4ac)
test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
     Running unittests src/main.rs (target/debug/deps/rust_axum_image_bbs-1ca2c5d0b2cedf4c)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/integration_create.rs (target/debug/deps/integration_create-184f73e2ea3b4ffb)
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
     Running tests/integration_delete.rs (target/debug/deps/integration_delete-c89a3aed0d94a2cb)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/integration_edit.rs (target/debug/deps/integration_edit-5a9558f1548af692)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
     Running tests/integration_errors.rs (target/debug/deps/integration_errors-33791515692a5c1a)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/integration_list.rs (target/debug/deps/integration_list-8ba8edd08e9a82b3)
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
     Running tests/integration_logs.rs (target/debug/deps/integration_logs-304f89b0c911fbc7)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/integration_smoke.rs (target/debug/deps/integration_smoke-4d72f420039b49e3)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

統合テストのファイルが1つずつ別のバイナリになっている。最後の行はドキュメントテストのぶんだ。

### 1ファイルだけ、1件だけ流す

ファイルを選ぶときは `--test` にファイル名を渡す。

```sh
$ cargo test --test integration_edit
```

```
     Running tests/integration_edit.rs (target/debug/deps/integration_edit-5a9558f1548af692)

running 16 tests
test edit_form_shows_current_values ... ok
test edit_form_for_missing_id_returns_404_page ... ok
test delete_checkbox_on_a_post_without_image_is_accepted ... ok
test edit_form_without_image_has_no_preview ... ok
test delete_image_checkbox_removes_only_the_image ... ok
test edit_of_missing_post_returns_404_page ... ok
test edit_of_title_only_keeps_text_and_image_and_refreshes_update_date ... ok
test edit_updates_title_and_text_and_keeps_the_image ... ok
test edit_with_new_image_of_the_same_extension_overwrites_it ... ok
test edit_with_51_char_title_is_rejected_and_keeps_the_input ... ok
test edit_with_new_image_replaces_it_and_removes_the_old_extension ... ok
test failed_image_save_keeps_the_post_and_its_old_file ... ok
test invalid_edit_keeps_the_image_and_the_checked_checkbox ... ok
test invalid_edit_rerenders_the_form_with_400_and_changes_nothing ... ok
test list_links_each_post_to_its_edit_form ... ok
test new_image_wins_over_the_delete_checkbox ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

1件だけなら、`--test` の後ろにテスト名（の一部）を足す。

```sh
$ cargo test --test integration_create jpeg_with_title
```

```
     Running tests/integration_create.rs (target/debug/deps/integration_create-184f73e2ea3b4ffb)

running 1 test
test jpeg_with_title_and_text_is_saved_as_id_jpg ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 22 filtered out; finished in 0.01s
```
### 対応表のテスト名が実在するか調べる

表から `ファイル::関数` を抜き出し、その関数がファイルにあるかを `grep` で調べる。このコマンドはリポジトリには入っていない。

```sh
$ grep -oE 'integration_[a-z]+::[a-z0-9_]+' docs/tumigidoc/rust-axum-image-bbs/implements/TASK-0018/coverage.md \
  | sort -u | sed 's/::/ /' \
  | while read -r file name; do grep -q "fn $name(" "tests/$file.rs" || echo "missing: $file::$name"; done
```

何も表示されず、表のテスト名はすべて実在した。実在しない名前なら `missing: integration_list::no_such_test` のように出ることも確かめた。

### e2e スクリプトを流す

```sh
$ E2E_PORT=3000 scripts/e2e.sh
```

```
...
== delete
  ok   POST /posts for delete target is 303
  ok   GET /posts/{id}/delete is 405
  ok   POST /posts/{id}/delete is 303
  ok   deleted post is gone from the list
  ok   deleted post's image is removed
  ok   delete of a missing post is 404

passed: 67, failed: 0
```

### わざと壊して、どのテストが落ちるかを見る

`create` ハンドラで、画像の保存に失敗したときの取り消し（第9回）を呼ばないようにする。

```diff:src/handlers/posts.rs
         if let Err(err) = attach_image(&state, id, image).await {
             // 画像を保存できなかった投稿は残さない（EDGE-005）。元のエラー（多くは I/O の 500）を返す
-            rollback_create(&state, id, &err).await;
+            // rollback_create(&state, id, &err).await;
             return Err(err);
```

`cargo test` は、落ちたテストのあるバイナリで止まり、残りを流さない。全部を流すには `--no-fail-fast` を付ける。

```sh
$ cargo test --no-fail-fast
```

まずコンパイラが、`rollback_create` が使われていないという `dead_code` の警告を出した。テストは2件落ちた。1件目は取り消しそのもののテストだ。

```
running 23 tests
test failed_image_save_rolls_back_the_post ... FAILED

failures:

---- failed_image_save_rolls_back_the_post stdout ----

thread 'failed_image_save_rolls_back_the_post' (1282) panicked at tests/integration_create.rs:397:5:
assertion `left == right` failed: 投稿は取り消される
  left: 1
 right: 0
```

保存に失敗したのに行が残っている。`assert_eq!` の3つ目の引数「投稿は取り消される」が、そのまま失敗の理由として出る。

2件目は、TASK-0020 で足した `integration_logs.rs` の `rollback_logs_the_post_id_and_the_reason` だ。assert のメッセージに集めたログ全体が出て、500 の ERROR の行はあるのに、取り消しの WARN の行が無いことが読み取れた。最後のまとめはこうなる。

```
error: 2 targets failed:
    `--test integration_create`
    `--test integration_logs`
```

確かめたら `git checkout -- src/handlers/posts.rs` で元に戻す。

### 修正前の `capture_logs` で、並列のときだけ落ちることを確かめる

`tests/common/mod.rs` を `eb33843` の時点に戻し、`integration_errors` のバイナリを直接起動して、並列と1スレッドで100回ずつ流す。

```sh
$ git show eb33843:tests/common/mod.rs > tests/common/mod.rs
$ cargo test --test integration_errors --no-run
```

```
  Executable tests/integration_errors.rs (target/debug/deps/integration_errors-33791515692a5c1a)
```

ハッシュは環境ごとに違うので、表示された名前を使う。

```sh
$ B=target/debug/deps/integration_errors-33791515692a5c1a
$ p=0; s=0; for i in $(seq 1 100); do $B >/dev/null 2>&1 || p=$((p+1)); $B --test-threads=1 >/dev/null 2>&1 || s=$((s+1)); done; echo "parallel fail=$p single fail=$s"
```

```
parallel fail=7 single fail=0
```

並列では100回中7回落ち、1スレッドでは1回も落ちなかった。コミットメッセージの数字と同じ傾向だ。落ちた回は `unmigrated_db_returns_500_and_logs_the_cause_as_error` が `ログ:` とだけ表示して panic し、その後ろは空だった。`git checkout -- tests/common/mod.rs` で修正後に戻し、同じループを並列だけで流すと `fixed parallel fail=0` だった。

## 詰まったポイントと気づき

- **ログのテストは並列実行のときだけ落ちた**: 1スレッドで流すと通るので、1回試しただけでは気づけない。`55efecd` のメッセージには、並列と1スレッドで100回ずつ流して比べた数字が残っている
- **テストが通っても、確かめたつもりの範囲を確かめていないことがある**: パストラバーサルのテストは `../../evil.png` を送るのに、`upload_dir` の1つ上しか調べていなかった。`../../` が指すのは2つ上だ。`ba2ccf7` で、1つ上と2つ上の両方を調べる形に広げた
- **取り消しの呼び出しを消すと、テストより先にコンパイラが気づいた**: `rollback_create` が未使用になったからだ。呼び出し先が別の場所でも使われていれば警告は出ない。最後に守るのはテストだ

## 次回予告

第13回はシリーズの最終回で、Docker と README で配布できる形にする。

- マルチステージの `Dockerfile` で、ビルド用と実行用のイメージを分ける
- `docker-compose.yml` で、DB とアップロード先をボリュームに置く
- ログに `post_id` などの構造化フィールドを出す（`tests/integration_logs.rs` もここで読む）
- README を整え、`git clone` から起動までの手順をまとめる

- 次回: 「Rust + axum で画像掲示板を作る (13) Docker と README で配布できる形にする」

## 参考リンク

- [The Rust Programming Language: テストの体系化](https://doc.rust-lang.org/book/ch11-03-test-organization.html)
- [`tower::ServiceExt::oneshot` (docs.rs)](https://docs.rs/tower/0.5/tower/trait.ServiceExt.html#method.oneshot)
- [`tempfile::TempDir` (docs.rs)](https://docs.rs/tempfile/3/tempfile/struct.TempDir.html)
- [`tokio::test` (docs.rs)](https://docs.rs/tokio/latest/tokio/attr.test.html)
- [`tracing::subscriber::set_global_default` (docs.rs)](https://docs.rs/tracing/latest/tracing/subscriber/fn.set_global_default.html)
- [`tracing_subscriber::fmt::MakeWriter` (docs.rs)](https://docs.rs/tracing-subscriber/0.3/tracing_subscriber/fmt/trait.MakeWriter.html)
- [`std::thread_local!`](https://doc.rust-lang.org/std/macro.thread_local.html)
- [`std::sync::Once`](https://doc.rust-lang.org/std/sync/struct.Once.html)
