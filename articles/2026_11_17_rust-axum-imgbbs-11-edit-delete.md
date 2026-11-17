---
title: "Rust + axum で画像掲示板を作る (11) 編集と削除で CRUD を完成させる"
emoji: "✏️"
type: "tech"
topics: ["rust", "axum", "sqlx", "askama", "web"]
published: false
---

## はじめに

[前回](./2026_11_17_rust-axum-imgbbs-10-app-error.md) は `AppError` を `thiserror` で仕上げ、HTML のエラーページと 404 の fallback をそろえた。今回は **編集と削除** を足して、CRUD を完成させる。

- `GET /posts/{id}/edit` で、今の値を入れた編集フォームを出す
- `POST /posts/{id}/edit` で、タイトルと本文を更新し、画像を差し替える
- 「画像を削除する」チェックボックスで、画像だけを消す
- `POST /posts/{id}/delete` で、投稿を画像ファイルごと消す

コミットは `d0fc1c3`（TASK-0013）、`1625b3f`（TASK-0014）、`a419112`（TASK-0015）、`3cea3e7`（TASK-0016）の4つだ。コードは `3cea3e7` の時点のものを載せる。

いちばん考えたのは **画像の差し替えの順番** だ。タスク文書の順番では、保存に失敗したときに古い画像が失われる。

## ルートを足す

```rust:src/lib.rs
        // `:id` はパスパラメータ。ハンドラ側で `Path<i64>` として受け取る
        // 同じ URL でも、GET はフォーム表示、POST は更新処理と、メソッドごとにハンドラを分けられる
        .route(
            "/posts/:id/edit",
            get(posts::edit_form).post(posts::edit_submit),
        )
        // 削除は POST だけ。GET を登録しないので、GET で来たら axum が 405 を返す（NFR-105）
        .route("/posts/:id/delete", post(posts::delete))
```

`get(...)` が返す `MethodRouter` に `.post(...)` をつなげると、同じ URL の GET と POST を別々のハンドラに振り分けられる。削除は `post(...)` だけを登録する。

## 編集フォームを表示する

```rust:src/handlers/posts.rs
/// 学習メモ:
/// - `Path(id): Path<i64>` は、ルート `/posts/:id/edit` の `:id` 部分を i64 に変換して受け取る抽出器。
///   `/posts/abc/edit` のように数値にできない値なら、ハンドラは呼ばれず axum が 400 を返す。
/// - ...（省略）
pub async fn edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<EditView> {
    let post = post_repo::find(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    // `post` はこの後 EditView へムーブするので、フォーム用の値は clone で複製しておく
    let form = PostFormInput {
        title: post.title.clone(),
        text: post.text.clone(),
        ..PostFormInput::default()
    };
    Ok(EditView {
        post,
        form,
        errors: Vec::new(),
    })
}
```

### `Path<i64>` で id を受け取る

`Path<i64>` は、URL のパスの一部を型に変換して受け取る **抽出器** だ。`Path(id)` は `State(state)` と同じパターンの書き方で、包みを開けて中の `i64` を `id` に取り出す。型が `i64` なので、`/posts/abc/edit` はハンドラに届かず、axum が 400 を返す。実際の応答は「動かして確かめる」で見る。

### `Option<Post>` を `ok_or` で 404 にする

`post_repo::find` の戻り値は `AppResult<Option<Post>>` だ。外側の `Result` が「DB に問い合わせられたか」、内側の `Option` が「その id の行があったか」を表す。これを2つの `?` でほどく。

1. `.await?` で `Result` をほどく。DB エラーならここで返る（500）
2. `.ok_or(AppError::NotFound)` で `Option<Post>` を `Result<Post, AppError>` に変える。`None` は `Err(AppError::NotFound)` になる
3. 2つ目の `?` で、`Err` なら返る（404）。`Ok` なら中の `Post` が `post` に入る

`AppError::NotFound` は前回の `IntoResponse` を通って 404 の HTML ページになる。

### `EditView` は `post` と `form` を分けて持つ

```rust:src/templates/mod.rs
/// 学習メモ:
/// - `post`（DB に保存されている今の値）と `form`（フォームに書き戻す値）を分けて持つ。
///   初めて開いたときは `form` に `post` の値をコピーして入れる。
///   バリデーションで差し戻したときは `form` に利用者の入力値を入れ、`post` は画像プレビューと送信先 URL に使う。
#[derive(Template)]
#[template(path = "edit.html")]
pub struct EditView {
    pub post: Post,
    pub form: PostFormInput,
    pub errors: Vec<String>,
}
```

タスク文書の名前は `EditTemplate` だが、`IndexView` と同じく設計書の `EditView` に合わせた。

`edit_form` の `clone` は **所有権** のためだ。`PostFormInput { title: post.title, ... }` と書くと、`String` の所有権が `form` へ移る（ムーブ）。すると `post` は一部が欠け、続く `EditView { post, ... }` で丸ごと渡せない。`clone` で複製すれば `post` は欠けない。`..PostFormInput::default()` は「残りのフィールドは既定値」という構造体更新記法だ。

### `templates/edit.html`

```html:templates/edit.html
<form method="post" action="/posts/{{ post.id }}/edit" enctype="multipart/form-data" class="card card-body mb-4">
  <!-- ...（省略: title / text の欄。値は form.title / form.text） -->
  {% if let Some(img) = post.image %}
  <div class="mb-3">
    <p class="form-label mb-1">現在の画像</p>
    <img src="/uploads/{{ img }}" class="img-fluid img-thumbnail" alt="現在の画像">
  </div>
  <div class="form-check mb-3">
    <input id="delete_image" type="checkbox" name="delete_image" value="on" class="form-check-input"{% if form.delete_image %} checked{% endif %}>
    <label for="delete_image" class="form-check-label">画像を削除する</label>
    <div class="form-text">新しい画像を選んだ場合は、削除ではなく差し替えになります。</div>
  </div>
  {% endif %}
  <!-- ...（省略: 新しい画像の input type="file"） -->
  <div>
    <button type="submit" class="btn btn-primary" onclick="return confirm('登録します。よろしいですか？');">登録</button>
    <a href="/" class="btn btn-secondary">戻る</a>
  </div>
</form>
```

- `{% if let Some(img) = post.image %}` は Rust の `if let` と同じ形で、画像があるときだけプレビューと削除チェックボックスを出す
- `checked` は `form.delete_image` から決めるので、差し戻してもチェック状態が残る
- `return confirm(...)` は「キャンセル」で送信を止める

一覧の各投稿には `<a href="/posts/{{ post.id }}/edit" ...>編集</a>` を足した。

## 編集を保存する

### 作成の部品を使い回す

```rust:src/handlers/posts.rs
pub async fn edit_submit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    multipart: Result<Multipart, MultipartRejection>,
) -> AppResult<Response> {
    // ボディを読む前に存在を確かめる。無い投稿への送信は、中身に関係なく 404
    let post = post_repo::find(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let mut multipart = multipart?;
    let input = parse_multipart(&mut multipart).await?;

    let form = match validate(&input, &state.config) {
        Ok(form) => form,
        Err(AppError::Validation(errors)) => {
            // `post`（DB の値）は送信先 URL と画像プレビューに、`input`（入力値）はフォームの中身に使う
            let view = EditView {
                post,
                form: input,
                errors,
            };
            return Ok((StatusCode::BAD_REQUEST, view).into_response());
        }
        Err(err) => return Err(err),
    };

    // ...（画像の扱い。後述）

    Ok(Redirect::to(UPDATED_REDIRECT).into_response())
}
```

前半は第8回の `create` とほぼ同じで、新しい部品はほとんど無い。

- `parse_multipart` は作成のときのままだ。`delete_image` の欄も「欄があれば `true`」として読んでいた。チェックボックスは ON のときしか送られないからだ
- `validate` も同じものを使う。`&input` を借りるだけなので、失敗しても `input` をフォームに戻せる
- 成功したら 303 で `/?notice=updated` へ戻す

成功メッセージは、第8回の `Notice` enum に `Updated`（「更新しました」）と `Deleted`（「削除しました」）を足しただけだ。

`find` を **multipart を読む前** に置いたので、存在しない投稿への送信は、ボディが何であれ 404 になる（`edit_of_missing_post_returns_404_page`）。

### `post_repo::update`

```rust:src/repository/post_repo.rs
/// 学習メモ:
/// - `update_date = datetime('now')` のように、時刻は Rust 側で作らず SQLite に作らせる。
///   INSERT 時の `DEFAULT (datetime('now'))` と同じ形式・同じ時計になるので、一覧の表示がそろう。
pub async fn update(db: &SqlitePool, id: i64, title: &str, text: &str) -> AppResult<()> {
    sqlx::query("UPDATE post SET title = ?, text = ?, update_date = datetime('now') WHERE id = ?")
        .bind(title)
        .bind(text)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
```

`image` 列は第9回の `update_image_path(db, id, Option<&str>)` で別に更新する。`None` を `.bind()` すると NULL になる。分けておけば、画像を変えないときに `image` 列へ触らない。タスク文書の例は `sqlx::query!` マクロだが、プロジェクトは第5回から実行時の `sqlx::query` で通している。

`datetime('now')` は秒単位なので、INSERT と同じ1秒に UPDATE すると `update_date` の変化を確かめられない。ユニットテストは先に `update_date` を `'2000-01-01 00:00:00'` へずらしてから `update` を呼ぶ。

## 画像の扱いを `ImageChange` で1か所に決める

### タスク文書の形

編集で画像をどうするかは、新しい画像の有無と削除チェックの組み合わせで決まる。

| 新しい画像 | 削除チェック | 動作 |
|---|---|---|
| なし | OFF | `image` 列は変えない |
| あり | どちらでも | 新しい画像に差し替える |
| なし | ON | `image` 列を NULL にし、ファイルを消す |

タスク文書 TASK-0015 の例は、これを `if` と `else if` で書いている（プロジェクトのコードではない）。

```rust
if let Some(image) = validated.image.as_ref() {
    // 新画像あり → 差し替え（delete_image=on は無視）
    image_store::delete_image(&state.upload_dir, id).await?;
    let new_path = image_store::save_image(&state.upload_dir, id, image).await?;
    post_repo::update_image_path(&state.db, id, Some(new_path)).await?;
} else if input.delete_image {
    // 新画像なし + delete_image=on → 画像のみ削除
    image_store::delete_image(&state.upload_dir, id).await?;
    post_repo::update_image_path(&state.db, id, None).await?;
}
```

これでも動くが、条件の解釈がハンドラに散る。別の場所で同じ組み合わせを判定するたびに「両方あったらどちらが勝つか」を書き直すことになり、1か所で間違えると解釈が食い違う。

### プロジェクトの形

```rust:src/validators/post_form.rs
/// 編集で投稿の画像をどう変えるか。`ValidatedPostForm::image_change` だけが決める。
pub enum ImageChange<'a> {
    /// 画像はそのまま。`image` 列もファイルも触らない。
    Keep,
    /// この画像に差し替える。古い画像ファイルは消す（拡張子が変わる場合も）。
    Replace(&'a ValidatedImage),
    /// 画像だけを削除する。`image` 列を NULL にし、ファイルも消す（REQ-103）。
    Remove,
}

impl ValidatedPostForm {
    /// ...（省略: 上の表と同じ内容のコメント）
    /// 新しい画像と削除チェックが両方あるときは、差し替えを優先する（削除チェックは無視）。
    /// 画像を選んだ利用者の意図は「この画像にしたい」なので、それを捨てて画像なしにはしない。
    /// ...（省略: 学習メモ）
    pub fn image_change(&self) -> ImageChange<'_> {
        match (&self.image, self.delete_image) {
            (Some(image), _) => ImageChange::Replace(image),
            (None, true) => ImageChange::Remove,
            (None, false) => ImageChange::Keep,
        }
    }
}
```

`match (&self.image, self.delete_image)` は2つの値を **タプル** にして一度に判定する。各行が表の1行に対応する。1行目の `_` は「削除チェックは何でもよい」で、ここで「両方あれば差し替え」が決まる。`match` は上から試すので、2行目以降に来るのは画像が `None` のときだけだ。

ハンドラは結果を `match` で受けるだけになる。

```rust:src/handlers/posts.rs
    // `match` は ImageChange の全バリアントを書かないとコンパイルできない
    match form.image_change() {
        ImageChange::Keep => {
            post_repo::update(&state.db, id, form.title(), form.text()).await?;
        }
        ImageChange::Replace(image) => {
            let filename = image_store::save_image(&state.upload_dir, id, image).await?;
            post_repo::update(&state.db, id, form.title(), form.text()).await?;
            post_repo::update_image_path(&state.db, id, Some(&filename)).await?;
            remove_image_files(&state, id, Some(image.extension())).await;
        }
        ImageChange::Remove => {
            post_repo::update(&state.db, id, form.title(), form.text()).await?;
            post_repo::update_image_path(&state.db, id, None).await?;
            remove_image_files(&state, id, None).await;
        }
    }
```

真偽値を組み合わせる `if` はもう無い。各アームは「何をどの順に呼ぶか」だけを書く。Rust の `match` は **網羅的** でなければコンパイルが通らないので、バリアントを足せば、このハンドラは扱い忘れのエラーで止まる。

### `ImageChange` が育った順番

TASK-0014 の時点では削除チェックの処理は範囲外で、`ImageChange` は `Keep` と `Replace` の2つだった。`image_change` も `match &self.image { Some(image) => ImageChange::Replace(image), None => ImageChange::Keep }` だけだった。

TASK-0015 で `Remove` を足したときの変更は、enum に1行、`match` をタプルに、ハンドラにアームを1つ、で済んだ。同じコミットで、TASK-0009 からあった `ValidatedPostForm::delete_image()` を消した。外から削除チェックを読む必要がなくなったからだ。

### ライフタイム `'a`

`ImageChange<'a>` の `'a` は **ライフタイム引数** だ。

`Replace(&'a ValidatedImage)` は画像を **借りている** だけで、持ってはいない。画像は数 MB のバイト列なので、持たせると複製（`clone`）が要る。借りれば複製は起きない。

借りものには「貸し主がいつまで生きているか」という制約がつく。貸し主は検証済みフォーム `form` だ。`form` が捨てられた後に中の参照を使えば、もう無いメモリを読むことになるので、Rust はそれを許さない。`'a` は「中の参照は `'a` の期間だけ有効」という名札で、`image_change(&self) -> ImageChange<'_>` の `'_` は「その期間は `&self` を借りている期間と同じ」という意味だ。

`edit_submit` では `form` が関数の最後まで生きていて、`match form.image_change()` はその内側で使い終わるので問題は起きない。関数の引数と戻り値ならライフタイムは省略できることが多いが、enum や構造体の定義に参照を入れるときは書く必要がある。

## 差し替えの順番: 保存 → DB → 古い拡張子を消す

### タスク文書の順番で失うもの

タスク文書 TASK-0014 の例は、差し替えをこの順で行う。

1. タイトルと本文を UPDATE する
2. 古い画像を消す（`delete_image`）
3. 新しい画像を保存する（`save_image`）
4. `image` 列を新しいファイル名に UPDATE する

手順 3 で保存が失敗すると（ディスクがいっぱい、など）、500 が返る時点で手順 1 と 2 は済んでいる。古い画像ファイルはもう無く、戻す手段が無い。`image` 列は古いファイル名のままなので、一覧は存在しないファイルを指す。タイトルと本文だけは新しくなっている。タスク文書は「500 を返してユーザーに再操作させる方針」としていたが、再操作しても消えた画像は戻らない。

### プロジェクトの順番

```rust:src/handlers/posts.rs
/// 画像を差し替えるときの順序（途中で失敗しても、DB が存在しないファイルを指さないようにする）:
/// 1. 新しい画像を保存する。失敗したらここで 500。DB にも古い画像ファイルにもまだ触っていない
/// 2. 保存できてから、タイトル・本文と `image` 列を更新する
/// 3. 最後に、拡張子が変わって不要になった古いファイル（`5.jpg` → `5.png` の `5.jpg`）を消す
```

保存が最初なので、保存に失敗しても何も変わっていない。手順 3 は失敗しても応答を失敗にしない。消し損ねた古いファイルは、もう誰からも参照されないからだ。

```rust:src/handlers/posts.rs
/// 呼ぶのは DB がもうそのファイルを参照しない（またはこれから行ごと消す）とき。
/// 消せなくても残るのは誰も参照しないファイルだけなので、利用者への応答は失敗にせず警告ログを残す。
async fn remove_image_files(state: &AppState, id: i64, keep: Option<ImageExt>) {
    if let Err(err) = image_store::delete_image_except(&state.upload_dir, id, keep).await {
        tracing::warn!(post_id = id, error = ?err, "failed to remove image file");
    }
}
```

戻り値が `()` なのは、失敗を呼び出し側へ伝えないためだ。`?` で伝えると、DB の更新は済んだのに 500 を返すことになる。

### `delete_image_except` で新しいファイルを残す

保存を先にすると別の問題が出る。第9回の `delete_image` は3つの拡張子を全部試して消すので、保存の後に呼ぶと **今保存したファイルまで消す**。そこで「この拡張子は残す」引数を持つ `delete_image_except` を足し、`delete_image` は `keep` が `None` の場合にした。

```rust:src/storage/image_store.rs
pub async fn delete_image(upload_dir: &Path, id: i64) -> AppResult<()> {
    delete_image_except(upload_dir, id, None).await
}

pub async fn delete_image_except(
    upload_dir: &Path,
    id: i64,
    keep: Option<ImageExt>,
) -> AppResult<()> {
    for ext in [ImageExt::Jpg, ImageExt::Gif, ImageExt::Png] {
        // `continue` はループの残りを飛ばして次の拡張子へ進む
        if Some(ext) == keep {
            continue;
        }
        // ...（省略: remove_file。NotFound は成功扱い、それ以外のエラーは return）
    }
    Ok(())
}
```

`Some(ext) == keep` は `Option<ImageExt>` どうしの比較で、`ImageExt` が `PartialEq` を導出しているので書ける。ハンドラは `Some(image.extension())` を渡すので、新しい画像が PNG なら `5.png` を残し、残りの2つを試す。古い拡張子を覚えておく必要はない。

### `save_image` は `.tmp` に書いてから `rename` する

PNG を PNG に差し替えると、新旧のファイル名は同じ `5.png` で、`save_image` が上書きする。第9回の `save_image` は本来のファイル名へ直接書いていたので、書き込みの途中で失敗すると古い画像が途中までの中身で壊れる。保存を先にしても、保存そのものが古いファイルを壊しては意味がない。TASK-0014 でこう変えた。

```rust:src/storage/image_store.rs
    let path = upload_dir.join(&filename);

    // いったん一時ファイルに書き切ってから、`rename` で本来の名前に置き換える。
    // ...（省略）
    // 同じディレクトリ内の `rename` は一瞬で切り替わる（原子的）ので、古い画像か新しい画像のどちらかが必ず残る。
    let tmp = upload_dir.join(format!("{filename}.tmp"));
    let written = match tokio::fs::write(&tmp, image.bytes()).await {
        Ok(()) => tokio::fs::rename(&tmp, &path).await,
        Err(err) => Err(err),
    };
    if let Err(err) = written {
        // 書きかけの一時ファイルを残さない。ここでの失敗より元の失敗を返すほうが大事なので、結果は捨てる
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    Ok(filename)
```

途中で失敗しても壊れるのは `5.png.tmp` で、`5.png` には触っていない。`written` の `match` は「書けたら `rename` の結果、書けなければその失敗」を1つの `Result` にまとめるので、後始末は `.tmp` を消すだけで済む。

### 保存の失敗をテストで固定する

```rust:tests/integration_edit.rs
#[tokio::test]
async fn failed_image_save_keeps_the_post_and_its_old_file() {
    let app = common::setup().await;
    let id = create_with_image(&app, JPEG).await;
    // 新しい画像の保存先 `{id}.png` にディレクトリを置いて、保存を失敗させる
    std::fs::create_dir(app.upload_dir.path().join(format!("{id}.png"))).unwrap();

    let res = common::post_multipart(
        &app.router,
        &format!("/posts/{id}/edit"),
        &[
            Part::Text("title", "新しいタイトル"),
            Part::Text("text", "新しい本文"),
            Part::File("image", "new.png", PNG),
        ],
    )
    .await;

    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(row(&app, id).await, untouched_row(id, "jpg"));
    assert_eq!(upload(&app, &format!("{id}.jpg")), JPEG);
    // 書きかけのファイルも残らない（あるのは古い画像と、塞ぐために置いたディレクトリだけ）
    assert_eq!(
        common::upload_files(&app),
        [format!("{id}.jpg"), format!("{id}.png")]
    );
}
```

保存先にディレクトリを置くと、`rename` はディレクトリをファイルで置き換えられずに失敗する。

テストは、応答が 500 であること、行が作ったときのまま（`untouched_row`）であること、古い `{id}.jpg` が中身まで元のままで `.tmp` も無いことを確かめる。タスク文書の順番なら、タイトルは先に更新され、`{id}.jpg` は保存の前に消えているので、2つ目と3つ目の確認は通らない。`create_with_image` などのヘルパーと `common` の中身は次回見る。

## 画像だけを削除する

`Remove` のアームは `update` → `update_image_path(None)` → `remove_image_files(None)` の順だった。**DB が先、ファイルが後** だ。

```rust:src/handlers/posts.rs
/// 画像だけを削除するときも「DB が先、ファイルが後」。先に `image` 列を NULL にしておけば、
/// ファイルの削除に失敗しても、一覧が消えたファイルを表示しようとすることはない。
```

ファイルを先に消すと、NULL にする UPDATE が失敗したとき、`image` 列が消えたファイルを指したまま残る。DB を先にすれば、残りうるのは誰も指さないファイルだけだ。ファイルの削除に失敗したら警告ログを残し、応答は 303 のままにする。

画像の無い投稿に削除チェックが届いても、エラーにはならない（`delete_checkbox_on_a_post_without_image_is_accepted`）。4通りの組み合わせはユニットテスト `image_change_follows_the_new_image_and_the_delete_checkbox` で押さえた。

## 投稿を削除する

### ハンドラ

```rust:src/handlers/posts.rs
/// 順序は「画像ファイル → DB の行」。ファイルを消せなかった場合も警告ログだけ残して行は消す。
/// 逆に行が残ってファイルだけ消えると、一覧に壊れた画像が出てしまう。誰も参照しないファイルが残るほうがまし。
///
/// 学習メモ:
/// - ...（省略: 405 について。次の節）
/// - 画像の有無にかかわらずファイル削除を呼ぶ。`delete_image_except` は無いファイルを無視するので、
///   画像なしの投稿でも害はなく、過去の失敗で残ったファイルがあれば一緒に片付く。
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    // 値は使わず、存在の確認だけ。無ければ 404
    post_repo::find(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    remove_image_files(&state, id, None).await;
    post_repo::delete(&state.db, id).await?;
    Ok(Redirect::to(DELETED_REDIRECT).into_response())
}
```

存在の確認は編集と同じだ。同じ投稿を2回削除すると、2回目は行が無いので 404 になる。`post_repo::delete` は第9回で作成の取り消し用に作ったものを使った。

タスク文書の例は画像があるときだけファイルを消し、成功後は `/` へ戻していた。プロジェクトは画像の有無を見ずに消し（理由は学習メモのとおり）、`/?notice=deleted` へ戻す。

順番は設計書 `dataflow.md` に合わせた「ファイル → 行」で、ファイルを消せなくても行は消す。統合テスト `delete_removes_the_row_even_if_the_image_cannot_be_removed` は、画像の場所にディレクトリを置いて削除を失敗させ、それでも 303 が返り行が消えることを確かめている。

:::message
画像だけの削除は「DB → ファイル」、投稿の削除は「ファイル → DB」で、順番が逆だ。投稿の削除で `DELETE` の SQL そのものが失敗すると、ファイルはもう無く行だけが残る。doc コメントが避けたがっている「行が残ってファイルだけ消える」状態だ。`Remove` と同じ「DB → ファイル」なら、残るのは誰も指さないファイルだけになる。
:::

### GET は 405 になる

ルートには `post(posts::delete)` しか登録していない。GET で来ると、axum は「パスは合っているがメソッドが合わない」と判断し、**405 Method Not Allowed** と `allow: POST` ヘッダを自動で返す。

旧 PHP 版は `admin_delete.php?id=N` という GET のリンクで削除していた。GET で削除できると、リンクを踏んだりクローラがたどったりしただけで投稿が消える。削除を POST だけにしたのは旧版からの改善点である（NFR-105）。統合テスト `get_delete_returns_405_and_deletes_nothing` は、405 に加えて行も画像も残っていることを確かめる。

### 削除フォームと `confirm()`

```html:templates/index.html
            <a href="/posts/{{ post.id }}/edit" class="btn btn-primary btn-sm">編集</a>
            {# confirm で「キャンセル」を選ぶと false が返り、送信が止まる（REQ-102） #}
            <form method="post" action="/posts/{{ post.id }}/delete" class="d-inline"
                  onsubmit="return confirm('IDが「{{ post.id }}」のデータを削除します。よろしいですか？');">
              <button type="submit" class="btn btn-danger btn-sm">削除</button>
            </form>
```

POST で送るには `<a>` ではなく `<form method="post">` が要る。ボディは空で、サーバは URL の id だけを使う。`onsubmit` が `confirm()` の `false` を返すと、ブラウザは送信しない。

タスク文書の例は、隠しフォームと `msgDelete(id)` という関数を組み合わせていた。プロジェクトはフォーム自身の `onsubmit` に確認を書いたので、どちらも要らない。

## テストを流す

```sh
$ cargo test --test integration_edit
```

```
     Running tests/integration_edit.rs (target/debug/deps/integration_edit-434cf788717a1b03)

running 14 tests
test edit_form_for_missing_id_returns_404_page ... ok
test edit_form_shows_current_values ... ok
test delete_checkbox_on_a_post_without_image_is_accepted ... ok
test edit_form_without_image_has_no_preview ... ok
test edit_of_missing_post_returns_404_page ... ok
test delete_image_checkbox_removes_only_the_image ... ok
test edit_updates_title_and_text_and_keeps_the_image ... ok
test failed_image_save_keeps_the_post_and_its_old_file ... ok
test edit_with_new_image_of_the_same_extension_overwrites_it ... ok
test edit_with_new_image_replaces_it_and_removes_the_old_extension ... ok
test invalid_edit_rerenders_the_form_with_400_and_changes_nothing ... ok
test list_links_each_post_to_its_edit_form ... ok
test invalid_edit_keeps_the_image_and_the_checked_checkbox ... ok
test new_image_wins_over_the_delete_checkbox ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

```sh
$ cargo test --test integration_delete
```

```
     Running tests/integration_delete.rs (target/debug/deps/integration_delete-3670d95a37de5ff3)

running 6 tests
test delete_of_missing_post_returns_404_page ... ok
test delete_of_post_without_image_succeeds ... ok
test delete_removes_the_row_even_if_the_image_cannot_be_removed ... ok
test list_has_a_confirmed_delete_form_for_each_post ... ok
test get_delete_returns_405_and_deletes_nothing ... ok
test delete_removes_the_post_and_its_image ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

## 動かして確かめる

```sh
$ rm -rf data uploads
$ cargo run
```

```
2026-11-17T14:56:37.363019Z  INFO rust_axum_image_bbs: migrations applied
2026-11-17T14:56:37.363410Z  INFO rust_axum_image_bbs: server listening on http://0.0.0.0:3000
```

### 編集フォームを開く

1×1 ピクセルの PNG と JPEG を `base64 -d` で作り、PNG つきの投稿を作る。

```sh
$ echo 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==' | base64 -d > dot.png
$ echo '/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=' | base64 -d > dot.jpg
$ curl -si -F title=最初のタイトル -F text=最初の本文 -F image=@dot.png http://localhost:3000/posts
HTTP/1.1 303 See Other
location: /?notice=created
content-length: 0
date: Tue, 17 Nov 2026 14:56:44 GMT

$ ls uploads
1.png
$ curl -si http://localhost:3000/posts/1/edit
HTTP/1.1 200 OK
content-type: text/html; charset=utf-8
...
    <input id="title" name="title" class="form-control" maxlength="50" value="最初のタイトル">
...
    <textarea id="text" name="text" class="form-control" rows="4" maxlength="500">最初の本文</textarea>
...
    <img src="/uploads/1.png" class="img-fluid img-thumbnail" alt="現在の画像">
...
$ curl -si http://localhost:3000/posts/999/edit | head -3
HTTP/1.1 404 Not Found
content-type: text/html; charset=utf-8
content-length: 762
$ curl -s http://localhost:3000/posts/999/edit | grep -n 'ページが見つかりません'
18:    <li>ページが見つかりません</li>
$ curl -si http://localhost:3000/posts/abc/edit
HTTP/1.1 400 Bad Request
content-type: text/plain; charset=utf-8
content-length: 44
date: Tue, 17 Nov 2026 14:56:46 GMT

Invalid URL: Cannot parse `"abc"` to a `i64`
```

999 は `ok_or(AppError::NotFound)` を通った HTML の 404 ページだ。`abc` は `text/plain` の 400 で、本文は axum の `Path` が返す文面である。ハンドラより前で弾かれるので、`AppError` を通らない。

### 文章を直し、PNG を JPEG に差し替える

```sh
$ curl -si -F title=直したタイトル -F text=直した本文 http://localhost:3000/posts/1/edit
HTTP/1.1 303 See Other
location: /?notice=updated
content-length: 0
date: Tue, 17 Nov 2026 14:56:52 GMT

$ curl -s 'http://localhost:3000/?notice=updated' | grep -n 'alert-success\|直した\|uploads/'
16:<div class="alert alert-success" role="status">更新しました</div>
46:        <img src="/uploads/1.png" class="card-img-top" alt="">
49:          <h3 class="h5 card-title">直したタイトル</h3>
50:          <p class="card-text">直した本文</p>
$ curl -si -F title=直したタイトル -F text=画像をJPEGに -F image=@dot.jpg http://localhost:3000/posts/1/edit
HTTP/1.1 303 See Other
location: /?notice=updated
content-length: 0
date: Tue, 17 Nov 2026 14:56:52 GMT

$ ls uploads
1.jpg
$ curl -s -o /dev/null -w '%{http_code} /uploads/1.png\n' http://localhost:3000/uploads/1.png
404 /uploads/1.png
$ curl -s -o /dev/null -w '%{http_code} /uploads/1.jpg\n' http://localhost:3000/uploads/1.jpg
200 /uploads/1.jpg
$ python3 -c "import sqlite3; print(sqlite3.connect('data/app.db').execute('SELECT id, title, text, image, regist_date, update_date FROM post').fetchall())"
[(1, '直したタイトル', '画像をJPEGに', '1.jpg', '2026-11-17 14:56:44', '2026-11-17 14:56:52')]
```

文章だけなら `Keep` で画像は `1.png` のまま、JPEG を送ると `1.jpg` に替わり、古い `1.png` は 404 になった。

### 画像だけを消し、タイトルを空にする

```sh
$ curl -si -F title=直したタイトル -F text=画像だけ消す -F delete_image=on http://localhost:3000/posts/1/edit
HTTP/1.1 303 See Other
location: /?notice=updated
content-length: 0
date: Tue, 17 Nov 2026 14:56:57 GMT

$ ls -A uploads
$ python3 -c "import sqlite3; print(sqlite3.connect('data/app.db').execute('SELECT id, text, image FROM post').fetchall())"
[(1, '画像だけ消す', None)]
$ curl -si -F title= -F text=入力した本文 http://localhost:3000/posts/1/edit
HTTP/1.1 400 Bad Request
content-type: text/html; charset=utf-8
content-length: 1640
...
    <li>タイトルは必須です</li>
...
    <textarea id="text" name="text" class="form-control" rows="4" maxlength="500">入力した本文</textarea>
...
```

`image` 列は NULL（Python では `None`）になり、ファイルも消えた。空のタイトルは 400 で、入力した本文が残る。

### 投稿を削除する

削除用に PNG つきの投稿を作る（id=2）。

```sh
$ curl -s -o /dev/null -w '%{http_code}\n' -F title=消す投稿 -F image=@dot.png http://localhost:3000/posts
303
$ curl -si http://localhost:3000/posts/2/delete
HTTP/1.1 405 Method Not Allowed
allow: POST
content-length: 0
date: Tue, 17 Nov 2026 14:57:02 GMT

$ ls uploads
2.png
$ curl -si -X POST http://localhost:3000/posts/2/delete
HTTP/1.1 303 See Other
location: /?notice=deleted
content-length: 0
date: Tue, 17 Nov 2026 14:57:02 GMT

$ ls -A uploads
$ python3 -c "import sqlite3; print(sqlite3.connect('data/app.db').execute('SELECT id, title, image FROM post').fetchall())"
[(1, '直したタイトル', None)]
$ curl -si -X POST http://localhost:3000/posts/2/delete | head -3
HTTP/1.1 404 Not Found
content-type: text/html; charset=utf-8
content-length: 762
```

GET は 405 で、`2.png` は残っている。POST で行と `2.png` が一緒に消え、2回目は 404 になった。

### ファイルを消せないとき

JPEG つきの投稿（id=3）を作り、`uploads/3.gif` にディレクトリを置いてから画像だけを消す。

```sh
$ curl -s -o /dev/null -w '%{http_code}\n' -F title=JPEGの投稿 -F image=@dot.jpg http://localhost:3000/posts
303
$ mkdir uploads/3.gif
$ curl -s -o /dev/null -w '%{http_code}\n' -F title=JPEGの投稿 -F delete_image=on http://localhost:3000/posts/3/edit
303
$ ls -A uploads
3.gif
$ python3 -c "import sqlite3; print(sqlite3.connect('data/app.db').execute('SELECT id, title, image FROM post WHERE id = 3').fetchall())"
[(3, 'JPEGの投稿', None)]
$ rmdir uploads/3.gif
```

応答は 303 で、`image` 列は NULL、`3.jpg` も消えた。ディレクトリの `3.gif` で削除が止まり、サーバのログに WARN が出る。

```
2026-11-17T14:57:16.292374Z  WARN request{method=POST uri=/posts/3/edit version=HTTP/1.1}: rust_axum_image_bbs::handlers::posts: failed to remove image file post_id=3 error=Io(Os { code: 21, kind: IsADirectory, message: "Is a directory" })
```

DB を先に NULL にしたので、一覧が消えたファイルを指すことはない。

### e2e スクリプト

最後に `scripts/e2e.sh` を流す。一時ディレクトリでサーバを起動し、作成から削除までを `curl` で確かめるスクリプトだ。ポートは `E2E_PORT` で選べる（既定は 3999）。

```sh
$ scripts/e2e.sh
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

## 詰まったポイントと気づき

- **タスク文書の順番では、保存に失敗すると古い画像が消える**: 「保存 → DB → 古い拡張子を消す」に入れ替え、`failed_image_save_keeps_the_post_and_its_old_file` で固定した
- **保存を先にすると、全部消す `delete_image` では新しいファイルまで消える**: 後片付けに「この拡張子は残す」引数が要り、`delete_image_except` を足した
- **同じ拡張子の差し替えは、上書きそのものが古い画像を壊しうる**: 順番だけでは守れないので、`save_image` を「`.tmp` に書いて `rename`」に変えた。作成だけのうちは見えなかった問題だ
- **`Path<i64>` の 400 は HTML のエラーページにならない**: ハンドラより前で axum が弾くので、前回の `AppError` のページは使われず、`text/plain` の文面がそのまま返る
- **削除と画像だけの削除で、DB とファイルの順番が逆**: 投稿の削除は「ファイル → DB」で、`DELETE` が失敗すると doc コメントが避けたがっている状態になる。今のコードに残っている点として記録しておく

## 次回予告

第12回は、統合テストを全体として見直し、主要なシナリオを固める。

- `tests/common` の `setup()` が、in-memory SQLite と一時アップロードディレクトリで毎回まっさらなアプリを作る仕組み
- ポートを開かず、ルーターを `oneshot` で直接呼ぶテストの書き方
- テストの中で multipart のリクエストを組み立てる方法
- 受け入れ基準とテストの対応表

今回説明なしで使った `common::setup()` や `common::post_multipart` の中身を開ける。

- 次回: 「Rust + axum で画像掲示板を作る (12) 統合テストで主要シナリオを固める」

## 参考リンク

- [axum 0.7 `extract::Path` (docs.rs)](https://docs.rs/axum/0.7/axum/extract/struct.Path.html)
- [axum 0.7 `routing::MethodRouter` (docs.rs)](https://docs.rs/axum/0.7/axum/routing/method_routing/struct.MethodRouter.html)
- [`Option::ok_or`](https://doc.rust-lang.org/std/option/enum.Option.html#method.ok_or)
- [`tokio::fs::rename` (docs.rs)](https://docs.rs/tokio/latest/tokio/fs/fn.rename.html)
- [The Rust Programming Language: ライフタイムで参照を検証する](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html)
- [The Rust Programming Language: `match` は網羅的](https://doc.rust-lang.org/book/ch06-02-match.html#matches-are-exhaustive)
- [askama 0.12 (docs.rs)](https://docs.rs/askama/0.12/askama/)
- [MDN: `Window.confirm()`](https://developer.mozilla.org/ja/docs/Web/API/Window/confirm)
