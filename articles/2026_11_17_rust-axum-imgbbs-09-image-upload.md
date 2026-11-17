---
title: "Rust + axum で画像掲示板を作る (9) 画像アップロードとロールバック"
emoji: "🖼️"
type: "tech"
topics: ["rust", "axum", "tokio", "towerhttp", "sqlite"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-08-validation-create.md) は投稿フォームのバリデータと `POST /posts` を作った。画像は中身から形式を判定して `ValidatedImage` に詰めるところまで進めたが、保存はしていなかった。今回はその画像を **ディスクに保存し、`/uploads` で配信する**。

- `src/storage/image_store.rs` に `save_image` / `delete_image` を書く
- `AppError` に `Io` バリアントを足す
- リポジトリに `update_image_path` と `delete` を足す
- `create` を「INSERT → 画像を保存 → `image` 列を UPDATE」の順にし、失敗したら投稿を取り消す
- `ServeDir` を `/uploads` にマウントする
- 保存失敗を確実に起こすテストを書く

コミットは `TASK-0011: 画像を保存して /uploads で配信し、保存失敗時は投稿を取り消す`（`a149314`）の1つだ。

## 保存の順番を決める

画像のファイル名は `{投稿id}.{拡張子}`（例: `5.png`）にする。旧 PHP 版も `img/uploads/{id}.{ext}` と id で名前を付けていた（ただし拡張子は元のファイル名から取っていた）。id は SQLite が INSERT のときに振るので、**INSERT するまでファイル名が決まらない**。そこで順番はこうなる。

1. 画像なし（`image` 列は NULL）で INSERT し、新しい id を受け取る
2. 画像を `{upload_dir}/{id}.{拡張子}` に書き込む
3. `image` 列を UPDATE して、ファイル名を記録する

この順番には弱点がある。2 か 3 で失敗すると、「行はあるのに画像が無い」投稿や、どの行からも指されないファイルが残る。後半でこれを取り消す処理を書く。

## ストレージ層を分ける

DB を触るコードは `repository` に集めた。ファイルを触るコードは新しく `storage` に集める（`src/lib.rs` にも `pub mod storage;` を足す）。

```rust:src/storage/mod.rs
//! ファイルの保存・削除を担当するストレージ層。
//!
//! DB は `repository`、ファイルは `storage` と分けておくと、
//! ハンドラは「どこに・どう書くか」を知らずに「保存して」「消して」と頼むだけで済む。

pub mod image_store;
```

## `save_image` を書く

```rust:src/storage/image_store.rs
//! アップロード画像をディスクに保存・削除する。
//!
//! 保存先は `{upload_dir}/{投稿id}.{拡張子}`。ファイル名は投稿 id と `ImageExt::as_str()` だけから作り、
//! クライアントが送ってきたファイル名は一切使わない。
//! だから `../../etc/passwd` のような名前を送られても、upload_dir の外には書けない（NFR-103）。
//!
//! （学習メモは省略）

use std::io::ErrorKind;
use std::path::Path;

use crate::errors::AppResult;
use crate::models::image_ext::ImageExt;
use crate::validators::post_form::ValidatedImage;

/// 画像を `{upload_dir}/{id}.{拡張子}` に保存し、DB の `image` 列に入れるファイル名（例: `"5.png"`）を返す。
///
/// 学習メモ:
/// - `upload_dir: &Path` は `PathBuf` の借用版（`String` に対する `&str` と同じ関係）。
///   呼び出し側は `&state.upload_dir` と書くだけで `&PathBuf` が `&Path` に自動変換される。
pub async fn save_image(upload_dir: &Path, id: i64, image: &ValidatedImage) -> AppResult<String> {
    // 起動時にも作っているが、実行中に消された場合に備えて毎回作る（既にあれば何もしない）
    tokio::fs::create_dir_all(upload_dir).await?;
    let filename = file_name(id, image.extension());
    // `join` はパスを OS の区切り文字でつなげる（`uploads` + `5.png` → `uploads/5.png`）
    tokio::fs::write(upload_dir.join(&filename), image.bytes()).await?;
    Ok(filename)
}

// ...（delete_image は後述）

/// 保存ファイル名 `{id}.{拡張子}`。ファイル名を作るのはこの関数だけ。
fn file_name(id: i64, ext: ImageExt) -> String {
    format!("{id}.{}", ext.as_str())
}
```

### `&Path` で受け取る

`AppState` は保存先を `upload_dir: PathBuf` で持っている（第4回）。`PathBuf` は中身を所有するパスで、`&Path` はそれを借りて見るだけの参照だ。`save_image` は保存先を読むだけなので、引数は `&Path` にした。呼び出し側が `&state.upload_dir`（型は `&PathBuf`）を渡すと、`&Path` に自動で変換される。`PathBuf` が `Deref<Target = Path>` を実装しているからで、これを **参照外し型強制（deref coercion）** と呼ぶ。`&String` を `&str` の引数に渡せるのと同じ仕組みだ。

戻り値は `"5.png"` というファイル名だけで、`uploads/` は付けない。URL の `/uploads/` は、第6回で書いた `index.html` の `<img src="/uploads/{{ path }}">` が付ける。`TASK-0011.md` にも「戻り値は `{id}.{ext}` 形式」とある。保存先は `UPLOAD_DIR` で変えられるので、ディレクトリ名を DB に焼き込まないほうが移設に強い。

### `tokio::fs` で待つ理由

axum のハンドラは tokio のワーカースレッドの上で動き、1本のスレッドが多数のリクエストを切り替えながら進めている。ここで `std::fs::write` を呼ぶと、書き終わるまでそのスレッドが止まり、同じスレッドの他のリクエストも止まる。5MB の画像なら、その時間は無視できない。

`tokio::fs` の関数は、普通のブロッキングなファイル操作を **別のスレッドプール** で実行する。tokio のドキュメントによると、多くの OS には非同期のファイル API が無いため、`spawn_blocking` 用のスレッドプールで走らせている。ハンドラは `.await` で結果を待ち、その間ワーカースレッドは他のリクエストを処理できる。

### `?` と `AppError::Io`

`tokio::fs::write` のエラー型は `std::io::Error` で、`save_image` の戻り値は `Result<String, AppError>` だ。型が違うのに `?` で返せるのは、変換を足したからである。

```rust:src/errors.rs
/// `std::io::Error` → `AppError` への変換。これで `tokio::fs` の呼び出しにも `?` が使える。
impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err)
    }
}
```

`?` は `Err(e)` を見つけると `From::from(e)` で戻り値のエラー型へ変換してから return する。`sqlx::Error` のときと同じ仕組みだ。enum には `Io(std::io::Error)` を足し、レスポンスへの変換にも腕を足した。

```rust:src/errors.rs
            AppError::Io(err) => {
                // ファイルパスなどの内部情報は利用者に見せず、ログにだけ残す
                tracing::error!(error = %err, "io error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
            }
```

I/O のエラー文には保存先の絶対パスが入ることがある。それはログにだけ残し、利用者には固定の文字列を返す。この時点ではまだ素のテキストで、HTML のエラーページは次回作る。

## ファイル名は id と `ImageExt` だけから作る

`file_name` は `format!("{id}.{}", ext.as_str())` の1行だ。`{id}` は変数をそのまま書式に埋め込む書き方（Rust 1.58 から）で、`{}` には後ろの `ext.as_str()` が入る。ここに入る値は2つしかない。

- `id: i64` … SQLite が振った整数
- `ext.as_str()` … 前回の `ImageExt` が `match` で返す `"jpg"` / `"gif"` / `"png"` のどれか

クライアントが送ってきたファイル名（multipart の `filename="..."`）は、ここに入ってこない。前回の `parse_multipart` は名前を `PostFormInput::image_filename` に取っておくが、`ValidatedImage` にはバイト列と `ImageExt` しか入らないので、`save_image` からは名前が見えない。

だから `../../evil.png` という名前で送られても、保存先がずれる余地がない。`/` や `..` を取り除くサニタイズも要らない。**入力を消毒するのではなく、そもそも使わない** という対策だ。拡張子も同じで、名前が `evil.png` でも中身が JPEG なら `{id}.jpg` で保存される。後の「動かして確かめる」で実際に送ってみる。

## `delete_image` を書く

ロールバックで使うほか、第11回の編集・削除でも使う。

```rust:src/storage/image_store.rs
/// 投稿 id の画像を、拡張子を問わず削除する。ファイルが無くてもエラーにしない。
///
/// 編集で jpg → png に差し替えたとき、古い `5.jpg` を確実に消すために3種類とも試す。
pub async fn delete_image(upload_dir: &Path, id: i64) -> AppResult<()> {
    for ext in [ImageExt::Jpg, ImageExt::Gif, ImageExt::Png] {
        match tokio::fs::remove_file(upload_dir.join(file_name(id, ext))).await {
            Ok(()) => {}
            // 「そもそも無い」は消えているのと同じなので成功扱い（何度呼んでも同じ結果になる）
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            // `err.into()` は `From` 実装を使って `std::io::Error` → `AppError::Io` に変換する
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}
```

### 3つの拡張子を全部試す

`delete_image` は拡張子を引数に取らず、`{id}.jpg`・`{id}.gif`・`{id}.png` を順に消しに行く。ロールバックの時点では `image` 列がまだ NULL なので、DB から拡張子を引けないからだ。`for ext in [...]` は配列を値のまま回し、`ext` には `Copy` な `ImageExt` が1つずつ入る。名前を作るのは削除でも `file_name` だけなので、保存と削除で名前の付け方がずれない。

### 「無い」は成功として扱う

```rust
Err(err) if err.kind() == ErrorKind::NotFound => {}
```

`if` 付きのパターンを **マッチガード** という。「`Err` で、かつ `NotFound` のとき」だけこの腕に入る。無いファイルは「消えている」という目的を達成済みなので、何もせず次へ進む。それ以外の失敗（パスがディレクトリだった、など）は3本目の腕で返す。`err.into()` は `AppError::from(err)` と同じ意味で、`From` を実装すると逆向きの `Into` が自動で使えるようになる。

この扱いのおかげで、`delete_image` は **何度呼んでも同じ結果になる**。ロールバックは「どこまで進んだか分からない状態」から呼ばれるので、これが大事になる。

## リポジトリに `update_image_path` と `delete` を足す

第5回と同じく実行時 API の `sqlx::query(...)` で書く。タスク文書のコード例は `query!` マクロだが、ビルドに生きた DB や `.sqlx/` キャッシュを要求しないというプロジェクトの方針（第5回）を優先した。

```rust:src/repository/post_repo.rs
/// 投稿の画像ファイル名（例: `"5.png"`）を設定する。`None` なら NULL（画像なし）にする。
///
/// 学習メモ:
/// - `Option<&str>` を `.bind()` すると、`Some("5.png")` は文字列、`None` は SQL の NULL として送られる。
pub async fn update_image_path(db: &SqlitePool, id: i64, image: Option<&str>) -> AppResult<()> {
    sqlx::query("UPDATE post SET image = ?, update_date = datetime('now') WHERE id = ?")
        .bind(image)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 投稿を1件削除する。該当行が無くてもエラーにはしない（存在確認は呼び出し側で `find` する）。
pub async fn delete(db: &SqlitePool, id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM post WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
```

`Option<&str>` なので `None` を渡せば NULL（画像なし）に戻せる。今回の呼び出しは `Some(...)` だけだ。`delete` は0行に当たってもエラーにならないので、ロールバックから安全に呼べる。どちらにもユニットテストを足した。

## `create` ハンドラをつなぐ

```rust:src/handlers/posts.rs
pub async fn create(
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> AppResult<Response> {
    // ...（multipart の読み取りと validate は前回と同じなので省略）

    // 画像のファイル名には投稿 id を使うので、先に画像なしで INSERT して id を決める
    let id = post_repo::insert(&state.db, form.title(), form.text()).await?;

    // `if let Some(x) = ...` は Option が Some のときだけ中に入る書き方
    if let Some(image) = form.image() {
        if let Err(err) = attach_image(&state, id, image).await {
            // 画像を保存できなかった投稿は残さない（EDGE-005）。元のエラー（多くは I/O の 500）を返す
            rollback_create(&state, id).await;
            return Err(err);
        }
    }

    Ok(Redirect::to(CREATED_REDIRECT).into_response())
}

/// 画像を `{id}.{拡張子}` で保存し、そのファイル名を投稿の `image` 列に記録する。
async fn attach_image(state: &AppState, id: i64, image: &ValidatedImage) -> AppResult<()> {
    let filename = image_store::save_image(&state.upload_dir, id, image).await?;
    post_repo::update_image_path(&state.db, id, Some(&filename)).await
}
```

前回は `post_repo::insert(...).await?;` と結果を捨てていた。`insert` は前回の時点で `last_insert_rowid()` の値を `AppResult<i64>` で返しているので、今回はそれを `let id` で受け取る。INSERT が失敗したら `?` でそのまま返す。まだ何も作っていないので、取り消すものは無い。

`form.image()` は `Option<&ValidatedImage>` を返す。`if let Some(image) = ...` は `Some` のときだけ中身を取り出して中に入り、画像なしの投稿は素通りする。`image` は借用なので、数 MB のバイト列はコピーされずに `attach_image` → `save_image` → `tokio::fs::write` と渡っていく。

`attach_image` は `save_image` と `update_image_path` を続けて呼ぶだけの関数だ。分けたのは、**どちらで失敗しても同じロールバックに流す** ためである。最後の行にはセミコロンも `?` も無い。Rust では最後の式がそのまま戻り値になり、`update_image_path` の戻り値はちょうど `AppResult<()>` なので、包み直す必要がない。`Some(&filename)` の `&String` が `&str` として渡せるのも、`&PathBuf` → `&Path` と同じ参照外し型強制による。

## 行とファイルは1つのトランザクションに入らない

ここが今回の中心だ。

DB だけなら、INSERT と UPDATE をトランザクションで囲み、失敗したらロールバックすればよい。ところが間にファイルの書き込みが挟まる。**ファイルシステムは SQLite のトランザクションに参加しない**。`BEGIN` の後で書いたファイルは、`ROLLBACK` しても消えない。

そこで、失敗したら自分で元に戻す。これを **補償処理（compensation）** と呼ぶ。

```rust:src/handlers/posts.rs
/// 画像の保存に失敗した投稿を取り消す（補償処理）。書きかけのファイルと INSERT 済みの行を消す。
///
/// SQLite の1文はそれぞれ原子的に実行されるが、「INSERT → ファイル保存 → UPDATE」の3つをまとめて
/// 取り消す仕組みはないので、失敗したら自分で DELETE して元に戻す。
/// ここでの失敗はログにだけ残す。呼び出し元は最初のエラーのほうを利用者に返す。
async fn rollback_create(state: &AppState, id: i64) {
    tracing::warn!(
        post_id = id,
        "image could not be saved, rolling back the post"
    );
    // `if let Err(err) = ...` は「失敗したときだけ」処理する書き方。成功（Ok）なら何もしない
    if let Err(err) = image_store::delete_image(&state.upload_dir, id).await {
        tracing::warn!(post_id = id, error = ?err, "rollback: failed to remove image file");
    }
    if let Err(err) = post_repo::delete(&state.db, id).await {
        tracing::warn!(post_id = id, error = ?err, "rollback: failed to delete post");
    }
}
```

`save_image` で失敗したなら、残っているのは行と、あれば書きかけのファイルだ。`update_image_path` で失敗したなら、行と保存済みのファイルが残っている。どちらでも `rollback_create` は両方を消しに行く。どちらの削除も「無くても成功」なので、どこまで進んでいたかを気にしなくてよい。

利用者に見えるのは 500 だけだ。`create` は取り消しの後、最初のエラー（`attach_image` が返した `err`）をそのまま返す。保存の失敗なら `AppError::Io` なので 500 になる。リダイレクトは返らず、一覧にもその投稿は現れない。取り消しが成功すれば、ディスクにも DB にも、このリクエストが作ったものは残らない。

取り消し自体が失敗することもある。その失敗はログに残すだけにして、利用者には最初のエラーを返す。2つ目のエラーで1つ目を上書きすると、原因を調べられなくなるからだ。`rollback_create` の戻り値が `()` なのは、この「ログに残して先へ進む」を型で表している。

ログの `error = ?err` は `Debug` 形式、`error = %err` は `Display` 形式でフィールドに埋め込む指定だ。この時点の `AppError` は `#[derive(Debug)]` だけなので、`?err` を使う。

取り消した id は再利用されない。`post` テーブルは第3回で `AUTOINCREMENT` 付きにしたので、SQLite は消された id を二度と振らない。もし再利用されると、消し損ねた `3.png` を次の投稿が上書きしかねない。

### タスク文書との違い

`TASK-0011.md` のコード例は `save_image` の失敗だけを取り消し、UPDATE は `?` で返すだけなので、UPDATE が失敗すると保存済みのファイルが残る。実装は `attach_image` で両方をまとめ、`dataflow.md` の「UPDATE 失敗 → ファイル削除して 500 を返す（孤児ファイル防止）」に合わせた。また、タスク文書は `let _ = ...` で取り消しの失敗を黙って捨てるが、実装はログに残す。後で手作業の掃除が要るという合図になるからだ。

## `/uploads` で画像を配信する

配信は自分でハンドラを書かず、`tower-http` の `ServeDir` に任せる。`ServeDir` は `fs` feature に入っている。

```toml:Cargo.toml
#   features = ["fs"] で ServeDir が使えるようになり、アップロード画像のディレクトリをそのまま配信できる。
tower-http = { version = "0.5", features = ["trace", "fs"] }
```

```rust:src/lib.rs
use tower_http::services::ServeDir;

// ...（省略）

pub fn build_app(state: AppState) -> Router {
    // `state` をルーターへムーブする前に、必要な値（usize はコピーされる）を取り出しておく
    let max_body_bytes = state.config.max_image_bytes + MULTIPART_OVERHEAD_BYTES;
    // `/uploads/5.png` → `{upload_dir}/5.png` を返す静的ファイル配信（REQ-013）。
    // ServeDir は `..` を含むパスを拒否するので、upload_dir の外のファイルは読めない。
    // `ServeDir::new` は `AsRef<Path>`（パスとして借用できる型）を受け取り、内部で自分用の `PathBuf` を作る。
    // そのため `&state.upload_dir` と借用で渡せば足り、`state` 本体はこの後 `with_state` へムーブできる。
    let uploads = ServeDir::new(&state.upload_dir);

    // ...（TraceLayer の説明コメントは省略）
    Router::new()
        .route("/", get(posts::list))
        .route("/posts", post(posts::create))
        // `route` は「この URL にこのハンドラ関数」、`nest_service` は「この URL 以下を丸ごとこのサービスに」。
        // ServeDir には `/uploads` を取り除いた残り（`/5.png`）が渡る。
        .nest_service("/uploads", uploads)
        // multipart の読み取りはこの上限で打ち切られる（超えたら 413）
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

### `route` と `nest_service`

`ServeDir` はハンドラ関数ではなく、Tower の `Service` を直接実装した型だ。だから `route(..., get(...))` ではなく、Service を「この URL 以下まるごと」で受け取る `nest_service` で付ける。`/uploads/1.png` なら `/1.png` が届き、`{upload_dir}/1.png` を開く。

### `&state.upload_dir` と借用で渡す理由

`state.upload_dir` を `&` なしで渡すと、`PathBuf` が `state` から `ServeDir` へムーブされ、最後の `.with_state(state)` が書けなくなる。`state` の一部が抜け出た後は、`state` 全体をもう使えないからだ。

`tower-http 0.5.2` の `ServeDir::new` は `P: AsRef<Path>` を受け取る。`AsRef<Path>` は「`&Path` として借りられる型」を表すトレイトで、`PathBuf` も `&PathBuf` も満たす。`ServeDir::new` はそこから中で自分用の `PathBuf` を作るので、呼び出し側が持ち主を手放す必要はない。だから `&state.upload_dir` と借用で渡せば足りる。`state` は何も失わず、この後 `with_state` へムーブできる。タスク文書のコード例は `clone()` で複製していたが、借用で済むのでプロジェクトは複製をやめた。

### `ServeDir` が返すもの

`tower-http 0.5.2` のソースで挙動を確かめた。

- ファイルがあれば 200。`Content-Type` は拡張子から決める（`mime_guess` を使う）
- ファイルが無ければ、本文が空の 404
- パスに `..` があれば、ファイルを探さずに 404

拡張子を中身から決めたので、配信時の `Content-Type` も中身と一致する。

## テストを書く

### `image_store` のユニットテスト

一時ディレクトリには `tempfile` クレートの `TempDir` を使う。`TempDir` は drop されたときにディレクトリごと消えるので、後片付けを書かなくてよい。

:::details image_store のテスト（4件）
```rust:src/storage/image_store.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::validators::post_form::{validate, PostFormInput};
    use tempfile::TempDir;

    /// 検証済みの PNG 画像を作る。`ValidatedImage` はバリデータしか作れないので、本物と同じく validate を通す。
    fn png_image() -> ValidatedImage {
        let input = PostFormInput {
            title: "t".to_string(),
            image_bytes: Some(b"\x89PNG\r\n\x1a\n-png-body".to_vec()),
            ..PostFormInput::default()
        };
        let form = validate(&input, &AppConfig::from_env_or_default()).unwrap();
        form.image().unwrap().clone()
    }

    #[tokio::test]
    async fn save_image_writes_file() {
        let dir = TempDir::new().unwrap();
        let image = png_image();

        let filename = save_image(dir.path(), 1, &image).await.unwrap();

        assert_eq!(filename, "1.png");
        assert_eq!(
            std::fs::read(dir.path().join("1.png")).unwrap(),
            image.bytes()
        );
    }

    #[tokio::test]
    async fn save_image_creates_parent_dir() {
        let dir = TempDir::new().unwrap();
        let upload_dir = dir.path().join("nested").join("uploads");

        save_image(&upload_dir, 7, &png_image()).await.unwrap();

        assert!(upload_dir.join("7.png").is_file());
    }

    #[tokio::test]
    async fn delete_image_removes_existing() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("1.jpg"), b"old").unwrap();
        std::fs::write(dir.path().join("1.png"), b"new").unwrap();
        std::fs::write(dir.path().join("11.jpg"), b"other post").unwrap();

        delete_image(dir.path(), 1).await.unwrap();

        assert!(!dir.path().join("1.jpg").exists());
        assert!(!dir.path().join("1.png").exists());
        // 別の投稿（id=11）の画像は残る
        assert!(dir.path().join("11.jpg").exists());
    }

    #[tokio::test]
    async fn delete_image_ignores_missing() {
        let dir = TempDir::new().unwrap();

        delete_image(dir.path(), 999).await.unwrap();
    }
}
```
:::

`ValidatedImage` はフィールドが非公開で直接は作れない（前回）ので、`png_image()` は `validate` を通して作る。`11.jpg` は、`1` で始まるファイルを巻き込んで消していないことの確認だ。

```sh
$ cargo test image_store
```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.19s
     Running unittests src/lib.rs (target/debug/deps/rust_axum_image_bbs-03610b619014cd4e)

running 4 tests
test storage::image_store::tests::delete_image_ignores_missing ... ok
test storage::image_store::tests::delete_image_removes_existing ... ok
test storage::image_store::tests::save_image_creates_parent_dir ... ok
test storage::image_store::tests::save_image_writes_file ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 34 filtered out; finished in 0.00s
```

### 統合テスト: 保存から配信まで

`tests/integration_create.rs` には5件足した。テスト用の `TestResponse` に本文の生のバイト列 `bytes` と `content_type` を足し、`UPLOAD_DIR` の中身を名前順に返す `common::upload_files` も用意した。1件目は保存から配信までを1本でたどる。

```rust:tests/integration_create.rs
#[tokio::test]
async fn png_is_saved_as_id_png_and_served_from_uploads() {
    let app = common::setup().await;
    let png = png_of_len(1000);

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "画像つき"),
            Part::File("image", "photo.png", &png),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let id = newest_id(&app.db).await;
    let filename = format!("{id}.png");
    assert_eq!(
        image_column(&app.db, id).await.as_deref(),
        Some(filename.as_str())
    );
    // ...（保存先のファイル一覧と中身の比較は省略）

    // 一覧に画像が出て、その URL から保存した中身がそのまま返ってくる（REQ-013）
    let (_, html) = common::get(&app.router, "/").await;
    let src = format!("/uploads/{filename}");
    assert!(html.contains(&format!(r#"src="{src}""#)));
    let served = common::send(&app.router, Request::get(&src).body(Body::empty()).unwrap()).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.content_type.as_deref(), Some("image/png"));
    assert_eq!(served.bytes, png);
}
```

DB の `image` 列、ディスク上のファイル、一覧の `<img src>`、`/uploads` からの配信を、同じ `{id}.png` でつないで確かめている。

2件目は、JPEG を `../../evil.png` という名前で送り、保存先の中身がちょうど `{id}.jpg` の1つになることを見る。残りは 5MB ちょうどの保存と、画像でないファイルの 400 だ。

### 統合テスト: 保存を失敗させる

```rust:tests/integration_create.rs
#[tokio::test]
async fn failed_image_save_rolls_back_the_post() {
    let app = common::setup().await;
    // 最初の投稿は id=1 になる。保存先 `1.png` にディレクトリを置いて、ファイルの書き込みを失敗させる
    // （root で動くテストでも確実に失敗させるため、権限ではなくディレクトリで塞ぐ）
    std::fs::create_dir(app.upload_dir.path().join("1.png")).unwrap();

    let res = common::post_multipart(
        &app.router,
        "/posts",
        &[
            Part::Text("title", "保存失敗"),
            Part::File("image", "a.png", &png_of_len(100)),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(common::post_count(&app.db).await, 0, "投稿は取り消される");
}
```

問題は `save_image` をどう失敗させるかだ。タスク文書は「mock」と書いているが、Rust で関数を差し替えるには trait を切って注入するなどの準備が要る。ここでは本物の `save_image` を本物のファイルシステムで失敗させる。

最初に思いつくのは、保存先を `chmod 555` で書き込み禁止にする方法だ。ところが **root で動くと、書き込みは失敗しない**。root はファイルの権限チェックを素通りするからだ。CI のコンテナや Docker の中では、テストが root で動くことがよくある。手元もちょうど root だった。

```sh
$ id -un
root
$ mkdir ro && chmod 555 ro && echo x > ro/1.png && ls -ld ro ro/1.png
dr-xr-xr-x 2 root root 4096 Nov 17 14:44 ro
-rw-r--r-- 1 root root    2 Nov 17 14:44 ro/1.png
```

書き込み禁止のはずの `ro/` に `1.png` が書けた。権限で塞ぐテストは、一般ユーザーでは通り、root では落ちる。

そこで、書き込み先の `1.png` という名前で **ディレクトリ** を先に作っておく。ディレクトリにファイルとして書き込もうとすると、OS は `Is a directory` で失敗させる。権限ではなくファイルの種類の問題なので、root でも同じく失敗する。テストの DB は毎回空なので、最初の投稿は必ず id=1 になり、塞ぐ名前を決め打ちできる。

```sh
$ cargo test --test integration_create -- png_is_saved saved_extension image_of_exactly non_image_file failed_image_save
```

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.15s
     Running tests/integration_create.rs (target/debug/deps/integration_create-c6f8e6faad8d6b13)

running 5 tests
test failed_image_save_rolls_back_the_post ... ok
test non_image_file_is_rejected_without_row_or_file ... ok
test png_is_saved_as_id_png_and_served_from_uploads ... ok
test saved_extension_comes_from_content_not_file_name ... ok
test image_of_exactly_5mb_is_saved ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.08s
```

`--` の後ろに名前を複数並べると、どれかを含むテストだけが走る。なお、UPDATE で失敗させるテストは無い。UPDATE だけを確実に失敗させる仕掛けが簡単には作れないためで、その経路は `attach_image` が両方の失敗を同じ `Err` で返すことに頼っている。

## 動かして確かめる

DB と保存先を空にしてから起動する。

```sh
$ rm -rf data uploads
$ cargo run
```

```
2026-11-17T14:44:56.249055Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T14:44:56.249408Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

### PNG を投稿して取り出す

1×1 ピクセルの本物の PNG と JPEG を `base64 -d` で書き出す。

```sh
$ echo 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC' | base64 -d > red.png
$ echo '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/wAALCAABAAEBAREA/8QAFAABAAAAAAAAAAAAAAAAAAAAAP/EABQQAQAAAAAAAAAAAAAAAAAAAAD/2gAIAQEAAD8AP//Z' | base64 -d > gray.jpg
$ file red.png gray.jpg
red.png:  PNG image data, 1 x 1, 8-bit/color RGB, non-interlaced
gray.jpg: JPEG image data, JFIF standard 1.01, aspect ratio, density 1x1, segment length 16, baseline, precision 8, 1x1, components 1
```

`curl -F` で multipart/form-data を送る。`image=@red.png` の `@` は「このファイルの中身を送る」という意味だ。

```sh
$ curl -s -i -F title=赤い点 -F text=1x1のPNG -F image=@red.png http://localhost:3000/posts
HTTP/1.1 303 See Other
location: /?notice=created
content-length: 0
date: Tue, 17 Nov 2026 14:45:01 GMT

$ ls uploads
1.png
$ curl -s http://localhost:3000/ | grep -n 'uploads\|赤い点'
44:        <img src="/uploads/1.png" class="card-img-top" alt="">
47:          <h3 class="h5 card-title">赤い点</h3>
```

id=1 の投稿が `1.png` で保存され、一覧の `<img>` がそれを指している。その URL から取り出して、元のファイルと比べる。

```sh
$ curl -s -D - -o served.png http://localhost:3000/uploads/1.png
HTTP/1.1 200 OK
content-type: image/png
accept-ranges: bytes
last-modified: Tue, 17 Nov 2026 14:45:01 GMT
content-length: 69
date: Tue, 17 Nov 2026 14:45:05 GMT

$ cmp red.png served.png && echo same
same
```

`cmp` は2つのファイルが同じなら何も出さずに成功する。送った 69 バイトがそのまま返ってきた。無いファイルは本文が空の 404 になる。

```sh
$ curl -si http://localhost:3000/uploads/999.png
HTTP/1.1 404 Not Found
content-length: 0
date: Tue, 17 Nov 2026 14:45:05 GMT
```

### `../../evil.png` という名前の JPEG を送る

`-F 'image=@gray.jpg;filename=../../evil.png'` と書くと、中身は `gray.jpg`、申告するファイル名は `../../evil.png` になる。その名前が本当に送られたかを、`--trace-ascii` の記録で確かめる。

```sh
$ curl -s -o /dev/null -w "%{http_code}\n" --trace-ascii trace.txt -F title=偽の名前 -F 'image=@gray.jpg;filename=../../evil.png' http://localhost:3000/posts
303
$ grep -a -A1 filename trace.txt
00a2: Content-Disposition: form-data; name="image"; filename="../../ev
00e2: il.png"
$ ls uploads
1.png
2.jpg
```

トレースの表示幅で折り返されているが、`filename="../../evil.png"` がそのまま送られている。それでも保存先は `uploads/2.jpg` だった。`../../` は無視され、拡張子も中身の JPEG に合わせて `.jpg` になった。念のため `find / -name evil.png` でディスク全体を探したが、1件も見つからなかった。

### 保存を失敗させる

最後に、ロールバックを本物のサーバで起こす。今の投稿は2件で、次は id=3 になる。テストと同じく `uploads/3.png` というディレクトリを先に作る。

```sh
$ python3 -c "import sqlite3; print(sqlite3.connect('data/app.db').execute('SELECT COUNT(*) FROM post').fetchone()[0])"
2
$ mkdir uploads/3.png
$ curl -s -i -F title=保存失敗 -F image=@red.png http://localhost:3000/posts
HTTP/1.1 500 Internal Server Error
content-type: text/plain; charset=utf-8
content-length: 21
date: Tue, 17 Nov 2026 14:45:21 GMT

Internal Server Error
$ python3 -c "import sqlite3; print(sqlite3.connect('data/app.db').execute('SELECT COUNT(*) FROM post').fetchone()[0])"
2
$ ls -l uploads
total 12
-rw-r--r-- 1 root root   69 Nov 17 14:45 1.png
-rw-r--r-- 1 root root  159 Nov 17 14:45 2.jpg
drwxr-xr-x 2 root root 4096 Nov 17 14:45 3.png
```

利用者に返るのは素のテキストの `Internal Server Error` だけで、パスやエラーの中身は出ていない。件数は2のままで、INSERT された id=3 の行は消えている。`3.png` は自分で作ったディレクトリが残っているだけだ。サーバのログにはこう出た。

```
2026-11-17T14:45:21.963603Z DEBUG request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_request: started processing request
2026-11-17T14:45:21.966414Z  WARN request{method=POST uri=/posts version=HTTP/1.1}: rust_axum_image_bbs::handlers::posts: image could not be saved, rolling back the post post_id=3
2026-11-17T14:45:21.966674Z  WARN request{method=POST uri=/posts version=HTTP/1.1}: rust_axum_image_bbs::handlers::posts: rollback: failed to remove image file post_id=3 error=Io(Os { code: 21, kind: IsADirectory, message: "Is a directory" })
2026-11-17T14:45:21.968248Z ERROR request{method=POST uri=/posts version=HTTP/1.1}: rust_axum_image_bbs::errors: io error error=Is a directory (os error 21)
2026-11-17T14:45:21.968334Z DEBUG request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=4 ms status=500
2026-11-17T14:45:21.968388Z ERROR request{method=POST uri=/posts version=HTTP/1.1}: tower_http::trace::on_failure: response failed classification=Status code: 500 Internal Server Error latency=4 ms
```

上から読むと、処理の流れがそのまま追える。

1. `image could not be saved, rolling back the post` … `save_image` が失敗し、`rollback_create` に入った
2. `rollback: failed to remove image file` … `delete_image` が `3.png` を消そうとしたが、ディレクトリなので `remove_file` が失敗した（`3.jpg` と `3.gif` は `NotFound` で素通り）。警告だけ出して行の削除へ進む
3. 行の削除には警告が無いので、成功している（件数が2のままなのと一致する）
4. `io error error=Is a directory (os error 21)` … `create` が最初のエラーを返し、`AppError::Io` が 500 にした

2行目は `?err`（`Debug`）なので `Io(Os { ... })` と型の中まで見え、4行目は `%err`（`Display`）なので文だけになる。ディレクトリを消して、もう一度送る。

```sh
$ rmdir uploads/3.png
$ curl -s -o /dev/null -w "%{http_code}\n" -F title=もう一度 -F image=@red.png http://localhost:3000/posts
303
$ python3 - data/app.db <<'EOF'
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
for row in con.execute("SELECT id, title, image FROM post ORDER BY id"):
    print(row)
EOF
(1, '赤い点', '1.png')
(2, '偽の名前', '2.jpg')
(4, 'もう一度', '4.png')
```

新しい投稿は id=4 になった。取り消した id=3 は `AUTOINCREMENT` によって欠番になり、再利用されていない。

## 詰まったポイントと気づき

- **ファイルは DB のトランザクションに入らない**: INSERT と UPDATE をトランザクションで囲んでも、間のファイル書き込みは取り残される。取り消しは自分で書くしかない。取り消し用の関数を「無くても成功」にしておくと、どこまで進んだかを気にせず全部呼べる
- **root ではパーミッションで書き込みを止められない**: 保存失敗のテストを `chmod` で作ると、root で動く CI やコンテナでは書き込みが成功してしまう。書き込み先の名前でディレクトリを作っておくと、root でも確実に失敗する
- **取り消し自体も失敗しうる**: 動作確認では、ディレクトリを `remove_file` できずに取り消しの一部が失敗した。止めずに警告を出して行の削除へ進む作りにしてあったので、行はちゃんと消えた

## 次回予告

第10回は `AppError` を仕上げる。

- `thiserror` で `AppError` の定義を短くし、`Display` も実装する
- バリアントごとにステータスコードを対応させる
- 今回の素のテキストの 500 に代えて、HTML のエラーページを描画する
- どのルートにも当たらない URL に 404 の fallback を足す

- 次回: 「Rust + axum で画像掲示板を作る (10) AppError と IntoResponse でエラー処理をまとめる」

## 参考リンク

- [`tokio::fs` モジュール (docs.rs)](https://docs.rs/tokio/latest/tokio/fs/index.html)
- [`tower_http::services::ServeDir` (docs.rs, 0.5)](https://docs.rs/tower-http/0.5/tower_http/services/struct.ServeDir.html)
- [axum `Router::nest_service` (docs.rs, 0.7)](https://docs.rs/axum/0.7/axum/struct.Router.html#method.nest_service)
- [`std::path::Path`](https://doc.rust-lang.org/std/path/struct.Path.html) / [`std::io::ErrorKind`](https://doc.rust-lang.org/std/io/enum.ErrorKind.html)
- [tempfile crate (docs.rs)](https://docs.rs/tempfile/latest/tempfile/)
- [SQLite — AUTOINCREMENT](https://www.sqlite.org/autoinc.html)
